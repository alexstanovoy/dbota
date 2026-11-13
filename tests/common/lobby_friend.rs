//! A third real lobby/game client without a third renderer.
use bota_client_core::{App, Source, Transport};
use bota_proto::{Role, Team};
use bota_radovid::{
    Client, Incoming,
    proto::{AccountKind, ClientMsg, Event, Handoff, Reply, RulesetId, Secret},
};
use std::time::{Duration, Instant};

fn answer(client: &mut Client, sequence: u32) -> Reply {
    let until = Instant::now() + super::PATIENCE;
    while Instant::now() < until {
        if let Some(Incoming::Reply { seq, result }) = client
            .wait(Duration::from_millis(50))
            .expect("the control link")
            && seq == sequence
        {
            return result.expect("the request succeeds");
        }
    }
    panic!("the third client's request was not answered");
}

fn enter_lobby(client: &mut Client) -> bota_radovid::proto::LobbyId {
    let sequence = client
        .register("carol", "correct horse battery", AccountKind::Human, None)
        .expect("register");
    assert!(matches!(answer(client, sequence), Reply::LoggedIn { .. }));
    let sequence = client
        .request(ClientMsg::LobbySearch {
            query: "ali".into(),
        })
        .expect("search by host");
    let Reply::Lobbies { list } = answer(client, sequence) else {
        panic!("lobby search")
    };
    assert_eq!(list.len(), 1, "the owner search leaves out the newer decoy");
    assert_eq!(list[0].name, "Friday Night");
    let lobby = list[0].id;
    let sequence = client
        .request(ClientMsg::LobbyJoin {
            lobby,
            password: Some(Secret("open sesame".into())),
        })
        .expect("join");
    let Reply::Lobby { state } = answer(client, sequence) else {
        panic!("a direct join without approval")
    };
    assert_eq!(state.ruleset.0, "full");
    assert_eq!(state.teams, vec![5, 5]);
    lobby
}

fn await_game(client: &mut Client, lobby: bota_radovid::proto::LobbyId) -> Handoff {
    let until = Instant::now() + super::PATIENCE;
    let mut ready = false;
    while Instant::now() < until {
        match client
            .wait(Duration::from_millis(50))
            .expect("control link")
        {
            Some(Incoming::Event(Event::LobbyState { state }))
                if state.id == lobby && state.ruleset.0 == "solo_mid" && !ready =>
            {
                assert_eq!(
                    state.teams,
                    vec![5, 5],
                    "solo mid changes rules, never the lobby capacity"
                );
                client
                    .request(ClientMsg::LobbyReady { lobby, ready: true })
                    .expect("ready");
                ready = true;
            }
            Some(Incoming::Event(Event::MatchFound { handoff })) => {
                assert!(ready);
                assert_eq!(handoff.ruleset, RulesetId("solo_mid".into()));
                assert_eq!(handoff.seats.len(), 3);
                return handoff;
            }
            _ => {}
        }
    }
    panic!("the third client's match did not arrive");
}

pub fn join(service: &super::Service) -> (Client, App) {
    let pem = std::fs::read_to_string(&service.pem).expect("certificate");
    let tls = bota_radovid::Tls::trust_pem(&pem).expect("trust");
    let mut control = Client::open(&service.url, &tls).expect("control connection");
    let lobby = enter_lobby(&mut control);
    let handoff = await_game(&mut control, lobby);
    let game = open_game(&handoff, &tls);
    (control, game)
}

fn open_game(handoff: &Handoff, tls: &bota_radovid::Tls) -> App {
    let mut link = bota_radovid::connect_net(handoff, tls).expect("direct game connection");
    Transport::send(
        &mut link,
        &bota_radovid::seat_hello(handoff, Role::Player).expect("seat greeting"),
    )
    .expect("hello");
    Transport::send(&mut link, &bota_client_core::catalogue()).expect("catalogue");
    let mut game = App::new(Source::Live(Box::new(link)));
    let until = Instant::now() + super::PATIENCE;
    while game.my_slot.is_none() || game.lobby.slots.len() != 3 {
        assert!(
            Instant::now() < until,
            "the third client's game seat did not open"
        );
        game.pump(1.0 / 30.0);
        std::thread::sleep(Duration::from_millis(1));
    }
    let radiant = game
        .lobby
        .slots
        .iter()
        .filter(|slot| slot.team == Team::Radiant)
        .count();
    let dire = game
        .lobby
        .slots
        .iter()
        .filter(|slot| slot.team == Team::Dire)
        .count();
    assert_eq!((radiant, dire), (2, 1));
    game
}

/// Two existing members for Carol's independently rendered search-by-host path.
pub struct Friends {
    host: Client,
    guest: Client,
    lobby: bota_radovid::proto::LobbyId,
    tls: bota_radovid::Tls,
}

impl Friends {
    pub fn new(service: &super::Service) -> Self {
        let pem = std::fs::read_to_string(&service.pem).expect("certificate");
        let tls = bota_radovid::Tls::trust_pem(&pem).expect("trust");
        let mut host = registered_friend(service, &tls, "alice");
        let seq = host
            .request(ClientMsg::LobbyCreate {
                ruleset: RulesetId("full".into()),
                name: "Friday Night".into(),
                audience: bota_radovid::proto::Audience::Everyone,
                password: Some(Secret("open sesame".into())),
            })
            .expect("create");
        let Reply::Lobby { state } = answer(&mut host, seq) else {
            panic!("created lobby")
        };
        assert_eq!(state.teams, vec![5, 5]);
        let lobby = state.id;
        let mut guest = registered_friend(service, &tls, "bob");
        let seq = guest
            .request(ClientMsg::LobbyJoin {
                lobby,
                password: Some(Secret("open sesame".into())),
            })
            .expect("join guest");
        let Reply::Lobby { state } = answer(&mut guest, seq) else {
            panic!("guest joined")
        };
        assert_eq!(state.members.len(), 2);
        Self {
            host,
            guest,
            lobby,
            tls,
        }
    }

    pub fn start(&mut self) -> (App, App) {
        wait_state(&mut self.host, self.lobby, |state| state.members.len() == 3);
        let seq = self
            .host
            .request(ClientMsg::LobbyConfigure {
                lobby: self.lobby,
                ruleset: RulesetId("solo_mid".into()),
                audience: bota_radovid::proto::Audience::Everyone,
                password: Some(Secret("open sesame".into())),
            })
            .expect("mode change");
        assert!(matches!(answer(&mut self.host, seq), Reply::Done));
        let seq = self
            .guest
            .request(ClientMsg::LobbyReady {
                lobby: self.lobby,
                ready: true,
            })
            .expect("guest ready");
        assert!(matches!(answer(&mut self.guest, seq), Reply::Done));
        wait_state(&mut self.host, self.lobby, |state| {
            state.ruleset.0 == "solo_mid"
                && state
                    .members
                    .iter()
                    .filter(|m| m.account != state.owner)
                    .all(|m| m.ready)
        });
        let seq = self
            .host
            .request(ClientMsg::LobbyReady {
                lobby: self.lobby,
                ready: true,
            })
            .expect("start");
        assert!(matches!(answer(&mut self.host, seq), Reply::Done));
        let host = match_handoff(&mut self.host);
        let guest = match_handoff(&mut self.guest);
        (open_game(&host, &self.tls), open_game(&guest, &self.tls))
    }
}

fn wait_state(
    client: &mut Client,
    lobby: bota_radovid::proto::LobbyId,
    ready: impl Fn(&bota_radovid::proto::LobbyState) -> bool,
) {
    let until = Instant::now() + super::PATIENCE;
    while Instant::now() < until {
        if let Some(Incoming::Event(Event::LobbyState { state })) = client
            .wait(Duration::from_millis(50))
            .expect("control link")
            && state.id == lobby
            && ready(&state)
        {
            assert_eq!(state.teams, vec![5, 5]);
            return;
        }
    }
    panic!("the friends did not reach the expected lobby state");
}

fn match_handoff(client: &mut Client) -> Handoff {
    let until = Instant::now() + super::PATIENCE;
    while Instant::now() < until {
        if let Some(Incoming::Event(Event::MatchFound { handoff })) = client
            .wait(Duration::from_millis(50))
            .expect("control link")
        {
            assert_eq!(handoff.ruleset.0, "solo_mid");
            assert_eq!(handoff.seats.len(), 3);
            return handoff;
        }
    }
    panic!("the friends did not receive their game");
}

fn registered_friend(service: &super::Service, tls: &bota_radovid::Tls, login: &str) -> Client {
    let mut client = Client::open(&service.url, tls).expect("friend control");
    let sequence = client
        .register(login, "correct horse battery", AccountKind::Human, None)
        .expect("register friend");
    assert!(matches!(
        answer(&mut client, sequence),
        Reply::LoggedIn { .. }
    ));
    client
}
