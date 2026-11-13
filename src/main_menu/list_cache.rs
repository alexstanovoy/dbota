//! One bounded in-memory cache for service lists. Only opening a request checks freshness.
use super::{core::{Asked,MenuCore},leaderboard::{Board,Query},player_profile::Snapshot};
use bevy::platform::time::Instant;
use bota_radovid::proto::{ClientMsg,Reply,Error,MAX_RECENT_MATCHES,MAX_LOBBY_LIST,MAX_RATED_MODES};
use std::{sync::Arc,time::Duration};

const FRESHNESS:Duration=Duration::from_secs(60);
const CAPACITY:usize=32;
const MAX_PENDING:usize=64;
#[derive(Clone,PartialEq)]
pub(super) enum Request { Rpc(ClientMsg), Board(Query), Profile(u64) }
#[derive(Clone,PartialEq)]
pub(super) struct Key { service:String, viewer:u64, request:Request }
#[derive(Clone)]
pub(super) enum Read { Reply(Arc<Reply>), Synced }
#[derive(Clone)]
pub(super) enum Value { Read(Read), Board(Arc<Board>), Profile(Arc<Snapshot>) }
struct Entry { key:Key, received:Instant, value:Value }
#[derive(Default)]
pub(super) struct Cache {
    entries:Vec<Entry>,
    pending:Vec<(u32,Key)>,
    #[cfg(test)]
    pub(super) now:Option<Instant>,
}
impl Cache {
    fn now(&self)->Instant {
        #[cfg(test)]
        if let Some(now)=self.now {return now;}
        Instant::now()
    }
    pub(super) fn get(&self,key:&Key)->Option<Value> {
        self.entries.iter().find(|entry|&entry.key==key)
            .filter(|entry|self.now().saturating_duration_since(entry.received)<FRESHNESS)
            .map(|entry|entry.value.clone())
    }
    pub(super) fn put(&mut self,key:Key,value:Value) {
        self.entries.retain(|entry|entry.key!=key);
        if self.entries.len()==CAPACITY {self.entries.remove(0);}
        self.entries.push(Entry{key,received:self.now(),value});
    }
    pub(super) fn sent(&mut self,sequence:u32,key:Key) {
        if self.pending.len()==MAX_PENDING {self.pending.remove(0);}
        self.pending.push((sequence,key));
    }
    fn take_pending(&mut self,sequence:u32)->Option<Key> {
        let at=self.pending.iter().position(|(known,_)|*known==sequence)?;
        Some(self.pending.swap_remove(at).1)
    }
    pub(super) fn disconnected(&mut self) {self.pending.clear();}
    pub(super) fn clear(&mut self) {self.pending.clear();self.entries.clear();}
}

fn request(message:&ClientMsg,asked:Asked)->Option<Request> {
    if matches!(asked,Asked::Visitor(..)) {return None;}
    let message=match message {
        ClientMsg::Account{..}|ClientMsg::RecentMatches{..}|ClientMsg::Rulesets|ClientMsg::MyMatches=>message.clone(),
        ClientMsg::LobbyList=>ClientMsg::LobbySearch{query:String::new()},
        ClientMsg::LobbySearch{query}=>ClientMsg::LobbySearch{query:query.trim().to_lowercase()},
        _=>return None,
    };
    Some(Request::Rpc(message))
}
fn cached_reply(request:&Request,reply:&Reply)->Option<Read> {
    let Request::Rpc(request)=request else {return None};
    let reply=match (request,reply) {
        (ClientMsg::Account{account:expected},Reply::Account{account}) if *expected==account.id=>reply.clone(),
        (ClientMsg::RecentMatches{account,limit},Reply::Matches{list})
            if list.len()<=usize::from((*limit).min(MAX_RECENT_MATCHES)) && list.iter().all(|result|result.seats.iter().any(|seat|seat.account==*account))=>reply.clone(),
        (ClientMsg::LobbySearch{..},Reply::Lobbies{list})=>Reply::Lobbies{list:list.iter().take(MAX_LOBBY_LIST).cloned().collect()},
        (ClientMsg::Rulesets,Reply::Rulesets{list})=>Reply::Rulesets{list:list.iter().take(MAX_RATED_MODES+1).cloned().collect()},
        // Handoffs include credentials. Keep only the fact that the live state was synchronized;
        // its membership continues to follow events in MenuCore, not a stale cached handoff.
        (ClientMsg::MyMatches,Reply::Handoffs{..})=>return Some(Read::Synced),
        _=>return None,
    };
    Some(Read::Reply(Arc::new(reply)))
}
impl MenuCore {
    pub(super) fn list_key(&self,request:Request)->Option<Key> {
        if !self.logged_in() {return None;}
        Some(Key {service:bota_radovid::matchmaker_identity(&self.prefs.service)?,viewer:self.my_id()?.0,request})
    }
    pub(super) fn list_request_key(&self,message:&ClientMsg,asked:Asked)->Option<Key> {
        self.list_key(request(message,asked)?)
    }
    pub(super) fn cached_read(&self,message:&ClientMsg,asked:Asked)->Option<Read> {
        let key=self.list_request_key(message,asked)?;
        match self.list_cache.get(&key)? {Value::Read(read)=>Some(read),_=>None}
    }
    pub(super) fn remember_list_reply(&mut self,sequence:u32,result:&Result<Reply,Error>) {
        let Some(key)=self.list_cache.take_pending(sequence) else {return};
        if self.list_key(key.request.clone()).as_ref()!=Some(&key) {return;}
        if let Ok(reply)=result && let Some(reply)=cached_reply(&key.request,reply) {
            self.list_cache.put(key,Value::Read(reply));
        }
    }
    pub(super) fn restore_board(&mut self,query:&Query)->bool {
        let Some(key)=self.list_key(Request::Board(query.clone())) else {return false};
        let Some(Value::Board(board))=self.list_cache.get(&key) else {return false};
        self.board=(*board).clone();
        self.board.origin=self.prefs.service.clone();
        true
    }
    pub(super) fn cache_board(&mut self) {
        if let Some(key)=self.list_key(Request::Board(self.board.query.clone())) {
            self.list_cache.put(key,Value::Board(Arc::new(self.board.clone())));
        }
    }
    pub(super) fn cached_profile(&self,account:u64)->Option<Arc<Snapshot>> {
        let key=self.list_key(Request::Profile(account))?;
        match self.list_cache.get(&key)? {Value::Profile(snapshot)=>Some(snapshot),_=>None}
    }
    pub(super) fn cache_completed_profile(&mut self) {
        if let Some((account,snapshot))=self.profiles.take_snapshot()
            && let Some(key)=self.list_key(Request::Profile(account)) {
            self.list_cache.put(key,Value::Profile(Arc::new(snapshot)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(service:&str,viewer:u64)->Key {
        Key{service:service.into(),viewer,request:Request::Rpc(ClientMsg::Rulesets)}
    }
    #[test]
    fn cached_reads_are_isolated_by_service_and_signed_in_account() {
        let mut cache=Cache::default();
        cache.put(key("wss://one.example/control",1),Value::Read(Read::Synced));
        assert!(cache.get(&key("wss://one.example/control",1)).is_some());
        assert!(cache.get(&key("wss://two.example/control",1)).is_none());
        assert!(cache.get(&key("wss://one.example/control",2)).is_none());
        cache.clear();
        assert!(cache.get(&key("wss://one.example/control",1)).is_none());
    }
    #[test]
    fn reading_a_cached_result_does_not_extend_its_one_minute_age() {
        let now=Instant::now();
        let mut cache=Cache{now:Some(now),..Default::default()};
        let key=key("wss://one.example/control",1);
        cache.put(key.clone(),Value::Read(Read::Synced));
        cache.now=Some(now+Duration::from_secs(59));
        assert!(cache.get(&key).is_some());
        cache.now=Some(now+Duration::from_secs(60));
        assert!(cache.get(&key).is_none());
    }
    #[test]
    fn cache_capacity_is_bounded_and_credentials_are_not_cache_requests() {
        let mut cache=Cache::default();
        for viewer in 1..=33 {cache.put(key("wss://one.example/control",viewer),Value::Read(Read::Synced));}
        assert_eq!(cache.entries.len(),32);
        assert!(cache.get(&key("wss://one.example/control",1)).is_none());
        assert!(cache.get(&key("wss://one.example/control",33)).is_some());
        let login=ClientMsg::Login{login:"synthetic_user".into(),password:bota_radovid::proto::Secret::new("synthetic_password"),client:Default::default()};
        assert!(request(&login,Asked::Other).is_none());
    }
}
