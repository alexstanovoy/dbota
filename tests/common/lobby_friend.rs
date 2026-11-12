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
    let mut link = bota_radovid::connect_net(&handoff, &tls).expect("direct game connection");
    Transport::send(
        &mut link,
        &bota_radovid::seat_hello(&handoff, Role::Player).expect("seat greeting"),
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
    (control, game)
}
