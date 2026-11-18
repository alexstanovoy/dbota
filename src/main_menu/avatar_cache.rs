//! The pictures of other players that the rows of a list draw. The service names each player's
//! avatar by a version in the row; the cache asks for the versions it does not hold, a page at a
//! time and only for the rows wanted, decodes what arrives off the frame loop, and keeps the
//! small picture a row needs.
//!
//! Memory is bounded: at most [`CAPACITY`] pictures of [`LIST_SIDE`] squared RGBA pixels, which is
//! 2 MiB however many players the lists show, and at most one ask's worth of pictures decoding.
use super::avatar_image::{self, LIST_SIDE};
use bevy::{
    platform::time::Instant,
    tasks::{AsyncComputeTaskPool, Task},
};
use bota_radovid::proto::{AccountId, AvatarChange, AvatarQuery, AvatarVersion, Error};
use dbota_import::thumbnail::Pixels;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

/// Pictures kept, oldest out: the 100 rows of a leaderboard page and some.
pub const CAPACITY: usize = 128;
/// Players one ask is about, in the order the rows are drawn: the first rows arrive first.
pub const ASK: usize = 32;
/// Decisions kept so that an avatar that is gone, broken or newer than its row says is not asked
/// for again; forgotten wholesale when full.
const MAX_SETTLED: usize = 2048;
/// Refusals in a row after which the wanted players are given up on.
const MAX_REFUSALS: u8 = 3;
const SHORTEST_WAIT: Duration = Duration::from_millis(250);
const LONGEST_WAIT: Duration = Duration::from_secs(5);
/// How long an ask may go unanswered before it is taken as refused.
const ANSWER_DEADLINE: Duration = Duration::from_secs(20);

const _: () = assert!(CAPACITY * (LIST_SIDE * LIST_SIDE * 4) as usize <= 2 << 20);
const _: () = assert!(ASK <= bota_radovid::proto::MAX_AVATAR_BATCH);

/// A picture being decoded for a player, and the version the service sent it as.
struct Decoding {
    account: u64,
    version: AvatarVersion,
    task: Task<Result<Pixels, String>>,
}

struct Entry {
    version: AvatarVersion,
    picture: Arc<Pixels>,
    age: u64,
}

/// A player of a row, and the version of the avatar the row says they have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Wish {
    pub account: u64,
    pub version: Option<AvatarVersion>,
}

#[derive(Default)]
pub struct Cache {
    pictures: HashMap<u64, Entry>,
    clock: u64,
    decoding: Vec<Decoding>,
    /// (account, version a row named) pairs the service has answered, whatever it answered.
    settled: HashSet<(u64, AvatarVersion)>,
    /// The ask in flight, with the wish behind each of its queries.
    asked: Option<(Instant, Vec<(AvatarQuery, AvatarVersion)>)>,
    wait_until: Option<Instant>,
    refusals: u8,
}

impl Cache {
    /// The picture for a row naming `version`; the one held for an older version stands until the
    /// new one arrives, and a row naming none has none.
    pub fn picture(&self, account: u64, version: Option<AvatarVersion>) -> Option<Arc<Pixels>> {
        version?;
        self.pictures
            .get(&account)
            .map(|entry| Arc::clone(&entry.picture))
    }

    /// The picture of `account` is no longer current: the account changed its avatar here.
    pub fn forget(&mut self, account: u64) {
        self.pictures.remove(&account);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.pictures.len()
    }

    /// Everything forgotten: the connection, and with it whose avatars these are, changed.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The link ended: the ask in flight will never be answered. Pictures held are still good.
    pub fn link_lost(&mut self) {
        self.asked = None;
        self.refusals = 0;
        self.wait_until = None;
    }

    /// Takes in the pictures that finished decoding.
    pub fn poll(&mut self) {
        let mut waiting = Vec::new();
        for mut job in std::mem::take(&mut self.decoding) {
            match crate::pacing::poll(&mut job.task) {
                Some(Ok(picture)) => self.keep(job.account, job.version, picture),
                Some(Err(_)) => {}
                None => waiting.push(job),
            }
        }
        self.decoding = waiting;
    }

    fn keep(&mut self, account: u64, version: AvatarVersion, picture: Pixels) {
        self.clock += 1;
        self.pictures.insert(
            account,
            Entry {
                version,
                picture: Arc::new(picture),
                age: self.clock,
            },
        );
        while self.pictures.len() > CAPACITY {
            let oldest = self
                .pictures
                .iter()
                .min_by_key(|(_, entry)| entry.age)
                .map(|(account, _)| *account);
            match oldest {
                Some(account) => self.pictures.remove(&account),
                None => break,
            };
        }
    }

    /// What to ask the service now for the rows wanted, in their order: the players whose
    /// avatars are neither held, being decoded, nor already answered. Nothing while an ask is
    /// out or a refusal's wait lasts.
    pub fn next_ask(
        &mut self,
        wishes: impl IntoIterator<Item = Wish>,
        now: Instant,
    ) -> Option<Vec<AvatarQuery>> {
        if self
            .asked
            .as_ref()
            .is_some_and(|(sent, _)| now >= *sent + ANSWER_DEADLINE)
        {
            self.refused(&Error::Internal, now);
        }
        if self.asked.is_some() || self.wait_until.is_some_and(|until| now < until) {
            return None;
        }
        let mut seen = HashSet::new();
        let mut asked = Vec::new();
        for wish in wishes {
            let Some(version) = wish.version else {
                continue;
            };
            let held = self.pictures.get(&wish.account).map(|entry| entry.version);
            let wanted = (wish.account > 0 && wish.account <= i64::MAX as u64)
                && held != Some(version)
                && !self.settled.contains(&(wish.account, version))
                && !self.decoding.iter().any(|job| job.account == wish.account)
                && seen.insert(wish.account);
            if wanted {
                let query = AvatarQuery {
                    account: AccountId(wish.account),
                    version: held,
                };
                asked.push((query, version));
            }
            if asked.len() == ASK {
                break;
            }
        }
        if asked.is_empty() {
            return None;
        }
        let queries = asked.iter().map(|(query, _)| *query).collect();
        self.asked = Some((now, asked));
        Some(queries)
    }

    /// The service answered the first `answered` queries of the ask in flight: each answered
    /// query is settled, and a changed avatar is decoded.
    pub fn replied(&mut self, changes: Vec<AvatarChange>, answered: u8) {
        let Some((_, asked)) = self.asked.take() else {
            return;
        };
        self.refusals = 0;
        self.wait_until = None;
        let answered = usize::from(answered).min(asked.len());
        for (query, named) in &asked[..answered] {
            self.settle(query.account.0, *named);
            let change = changes
                .iter()
                .find(|change| change.account == query.account);
            match change.map(|change| change.avatar.as_ref()) {
                Some(Some(avatar)) => {
                    let png = avatar.png.clone();
                    self.decoding.push(Decoding {
                        account: query.account.0,
                        version: avatar.version,
                        task: AsyncComputeTaskPool::get()
                            .spawn(async move { avatar_image::decode_for_list(&png) }),
                    });
                }
                Some(None) => {
                    self.pictures.remove(&query.account.0);
                }
                None => {}
            }
        }
    }

    /// The service refused the ask: wait as it says, and give up on the players after a few
    /// refusals in a row.
    pub fn refused(&mut self, error: &Error, now: Instant) {
        let Some((_, asked)) = self.asked.take() else {
            return;
        };
        self.refusals += 1;
        if self.refusals >= MAX_REFUSALS {
            self.refusals = 0;
            for (query, named) in asked {
                self.settle(query.account.0, named);
            }
        }
        let wait = match error {
            Error::Throttled { retry_after_ms } | Error::RateLimited { retry_after_ms } => {
                Duration::from_millis(u64::from(*retry_after_ms))
            }
            _ => LONGEST_WAIT,
        };
        self.wait_until = Some(now + wait.clamp(SHORTEST_WAIT, LONGEST_WAIT));
    }

    fn settle(&mut self, account: u64, version: AvatarVersion) {
        if self.settled.len() >= MAX_SETTLED {
            self.settled.clear();
        }
        self.settled.insert((account, version));
    }

    /// Waits for every decoding to finish and takes the pictures in, for tests, which cannot
    /// watch a clock.
    #[cfg(test)]
    pub fn finish_decoding(&mut self) {
        use bevy::tasks::block_on;
        for job in std::mem::take(&mut self.decoding) {
            if let Ok(picture) = block_on(job.task) {
                self.keep(job.account, job.version, picture);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::tasks::TaskPool;
    use bota_radovid::proto::Avatar;

    fn pool() {
        AsyncComputeTaskPool::get_or_init(TaskPool::default);
    }

    fn version(byte: u8) -> AvatarVersion {
        AvatarVersion([byte; 32])
    }

    fn avatar(byte: u8, color: [u8; 4]) -> Avatar {
        let rgba: Vec<u8> = (0..avatar_image::SIDE * avatar_image::SIDE)
            .flat_map(|_| color)
            .collect();
        Avatar {
            version: version(byte),
            png: radovid_proto::encode_avatar_rgba(&rgba).expect("a flat picture fits"),
        }
    }

    fn wish(account: u64, byte: Option<u8>) -> Wish {
        Wish {
            account,
            version: byte.map(version),
        }
    }

    fn change(account: u64, avatar: Option<Avatar>) -> AvatarChange {
        AvatarChange {
            account: AccountId(account),
            avatar,
        }
    }

    #[test]
    fn rows_without_an_avatar_and_held_versions_are_not_asked_for() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        let first = cache.next_ask([wish(1, Some(1)), wish(2, None), wish(3, Some(3))], now);
        let queries = first.expect("two rows name avatars");
        assert_eq!(
            queries
                .iter()
                .map(|query| query.account.0)
                .collect::<Vec<_>>(),
            [1, 3]
        );
        assert!(queries.iter().all(|query| query.version.is_none()));
        cache.replied(
            vec![
                change(1, Some(avatar(1, [1, 2, 3, 255]))),
                change(3, Some(avatar(3, [9, 9, 9, 255]))),
            ],
            2,
        );
        cache.finish_decoding();
        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.next_ask([wish(1, Some(1)), wish(3, Some(3))], now),
            None,
            "held versions cost nothing"
        );
        let picture = cache.picture(1, Some(version(1))).expect("held");
        assert_eq!(picture.width(), LIST_SIDE);
        assert_eq!(&picture.rgba()[..4], &[1, 2, 3, 255]);
        assert!(
            cache.picture(1, None).is_none(),
            "a row naming none shows none"
        );
    }

    #[test]
    fn a_page_is_asked_in_row_order_in_parts_of_the_ask_size_without_repeating_a_player() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        let rows: Vec<Wish> = (1..=100u64).map(|a| wish(a, Some(a as u8))).collect();
        let first = cache
            .next_ask(rows.iter().copied().chain([wish(1, Some(1))]), now)
            .expect("ask");
        assert_eq!(first.len(), ASK);
        assert_eq!(first[0].account.0, 1, "the first row is asked first");
        assert_eq!(
            cache.next_ask(rows.iter().copied(), now),
            None,
            "one ask at a time"
        );
        // The service answered the first half only (its reply is byte bounded).
        cache.replied(
            (1..=16u64)
                .map(|a| change(a, Some(avatar(a as u8, [a as u8, 0, 0, 255]))))
                .collect(),
            16,
        );
        cache.finish_decoding();
        let second = cache.next_ask(rows.iter().copied(), now).expect("the rest");
        assert_eq!(second.first().map(|query| query.account.0), Some(17));
        assert_eq!(second.len(), ASK);
    }

    #[test]
    fn a_newer_version_than_the_row_says_is_not_asked_for_again() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        cache.next_ask([wish(5, Some(1))], now).expect("ask");
        // The service has a newer avatar than the board's row named.
        cache.replied(vec![change(5, Some(avatar(2, [4, 4, 4, 255])))], 1);
        cache.finish_decoding();
        assert_eq!(cache.next_ask([wish(5, Some(1))], now), None);
        assert!(
            cache.picture(5, Some(version(1))).is_some(),
            "the newer picture is shown"
        );
    }

    #[test]
    fn a_changed_row_asks_with_the_version_held_and_an_unchanged_answer_settles_it() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        cache.next_ask([wish(5, Some(1))], now).expect("ask");
        cache.replied(vec![change(5, Some(avatar(1, [4, 4, 4, 255])))], 1);
        cache.finish_decoding();
        let ask = cache
            .next_ask([wish(5, Some(2))], now)
            .expect("the row names a newer one");
        assert_eq!(ask[0].version, Some(version(1)), "the held version is sent");
        cache.replied(Vec::new(), 1);
        assert_eq!(
            cache.next_ask([wish(5, Some(2))], now),
            None,
            "unchanged: settled"
        );
    }

    #[test]
    fn a_cleared_avatar_removes_the_picture() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        cache.next_ask([wish(5, Some(1))], now).expect("ask");
        cache.replied(vec![change(5, Some(avatar(1, [4, 4, 4, 255])))], 1);
        cache.finish_decoding();
        cache.next_ask([wish(5, Some(2))], now).expect("ask");
        cache.replied(vec![change(5, None)], 1);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn a_hostile_avatar_is_dropped_without_a_picture_and_asked_for_once() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        cache.next_ask([wish(5, Some(1))], now).expect("ask");
        cache.replied(
            vec![change(
                5,
                Some(Avatar {
                    version: version(1),
                    png: b"<svg onload=alert(1)>".to_vec(),
                }),
            )],
            1,
        );
        cache.finish_decoding();
        assert_eq!(cache.len(), 0);
        assert_eq!(cache.next_ask([wish(5, Some(1))], now), None);
    }

    #[test]
    fn an_answer_for_a_player_not_asked_about_or_beyond_the_asked_count_is_ignored() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        cache
            .next_ask([wish(5, Some(1)), wish(6, Some(1))], now)
            .expect("ask");
        cache.replied(
            vec![
                change(99, Some(avatar(1, [1, 1, 1, 255]))),
                change(6, Some(avatar(1, [2, 2, 2, 255]))),
            ],
            1,
        );
        cache.finish_decoding();
        assert_eq!(
            cache.len(),
            0,
            "player 99 was not asked about; player 6 is past the answered count"
        );
    }

    #[test]
    fn refusals_wait_as_the_service_says_and_give_up_after_three_in_a_row() {
        pool();
        let mut cache = Cache::default();
        let wishes = [wish(5, Some(1))];
        let refusal = Error::Throttled {
            retry_after_ms: 1000,
        };
        let first = Instant::now();
        cache.next_ask(wishes, first).expect("ask");
        cache.refused(&refusal, first);
        assert!(
            cache
                .next_ask(wishes, first + Duration::from_millis(999))
                .is_none(),
            "waits as the service says"
        );
        let second = first + Duration::from_secs(1);
        cache
            .next_ask(wishes, second)
            .expect("asks again after the wait");
        cache.refused(&refusal, second);
        let third = second + Duration::from_secs(1);
        cache.next_ask(wishes, third).expect("a third time");
        cache.refused(&refusal, third);
        assert_eq!(
            cache.next_ask(wishes, third + Duration::from_secs(60)),
            None,
            "three refusals in a row: given up on"
        );
    }

    #[test]
    fn an_ask_that_is_never_answered_is_taken_as_refused_after_its_deadline() {
        pool();
        let mut cache = Cache::default();
        let start = Instant::now();
        let wishes = [wish(5, Some(1))];
        cache.next_ask(wishes, start).expect("ask");
        assert!(
            cache
                .next_ask(wishes, start + Duration::from_secs(19))
                .is_none()
        );
        let late = start + Duration::from_secs(40);
        assert!(
            cache.next_ask(wishes, late).is_none(),
            "the refusal's wait starts at the deadline"
        );
        assert!(
            cache
                .next_ask(wishes, late + Duration::from_secs(6))
                .is_some(),
            "asked again once the wait has passed"
        );
    }

    #[test]
    fn the_cache_never_holds_more_than_its_capacity_and_drops_the_oldest() {
        pool();
        let mut cache = Cache::default();
        let now = Instant::now();
        let accounts = CAPACITY as u64 + 10;
        let mut next = 1;
        while next <= accounts {
            let wishes: Vec<Wish> = (next..=accounts).map(|a| wish(a, Some(1))).collect();
            let ask = cache.next_ask(wishes, now).expect("ask");
            let changes = ask
                .iter()
                .map(|query| change(query.account.0, Some(avatar(1, [1, 1, 1, 255]))))
                .collect();
            let count = ask.len() as u8;
            next += u64::from(count);
            cache.replied(changes, count);
            cache.finish_decoding();
        }
        assert_eq!(cache.len(), CAPACITY);
        assert!(
            cache.picture(1, Some(version(1))).is_none(),
            "the oldest went first"
        );
        assert!(cache.picture(accounts, Some(version(1))).is_some());
    }
}
