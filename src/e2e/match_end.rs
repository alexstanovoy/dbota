//! How a match ends for the player, drawn by the real client against a real game server: a match the
//! service ends is a match that ended, with its reason on the screen; a connection that breaks is a
//! lost connection. The two never look alike.
use super::harness::{Client, Server};
use bota_proto::{Cheat, HeroId};

fn playing() -> (Server, Client) {
    let server = Server::start(1, &["--immortal-heroes", "--immortal-structures"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.frames(5);
    (server, client)
}

fn drawn(client: &mut Client) -> String {
    client.shown_texts().join("\n")
}

#[test]
fn a_match_the_service_ends_is_drawn_as_a_match_that_ended_with_its_reason_and_no_lost_connection()
{
    let (server, mut client) = playing();
    assert!(!drawn(&mut client).contains("MATCH ENDED"));
    server.ask_to_finish();
    client.until("the ending reached the client", |c| {
        c.core().aborted.is_some()
    });
    client.frames(5);
    let screen = drawn(&mut client);
    assert!(screen.contains("MATCH ENDED"), "{screen}");
    assert!(screen.contains("ended by the service"), "{screen}");
    assert!(!screen.contains("CONNECTION LOST"), "{screen}");
    assert!(!screen.contains("RECONNECTING"), "{screen}");
    assert!(!client.core().lost);
}

#[test]
fn a_connection_that_breaks_is_drawn_as_lost_never_as_an_ended_match() {
    let (mut server, mut client) = playing();
    server.crash();
    client.until("the connection counted as lost", |c| c.core().lost);
    let notice = crate::hud::notice(client.core());
    assert_eq!(notice, "CONNECTION LOST");
    client.frames(5);
    let screen = drawn(&mut client);
    assert!(!screen.contains("MATCH ENDED"), "{screen}");
    assert_eq!(client.core().aborted, None);
}
