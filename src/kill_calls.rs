//! What the heroes' falls make of kills, as Dota's server counts them for its announcements and
//! this client derives them from the facts it is told: the first hero kill of the match
//! (First Blood), kills in quick succession by one hero (multi-kills) and kills without falling
//! (streaks). The counting is [`Tracker`], a plain state machine; [`plugin`] feeds it the deaths
//! the core told and keeps what it answered for the frame ([`Calls`]), which the banner
//! ([`crate::kill_banner`]), the announcer ([`crate::voice`]) and the hero's voice
//! ([`crate::hero_voice`]) all read, so that there is one count.
//!
//! The rules are those established from the installed game (build 6975): a kill is a fall
//! credited to an enemy hero (`credited_hero`: the hero that struck last, or the only hero that
//! hurt the victim when a creep or tower did); a multi-kill goes on while each kill comes within
//! 18 seconds of game time of the hero's previous one; a streak ends with the fall of its hero to
//! an enemy (a deny, a suicide or neutral creeps leave it as it was) and is told only with a
//! kill. Nothing is told by the server for this: every hero's fall is told to every seat and
//! spectator, with the hero credited.

use std::collections::HashMap;
use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{App as Core, FeedKind, Victim};
use bota_proto::{EntityId, EventKind, Team};

use crate::session::Session;

/// Seconds of game time a hero has after a kill for the next to join its multi-kill
/// (established from the installed game).
pub const MULTI_KILL_SECONDS: u32 = 18;

/// The fewest kills in a row that make a streak worth a call.
pub const STREAK_FROM: u16 = 3;

/// Heroes whose runs are kept at most: a match has ten, cheats may add more.
const MAX_HEROES: usize = 64;

/// A hero's fall, as the counting needs it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fall {
    pub tick: u32,
    pub victim: EntityId,
    /// The hero the kill is credited to, if the fall is an enemy hero's kill.
    pub credited: Option<EntityId>,
    /// Whether the fall ends the victim's streak: it did not fall to its own side or to neutrals.
    pub ends_streak: bool,
    /// Whether the kill was shared: a creep or tower finished a hero that several enemy heroes
    /// had hurt, so that no one is credited, yet the game's First Blood is spent.
    pub shared: bool,
}

/// What a fall makes of the announcements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    pub tick: u32,
    pub victim: EntityId,
    /// The hero credited with the kill.
    pub killer: EntityId,
    /// The first kill of the match.
    pub first_blood: bool,
    /// Kills in a row within the window, this one included, from two on.
    pub multi: Option<u16>,
    /// Kills without falling, this one included, from [`STREAK_FROM`] on.
    pub streak: Option<u16>,
    /// The streak the victim had that this fall ended, from [`STREAK_FROM`] on.
    pub ended: Option<u16>,
}

/// A hero's run of kills.
#[derive(Clone, Copy, Debug, Default)]
struct Run {
    streak: u16,
    multi: u16,
    last_kill: Option<u32>,
}

/// The counting of a match.
#[derive(Clone, Debug)]
pub struct Tracker {
    window_ticks: u32,
    first_blood_claimed: bool,
    runs: HashMap<EntityId, Run>,
}

impl Tracker {
    /// A counting that gives kills `window_ticks` after the previous one to join a multi-kill;
    /// `first_blood_claimed` when the match is known to have had a kill already.
    #[must_use]
    pub fn new(window_ticks: u32, first_blood_claimed: bool) -> Tracker {
        assert!(window_ticks > 0, "a window of no time joins no kills");
        Tracker {
            window_ticks,
            first_blood_claimed,
            runs: HashMap::new(),
        }
    }

    /// Takes in one fall; what it announces, if anything: only a kill is announced, and the end
    /// of a streak with it (a creep's kill ends a streak without a word, as in Dota).
    pub fn fall(&mut self, fall: Fall) -> Option<Call> {
        let ended = self
            .runs
            .get_mut(&fall.victim)
            .filter(|_| fall.ends_streak)
            .and_then(|run| {
                let had = std::mem::take(&mut run.streak);
                (had >= STREAK_FROM).then_some(had)
            });
        if fall.shared && fall.credited.is_none() {
            self.first_blood_claimed = true;
        }
        let killer = fall.credited.filter(|killer| *killer != fall.victim)?;
        let (streak, multi) = self.kill(killer, fall.tick)?;
        let first_blood = !std::mem::replace(&mut self.first_blood_claimed, true);
        Some(Call {
            tick: fall.tick,
            victim: fall.victim,
            killer,
            first_blood,
            multi: (multi >= 2).then_some(multi),
            streak: (streak >= STREAK_FROM).then_some(streak),
            ended,
        })
    }

    /// A kill by `killer` on `tick`: its streak and its run of kills in a row now.
    fn kill(&mut self, killer: EntityId, tick: u32) -> Option<(u16, u16)> {
        if !self.runs.contains_key(&killer) && self.runs.len() >= MAX_HEROES {
            return None;
        }
        let run = self.runs.entry(killer).or_default();
        let open = run
            .last_kill
            .is_some_and(|last| tick < last.saturating_add(self.window_ticks));
        run.multi = if open { run.multi.saturating_add(1) } else { 1 };
        run.streak = run.streak.saturating_add(1);
        run.last_kill = Some(tick);
        Some((run.streak, run.multi))
    }
}

/// How long the announcer waits with the multi-kill line after a streak line (the delay of the
/// game's second query).
pub const MULTI_AFTER_STREAK: Duration = Duration::from_millis(1500);

/// Every sound a [`Line`] can name, for the voices to load.
pub const LINES: [&str; 13] = [
    "sound.announcer.kill.first_blood",
    "sound.announcer.kill.double",
    "sound.announcer.kill.triple",
    "sound.announcer.kill.ultra",
    "sound.announcer.kill.rampage",
    "sound.announcer.kill.streak.3",
    "sound.announcer.kill.streak.4",
    "sound.announcer.kill.streak.5",
    "sound.announcer.kill.streak.6",
    "sound.announcer.kill.streak.7",
    "sound.announcer.kill.streak.8",
    "sound.announcer.kill.streak.9",
    "sound.announcer.kill.streak.max",
];

/// A line the announcer speaks for a kill.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Line {
    FirstBlood,
    /// A multi-kill of this many kills.
    Multi(u16),
    /// A streak of this many kills.
    Streak(u16),
}

impl Line {
    /// The cache's sound of it (the default pack's: double, triple, ultra and rampage for two to
    /// five and on, a name for each streak from three to nine and one for every longer).
    pub fn logical(self) -> &'static str {
        const STREAKS: [&str; 7] = [
            "sound.announcer.kill.streak.3",
            "sound.announcer.kill.streak.4",
            "sound.announcer.kill.streak.5",
            "sound.announcer.kill.streak.6",
            "sound.announcer.kill.streak.7",
            "sound.announcer.kill.streak.8",
            "sound.announcer.kill.streak.9",
        ];
        match self {
            Line::FirstBlood => "sound.announcer.kill.first_blood",
            Line::Multi(0..=2) => "sound.announcer.kill.double",
            Line::Multi(3) => "sound.announcer.kill.triple",
            Line::Multi(4) => "sound.announcer.kill.ultra",
            Line::Multi(_) => "sound.announcer.kill.rampage",
            Line::Streak(n @ 3..=9) => STREAKS[usize::from(n) - 3],
            Line::Streak(_) => "sound.announcer.kill.streak.max",
        }
    }
}

/// The lines the announcer speaks for a call, each with the wait before it. First Blood is
/// spoken alone; otherwise a streak line, then a multi-kill line behind it (the game's rules
/// choose the one line of each of its two queries).
pub fn spoken(call: &Call) -> Vec<(Line, Duration)> {
    if call.first_blood {
        return vec![(Line::FirstBlood, Duration::ZERO)];
    }
    let mut lines = Vec::new();
    if let Some(streak) = call.streak {
        lines.push((Line::Streak(streak), Duration::ZERO));
    }
    if let Some(multi) = call.multi {
        let wait = if call.streak.is_some() {
            MULTI_AFTER_STREAK
        } else {
            Duration::ZERO
        };
        lines.push((Line::Multi(multi), wait));
    }
    lines
}

/// A message of the screen's kill banner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Banner {
    FirstBlood,
    /// The streak, 3 to 10 (a longer one reads as ten).
    Streak(u16),
    /// The multi-kill, 2 to 5 (a longer one reads as five), and whether it names the hero (it
    /// does not under a streak message that does).
    Multi {
        count: u16,
        named: bool,
    },
}

impl Banner {
    /// The localisation key of its words.
    pub fn key(self) -> String {
        match self {
            Banner::FirstBlood => "DOTA_Chat_FirstBlood".to_owned(),
            Banner::Streak(n) => format!("DOTA_Chat_New_KillStreak_{}", n.clamp(3, 10)),
            Banner::Multi { count, named } => {
                let word = ["DoubleKill", "TripleKill", "QuadKill", "QuintupleKill"]
                    [usize::from(count.clamp(2, 5)) - 2];
                let tail = if named { "" } else { "_noname" };
                format!("DOTA_Chat_{word}{tail}")
            }
        }
    }

    /// Whether it is shaken red (Dota's `Rampage`).
    pub fn rampage(self) -> bool {
        matches!(self, Banner::Multi { count, .. } if count >= 5)
    }
}

/// The messages the banner shows for a call, in order.
pub fn banners(call: &Call) -> Vec<Banner> {
    if call.first_blood {
        return vec![Banner::FirstBlood];
    }
    let streak = call.streak.map(Banner::Streak);
    let multi = call.multi.map(|count| Banner::Multi {
        count,
        named: streak.is_none(),
    });
    streak.into_iter().chain(multi).collect()
}

/// What the frame's falls announced, in order.
#[derive(Resource, Default, Debug)]
pub struct Calls(pub Vec<Call>);

/// Every call made since the match began, for the tests.
#[cfg(test)]
#[derive(Resource, Default, Debug)]
pub struct CallLog(pub Vec<Call>);

/// The counting of the match on screen and the tick it last read.
#[derive(Default)]
pub struct Counting {
    tracker: Option<Tracker>,
    tick: u32,
    epoch: u32,
}

impl Counting {
    /// What the falls the core told this frame announce. Another game, or a replay sought back,
    /// starts the counting afresh.
    pub fn read(&mut self, core: &Core, epoch: u32) -> Vec<Call> {
        let Some(view) = core.view() else {
            self.tracker = None;
            return Vec::new();
        };
        if self.epoch != epoch || view.tick < self.tick {
            self.tracker = None;
            self.epoch = epoch;
        }
        self.tick = view.tick;
        let window = MULTI_KILL_SECONDS * u32::from(core.tick_rate().max(1));
        let tracker = self
            .tracker
            .get_or_insert_with(|| Tracker::new(window, kills_before(core)));
        let mut calls = Vec::new();
        for (n, event) in core.told.iter().enumerate() {
            let tick = core.told_ticks.get(n).copied().unwrap_or(view.tick);
            let killer_team = match event {
                EventKind::Died {
                    killer: Some(killer),
                    ..
                } => core.freshest(*killer).map(|killer| killer.team),
                _ => None,
            };
            if let Some(call) =
                fall_of(event, tick, killer_team).and_then(|fall| tracker.fall(fall))
            {
                calls.push(call);
            }
        }
        calls
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Calls>()
        .add_systems(Update, observe.in_set(Observed));
    #[cfg(test)]
    app.init_resource::<CallLog>();
}

/// Where the frame's [`Calls`] are ready.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Observed;

/// The fall an event tells, if it is one; `killer_team` is the side of the unit that struck the
/// last blow, when it is told and known.
fn fall_of(event: &EventKind, tick: u32, killer_team: Option<Team>) -> Option<Fall> {
    let EventKind::Died {
        unit,
        denied,
        assists,
        credited_hero,
        ..
    } = event
    else {
        return None;
    };
    let neutral = killer_team == Some(Team::Neutral);
    let credited = credited_hero.as_deref().copied().filter(|_| !denied);
    Some(Fall {
        tick,
        victim: *unit,
        credited,
        ends_streak: !denied && !neutral,
        shared: credited.is_none() && !denied && !neutral && *assists != 0,
    })
}

/// Whether the scores show a kill already made, less those this frame's falls add.
fn kills_before(core: &Core) -> bool {
    let Some(view) = core.view() else {
        return false;
    };
    let counted: usize = view.players.iter().map(|p| usize::from(p.kills)).sum();
    let told = core
        .told
        .iter()
        .filter(|event| {
            matches!(
                event,
                EventKind::Died {
                    credited_hero: Some(_),
                    denied: false,
                    ..
                }
            )
        })
        .count();
    counted > told
}

/// A fallen hero as the feed names it: its seat, else its side.
fn victim_of(core: &Core, id: EntityId) -> Option<Victim> {
    let seat = core
        .view()?
        .players
        .iter()
        .find(|player| player.unit == Some(id))
        .map(|player| player.slot)
        .or_else(|| core.freshest(id)?.owner);
    match (seat, core.freshest(id)) {
        (Some(slot), _) => Some(Victim::Seat(slot)),
        (None, Some(unit)) => Some(Victim::Hero(unit.team)),
        (None, None) => None,
    }
}

fn observe(
    mut session: NonSendMut<Session>,
    epoch: Res<crate::main_menu::GameEpoch>,
    mut calls: ResMut<Calls>,
    mut counting: Local<Counting>,
    #[cfg(test)] mut log: ResMut<CallLog>,
) {
    let core = &mut session.0;
    calls.0 = counting.read(core, epoch.0);
    #[cfg(test)]
    log.0.extend(calls.0.iter().copied());
    // The end of a streak is a line of the feed, as in Dota's event column.
    for call in &calls.0 {
        let (Some(streak), Some(victim)) = (call.ended, victim_of(core, call.victim)) else {
            continue;
        };
        core.push_feed(FeedKind::StreakEnded { victim, streak });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: u32 = 540;

    fn hero(n: u32) -> EntityId {
        EntityId {
            idx: n,
            generation: 1,
        }
    }

    fn kill(tick: u32, victim: u32, killer: u32) -> Fall {
        Fall {
            tick,
            victim: hero(victim),
            credited: Some(hero(killer)),
            ends_streak: true,
            shared: false,
        }
    }

    fn death(tick: u32, victim: u32) -> Fall {
        Fall {
            tick,
            victim: hero(victim),
            credited: None,
            ends_streak: true,
            shared: false,
        }
    }

    fn tracker() -> Tracker {
        Tracker::new(WINDOW, false)
    }

    #[test]
    fn the_first_credited_kill_is_first_blood_and_only_it() {
        let mut t = tracker();
        let first = t.fall(kill(10, 2, 1)).expect("a call");
        let second = t.fall(kill(20, 3, 4)).expect("a call");
        assert!(first.first_blood);
        assert!(!second.first_blood);
    }

    #[test]
    fn a_fall_nobody_is_credited_with_is_not_first_blood_and_not_a_call() {
        let mut t = tracker();
        assert_eq!(t.fall(death(10, 2)), None);
        assert!(t.fall(kill(20, 2, 1)).expect("a call").first_blood);
    }

    #[test]
    fn a_match_with_a_kill_already_has_no_first_blood() {
        let mut t = Tracker::new(WINDOW, true);
        assert!(!t.fall(kill(10, 2, 1)).expect("a call").first_blood);
    }

    #[test]
    fn a_kill_of_oneself_is_no_kill() {
        let mut t = tracker();
        assert_eq!(t.fall(kill(10, 1, 1)), None);
    }

    #[test]
    fn kills_inside_the_window_make_a_double_a_triple_and_on() {
        let mut t = tracker();
        let multis: Vec<_> = (0..6)
            .map(|n| t.fall(kill(100 + n * 10, 10 + n, 1)).expect("a call").multi)
            .collect();
        assert_eq!(
            multis,
            [None, Some(2), Some(3), Some(4), Some(5), Some(6)],
            "a multi-kill counts on past five"
        );
    }

    #[test]
    fn the_window_is_open_one_tick_before_it_ends_and_closed_at_it() {
        let mut t = tracker();
        t.fall(kill(100, 2, 1));
        let open = t.fall(kill(100 + WINDOW - 1, 3, 1)).expect("a call");
        assert_eq!(open.multi, Some(2));
        let mut t = tracker();
        t.fall(kill(100, 2, 1));
        let closed = t.fall(kill(100 + WINDOW, 3, 1)).expect("a call");
        assert_eq!(closed.multi, None);
    }

    #[test]
    fn the_window_slides_from_the_previous_kill_not_the_first() {
        let mut t = tracker();
        t.fall(kill(0, 2, 1));
        t.fall(kill(WINDOW - 1, 3, 1));
        let third = t.fall(kill(2 * WINDOW - 2, 4, 1)).expect("a call");
        assert_eq!(third.multi, Some(3));
    }

    #[test]
    fn a_kill_after_the_window_starts_a_new_multi_kill_but_goes_on_the_streak() {
        let mut t = tracker();
        t.fall(kill(0, 2, 1));
        t.fall(kill(10, 3, 1));
        let late = t.fall(kill(10 + WINDOW, 4, 1)).expect("a call");
        assert_eq!(late.multi, None);
        assert_eq!(late.streak, Some(3));
    }

    #[test]
    fn multi_kills_are_counted_for_each_hero_apart() {
        let mut t = tracker();
        t.fall(kill(0, 2, 1));
        let other = t.fall(kill(5, 3, 4)).expect("a call");
        assert_eq!(other.multi, None);
        assert_eq!(t.fall(kill(10, 5, 1)).expect("a call").multi, Some(2));
    }

    #[test]
    fn a_streak_is_called_from_the_third_kill_and_goes_on_past_ten() {
        let mut t = tracker();
        let streaks: Vec<_> = (0..12)
            .map(|n| {
                t.fall(kill(n * WINDOW * 2, 10 + n, 1))
                    .expect("a call")
                    .streak
            })
            .collect();
        let want: Vec<_> = (0..12u16)
            .map(|n| (n + 1 >= STREAK_FROM).then_some(n + 1))
            .collect();
        assert_eq!(streaks, want);
    }

    #[test]
    fn a_fall_to_an_enemy_ends_the_victims_streak_and_names_how_long_it_was() {
        let mut t = tracker();
        for n in 0..4 {
            t.fall(kill(n * 2000, 10 + n, 1));
        }
        let ended = t.fall(kill(9000, 1, 7)).expect("a call");
        assert_eq!(ended.ended, Some(4));
        let again = t.fall(kill(9500, 8, 1)).expect("a call");
        assert_eq!(again.streak, None, "the streak starts over");
    }

    #[test]
    fn a_fall_to_a_creep_ends_the_streak_without_a_call() {
        let mut t = tracker();
        for n in 0..3 {
            t.fall(kill(n * 2000, 10 + n, 1));
        }
        assert_eq!(t.fall(death(9000, 1)), None);
        let next = t.fall(kill(9500, 8, 1)).expect("a call");
        assert_eq!(next.streak, None, "the streak starts over");
    }

    #[test]
    fn a_fall_that_ends_nothing_keeps_the_streak() {
        let mut t = tracker();
        for n in 0..3 {
            t.fall(kill(n * 2000, 10 + n, 1));
        }
        let mut denied = death(9000, 1);
        denied.ends_streak = false;
        assert_eq!(t.fall(denied), None, "a deny, a suicide or neutrals");
        let next = t.fall(kill(9500, 8, 1)).expect("a call");
        assert_eq!(next.streak, Some(4));
    }

    #[test]
    fn a_fall_ends_a_streak_shorter_than_a_call_without_a_word() {
        let mut t = tracker();
        t.fall(kill(0, 2, 1));
        t.fall(kill(10, 3, 1));
        assert_eq!(t.fall(death(20, 1)), None);
        assert_eq!(t.fall(kill(30, 4, 1)).expect("a call").streak, None);
    }

    #[test]
    fn a_death_does_not_close_the_victims_multi_kill_window() {
        let mut t = tracker();
        t.fall(kill(0, 2, 1));
        t.fall(death(5, 1));
        assert_eq!(t.fall(kill(10, 3, 1)).expect("a call").multi, Some(2));
    }

    #[test]
    fn a_kill_that_ends_a_streak_can_also_be_the_killers_own_call() {
        let mut t = tracker();
        for n in 0..3 {
            t.fall(kill(n * 2000, 10 + n, 1));
        }
        t.fall(kill(7000, 20, 5));
        t.fall(kill(7010, 21, 5));
        let both = t.fall(kill(7020, 1, 5)).expect("a call");
        assert_eq!(
            (both.streak, both.multi, both.ended),
            (Some(3), Some(3), Some(3))
        );
    }

    #[test]
    fn heroes_beyond_the_limit_are_not_counted() {
        let mut t = tracker();
        for n in 0..MAX_HEROES as u32 {
            t.fall(kill(n, 1000 + n, 100 + n));
        }
        assert_eq!(t.fall(kill(999, 2000, 5000)), None);
    }

    fn call(first_blood: bool, multi: Option<u16>, streak: Option<u16>) -> Call {
        Call {
            tick: 0,
            victim: hero(2),
            killer: hero(1),
            first_blood,
            multi,
            streak,
            ended: None,
        }
    }

    #[test]
    fn first_blood_is_spoken_and_shown_alone() {
        let c = call(true, Some(2), Some(3));
        assert_eq!(spoken(&c), [(Line::FirstBlood, Duration::ZERO)]);
        assert_eq!(banners(&c), [Banner::FirstBlood]);
    }

    #[test]
    fn a_multi_kill_under_a_streak_is_spoken_after_it_and_shown_without_the_name() {
        let c = call(false, Some(3), Some(4));
        assert_eq!(
            spoken(&c),
            [
                (Line::Streak(4), Duration::ZERO),
                (Line::Multi(3), MULTI_AFTER_STREAK)
            ]
        );
        assert_eq!(
            banners(&c),
            [
                Banner::Streak(4),
                Banner::Multi {
                    count: 3,
                    named: false
                }
            ]
        );
    }

    #[test]
    fn a_multi_kill_alone_is_spoken_at_once_and_shown_with_the_name() {
        let c = call(false, Some(2), None);
        assert_eq!(spoken(&c), [(Line::Multi(2), Duration::ZERO)]);
        assert_eq!(
            banners(&c),
            [Banner::Multi {
                count: 2,
                named: true
            }]
        );
    }

    #[test]
    fn a_plain_kill_says_nothing() {
        let c = call(false, None, None);
        assert!(spoken(&c).is_empty());
        assert!(banners(&c).is_empty());
    }

    #[test]
    fn every_count_has_the_pack_line_and_the_banner_words_the_game_clamps_it_to() {
        let multi: Vec<_> = (2..=7).map(|n| Line::Multi(n).logical()).collect();
        assert_eq!(multi[0], "sound.announcer.kill.double");
        assert_eq!(multi[1], "sound.announcer.kill.triple");
        assert_eq!(multi[2], "sound.announcer.kill.ultra");
        assert!(
            multi[3..]
                .iter()
                .all(|l| *l == "sound.announcer.kill.rampage")
        );
        let streaks: Vec<_> = [3, 9, 10, 11, 40]
            .map(|n| Line::Streak(n).logical())
            .to_vec();
        assert_eq!(
            streaks,
            [
                "sound.announcer.kill.streak.3",
                "sound.announcer.kill.streak.9",
                "sound.announcer.kill.streak.max",
                "sound.announcer.kill.streak.max",
                "sound.announcer.kill.streak.max"
            ]
        );
        assert_eq!(Banner::Streak(11).key(), "DOTA_Chat_New_KillStreak_10");
        assert_eq!(
            Banner::Multi {
                count: 9,
                named: false
            }
            .key(),
            "DOTA_Chat_QuintupleKill_noname"
        );
        assert!(
            Banner::Multi {
                count: 6,
                named: true
            }
            .rampage()
        );
        assert!(
            !Banner::Multi {
                count: 4,
                named: true
            }
            .rampage()
        );
    }

    fn died(credited: Option<u32>, denied: bool, assists: u16) -> EventKind {
        EventKind::Died {
            unit: hero(2),
            killer: None,
            denied,
            gold: 0,
            bounty: None,
            assists,
            awards: None,
            credited_hero: credited.map(|n| Box::new(hero(n))),
        }
    }

    #[test]
    fn a_kill_shared_by_several_heroes_spends_first_blood_without_a_call() {
        let mut t = tracker();
        let shared = fall_of(&died(None, false, 0b101), 10, Some(Team::Dire)).expect("a fall");
        assert!(shared.shared);
        assert_eq!(t.fall(shared), None);
        assert!(!t.fall(kill(20, 3, 1)).expect("a call").first_blood);
    }

    #[test]
    fn a_hero_that_neutrals_finished_spends_nothing_and_ends_no_streak() {
        let fall = fall_of(&died(None, false, 0b1), 10, Some(Team::Neutral)).expect("a fall");
        assert!(!fall.shared, "no kill, so no First Blood spent");
        assert!(!fall.ends_streak);
        let mut t = tracker();
        t.fall(fall);
        assert!(t.fall(kill(20, 3, 1)).expect("a call").first_blood);
    }

    #[test]
    fn a_deny_is_no_kill_and_ends_no_streak() {
        let fall = fall_of(&died(Some(1), true, 0), 10, Some(Team::Radiant)).expect("a fall");
        assert_eq!(fall.credited, None);
        assert!(!fall.ends_streak);
        assert!(!fall.shared);
    }

    #[test]
    fn a_hero_kill_names_its_credited_hero_and_a_hidden_killer_still_ends_the_streak() {
        let fall = fall_of(&died(Some(7), false, 0), 10, None).expect("a fall");
        assert_eq!(fall.credited, Some(hero(7)));
        assert!(fall.ends_streak);
    }

    #[test]
    fn only_a_death_is_a_fall() {
        let event = EventKind::LevelUp {
            unit: hero(1),
            level: 2,
        };
        assert_eq!(fall_of(&event, 1, None), None);
    }

    #[test]
    fn every_line_the_rules_can_speak_is_one_the_voices_load() {
        let lines = [Line::FirstBlood]
            .into_iter()
            .chain((2..=9).map(Line::Multi))
            .chain((3..=14).map(Line::Streak));
        for line in lines {
            assert!(LINES.contains(&line.logical()), "{line:?}");
        }
    }
}
