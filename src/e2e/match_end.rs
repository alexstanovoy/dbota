//! How a match ends for the player, drawn by the real client against a real game server: a match the
//! service ends is a match that ended, with its reason on the post-game screen (which opens over
//! the match in the same frame, so nothing is said under it); a connection that breaks is a lost
//! connection. The two never look alike.
use super::harness::{Client, Server};
use bota_proto::{AbortReason, Cheat, HeroId, ServerMsg};

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

/// The warning ahead of a time limit and the ending it brings, told to the running client as the
/// server tells them (the real server's limit is end to end in the menu tests): the feed line, then
/// the post-game screen, which says nobody won and why the match ended. Its texts are the only
/// ones drawn then, as the screen covers the match: the test reads what `shown_texts` finds there
/// and that the match under it says nothing a player could not read.
#[test]
fn the_warning_the_time_limit_ending_and_the_post_game_header_are_drawn() {
    let (_server, mut client) = playing();
    client.core_mut().handle(ServerMsg::TimeLimitNear {
        remaining_secs: 300,
    });
    client.frames(5);
    let screen = drawn(&mut client);
    assert!(
        screen.contains("The match ends in 5 minutes (time limit)"),
        "{screen}"
    );
    client.core_mut().handle(ServerMsg::MatchAborted {
        reason: AbortReason::TimeLimit,
        stats: None,
    });
    client.until("the post-game header says nobody won", |c| {
        c.shown_texts().iter().any(|text| text == "NO RESULT")
    });
    let screen = drawn(&mut client);
    assert!(
        screen.contains("TIME LIMIT REACHED"),
        "the reason beside it: {screen}"
    );
    assert!(!screen.contains("DRAW"), "{screen}");
    assert!(!screen.contains("CONNECTION LOST"), "{screen}");
    assert_eq!(
        crate::hud::notice(client.core()),
        "",
        "nothing is said in the middle of the match under the post-game screen"
    );
}

#[test]
fn a_match_the_service_ends_is_drawn_as_a_match_that_ended_with_its_reason_and_no_lost_connection()
{
    let (server, mut client) = playing();
    assert!(!drawn(&mut client).contains("NO RESULT"));
    server.ask_to_finish();
    client.until("the ending reached the client", |c| {
        c.core().aborted.is_some()
    });
    client.until("the post-game header says nobody won", |c| {
        c.shown_texts().iter().any(|text| text == "NO RESULT")
    });
    let screen = drawn(&mut client);
    assert!(screen.contains("ENDED BY THE SERVICE"), "{screen}");
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
    client.frames(1);
    let screen = drawn(&mut client);
    assert!(!screen.contains("NO RESULT"), "{screen}");
    assert_eq!(client.core().aborted, None);
}
