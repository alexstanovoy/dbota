//! The pre-game's cues on the game clock, as Dota plays them today: "Prepare for battle" at -1:30,
//! "Thirty seconds to battle" at -0:30, the ancients' horn at -0:12 and "The battle begins!" at
//! -0:03, each once, and none that would start before the match's pre-game does.

use std::time::Duration;

use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::voice::{PregameCue, PregameHeard};

/// Owner-confirmed current Dota timings: seconds before 0:00 each cue starts.
const PREPARE_BEFORE: u32 = 90;
const THIRTY_BEFORE: u32 = 30;
const HORN_BEFORE: u32 = 12;
const BATTLE_BEGINS_BEFORE: u32 = 3;

/// A match watched through its pre-game and a little past 0:00: each cue starts once, on the
/// very tick its time before 0:00 falls on, and a cue whose time lies before the pre-game's
/// start (Prepare for battle at -1:30 in a 30 s pre-game) does not play.
#[test]
fn the_pregame_cues_play_on_dotas_clock_once_each() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let rate = u32::from(client.core().tick_rate());
    let pregame = client.core().pregame_ticks();
    assert!(
        pregame > HORN_BEFORE * rate,
        "a pre-game long enough for the horn"
    );
    let past_zero = pregame + rate;
    client.within(Duration::from_secs(120), "the clock past 0:00", |c| {
        c.view().game_tick() >= past_zero
    });
    let played = &client.world().resource::<PregameHeard>().0;
    let at = |cue: PregameCue| {
        let ticks: Vec<u32> = played
            .iter()
            .filter(|(c, _)| *c == cue)
            .map(|(_, t)| *t)
            .collect();
        assert!(ticks.len() <= 1, "{cue:?} played once: {ticks:?}");
        ticks.first().copied()
    };
    // On the cue's tick, or the first tick the client saw of the match when the cue falls on
    // the pre-game's very first (a client takes the match in from its first view).
    let first = played.iter().map(|(_, t)| *t).min().unwrap_or(0);
    let expected = |before: u32| pregame.checked_sub(before * rate).map(|t| t.max(first));
    assert_eq!(at(PregameCue::Prepare), expected(PREPARE_BEFORE));
    assert_eq!(at(PregameCue::ThirtySeconds), expected(THIRTY_BEFORE));
    assert_eq!(at(PregameCue::Horn), expected(HORN_BEFORE));
    assert_eq!(at(PregameCue::BattleBegins), expected(BATTLE_BEGINS_BEFORE));
    assert!(
        first < rate,
        "the client saw the pre-game from its start: tick {first}"
    );
}
