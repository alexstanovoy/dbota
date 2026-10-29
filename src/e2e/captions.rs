//! No captions Dota does not show: no CHEATS under the clock of a match
//! with cheats, no VISION line for one who watches.

use bota_proto::HeroId;

use super::harness::{Client, Server, quiet_seat};

/// What a caption the client must not show would say: Dota's word for cheats (and the plain
/// one), and the vision line's beginning.
fn captions(client: &Client) -> Vec<String> {
    let texts = client.world().resource::<crate::dota_text::Texts>();
    let cheats = texts.label("DOTA_Lobby_Prefs_Cheats", &[]);
    ["CHEATS".to_owned(), "VISION".to_owned()]
        .into_iter()
        .chain(cheats.map(|c| c.to_uppercase()))
        .collect()
}

fn captioned(client: &mut Client) -> Vec<String> {
    let banned = captions(client);
    client
        .shown_texts()
        .into_iter()
        .filter(|text| {
            banned
                .iter()
                .any(|b| text.to_uppercase().starts_with(b.as_str()))
        })
        .collect()
}

/// A match that honours cheats shows no CHEATS mark under its clock, and a spectator's screen
/// no VISION line: the replay controls' vision button alone says whose eyes a replay is seen
/// with.
#[test]
fn no_cheats_or_vision_caption_is_shown() {
    {
        let server = Server::start(1, &[]);
        let mut player = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        player.frames(30);
        assert!(
            player.core().statics().is_some_and(|s| s.metadata().cheats),
            "a cheat match"
        );
        assert_eq!(
            captioned(&mut player),
            Vec::<String>::new(),
            "a player's screen"
        );
    }
    // In real time: the one seat would play its match out at once in lockstep, watched or not.
    let server = Server::start(1, &["--spectators", "open", "--mode", "realtime"]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let mut watcher = Client::run(&["--addr", &server.address, "--spectate"]);
    watcher.until("the match seen", |c| c.core().view().is_some());
    watcher.frames(30);
    assert_eq!(
        captioned(&mut watcher),
        Vec::<String>::new(),
        "a spectator's screen"
    );
}
