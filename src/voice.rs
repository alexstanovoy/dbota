//! The voices: one's hero answering its move and attack orders as in Dota
//! (now and then, not on every click) and speaking of the runes it takes and
//! bottles, the announcer at hero selection and through the pre-game with the
//! ancients' horn, the rooster at daybreak and the wolf at nightfall, and the
//! click of the interface.
//! Silent where the cache has no such sound.

use std::sync::Arc;

use bevy::{audio::Volume, prelude::*};
use bota_client_core::Phase;
use bota_proto::{EventKind, Order, Team};

use crate::config::Settings;
use crate::manifest::{Manifest, cache_path};
use crate::session::Session;
use crate::talk::{LineFace, Speech};

/// A cue of the pre-game: the announcer's lines and the ancients' horn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PregameCue {
    /// "Prepare for battle" (Dota's `IsPrepStart`, as its pre-game starts).
    Prepare,
    /// "Thirty seconds to battle" (`IsGameStarting30`).
    ThirtySeconds,
    /// Each side's ancient's horn (the map's `GameStart.RadiantAncient` and
    /// `GameStart.DireAncient`).
    Horn,
    /// "The battle begins!" (`IsGameStart`).
    BattleBegins,
}

/// The pre-game's cues and the seconds before 0:00 at which each starts:
/// owner-confirmed current Dota timings (2026-10-30), the times one hears in
/// a game of Dota today. The install's announcer rules name the lines; Dota
/// no longer plays its "Ten seconds to battle" (`IsGameStarting10`). A cue
/// that would start before the match's pre-game does (a pre-game shorter
/// than Dota's 1:30) is left out.
pub const PREGAME_CUES: [(PregameCue, u32); 4] = [
    (PregameCue::Prepare, 90),
    (PregameCue::ThirtySeconds, 30),
    (PregameCue::Horn, 12),
    (PregameCue::BattleBegins, 3),
];

/// Seconds after its time within which a cue still starts: a match joined,
/// a reconnect or a replay sought past it plays none it has missed.
const CUE_GRACE_SECS: u32 = 2;

/// The pre-game cues played in the match on screen, each with the game tick
/// it started on, whether or not they could be heard (muted, no sound
/// device): for the tests and tools.
#[derive(Resource, Default, Debug)]
pub struct PregameHeard(pub Vec<(PregameCue, u32)>);
/// Seconds a hero keeps quiet after answering an order.
const VOICE_REST: f32 = 6.0;

/// The sounds of one logical name, played in turn.
#[derive(Default)]
struct Lines {
    sources: Vec<Handle<AudioSource>>,
    /// Per sound, the face it is said with (a voice line's lip sync).
    faces: Vec<Option<Arc<LineFace>>>,
    next: usize,
}

impl Lines {
    fn load(manifest: &Manifest, server: &AssetServer, name: &str) -> Lines {
        Lines::of(manifest, server, manifest.sound_files(name))
    }

    fn of<'a>(
        manifest: &Manifest,
        server: &AssetServer,
        files: impl Iterator<Item = &'a str>,
    ) -> Lines {
        let (sources, faces) = files
            .map(|rel| {
                (
                    server.load(cache_path(rel)),
                    crate::talk::lipsync(manifest, rel),
                )
            })
            .unzip();
        Lines {
            sources,
            faces,
            next: 0,
        }
    }

    /// The next of the sounds, if there are any.
    fn take(&mut self) -> Option<Handle<AudioSource>> {
        self.take_line().map(|(source, _)| source)
    }

    /// The next of the sounds and its face.
    fn take_line(&mut self) -> Option<(Handle<AudioSource>, Option<Arc<LineFace>>)> {
        let at = self.next % self.sources.len().max(1);
        let source = self.sources.get(at)?.clone();
        self.next += 1;
        Some((source, self.faces.get(at).cloned().flatten()))
    }
}

/// Seconds a queued announcer line may wait for the one before it; older
/// ones are dropped, as a line about something long past would mislead.
const ANNOUNCER_PATIENCE: f32 = 3.0;

/// The announcer's lines in the order they were called: one speaks at a time,
/// and the next starts when it ends, as in Dota.
#[derive(Resource, Default)]
struct Announcer {
    waiting: std::collections::VecDeque<(Handle<AudioSource>, f32, f32)>,
    /// The line being spoken.
    speaking: Option<Entity>,
    /// Seconds since startup at which it began.
    began: f32,
}

impl Announcer {
    fn push(&mut self, source: Option<Handle<AudioSource>>, gain: f32, now: f32) {
        if let Some(source) = source {
            self.waiting.push_back((source, gain, now));
        }
    }

    /// The line to start now, if none is speaking: the oldest that is not
    /// stale.
    fn next(&mut self, now: f32, speaking: bool) -> Option<(Handle<AudioSource>, f32)> {
        if speaking {
            return None;
        }
        while let Some((source, gain, queued)) = self.waiting.pop_front() {
            if now - queued <= ANNOUNCER_PATIENCE {
                return Some((source, gain));
            }
        }
        None
    }
}

#[derive(Component)]
struct AnnouncerLine;

#[derive(Resource, Default)]
struct Voices {
    /// By hero Dota name: its move and attack answers.
    heroes: Vec<(String, Lines, Lines)>,
    /// A hero's lines of the runes it takes and bottles, by their logical name
    /// (`sound.voice.<hero>.rune.<rune art>`, `….rune.bottle`).
    runes: Vec<(String, Lines)>,
    click: Lines,
    /// The power button's own click (Dota's `ui.menu_quit`).
    quit: Lines,
    choose: Lines,
    prepare: Lines,
    thirty: Lines,
    begins: Lines,
    /// "Radiant's / Dire's structures are fortified."
    fortified: [Lines; 2],
    /// The horn at each side's ancient: Radiant's, then Dire's.
    horns: [Lines; 2],
    /// Daybreak's birds and rooster, and nightfall's howl.
    day: Lines,
    night: Lines,
}

/// What the voices remember between frames.
#[derive(Default)]
struct Heard {
    /// The last order sent that was looked at.
    seq: u32,
    /// Seconds before the hero may answer again.
    rest: f32,
    /// Whether the announcer has asked for a hero.
    asked: bool,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Voices>()
        .init_resource::<Announcer>()
        .init_resource::<PregameHeard>()
        .add_systems(Startup, load_menu)
        .add_systems(Update, load.run_if(crate::loading::requested))
        .add_systems(PostUpdate, report.in_set(crate::loading::Report))
        .add_systems(
            Update,
            (
                answer, rune_lines, announce, fortified, herald, click, speak,
            ),
        );
}

fn load(mut voices: ResMut<Voices>, manifest: Res<Manifest>, server: Res<AssetServer>) {
    for id in 0..bota_core::HEROES.len() as u16 {
        let Some(name) = manifest.dota_name("hero", id) else {
            continue;
        };
        let moves = Lines::load(&manifest, &server, &format!("sound.voice.{name}.move"));
        let attacks = Lines::load(&manifest, &server, &format!("sound.voice.{name}.attack"));
        voices.heroes.push((name.to_string(), moves, attacks));
        let arts = bota_proto::RuneKind::ALL.map(crate::runes::art);
        for what in arts.into_iter().chain(["bottle"]) {
            let logical = format!("sound.voice.{name}.rune.{what}");
            let lines = Lines::load(&manifest, &server, &logical);
            if !lines.sources.is_empty() {
                voices.runes.push((logical, lines));
            }
        }
    }
    voices.choose = Lines::load(&manifest, &server, "sound.announcer.choose_hero");
    voices.prepare = Lines::load(&manifest, &server, "sound.announcer.battle_prepare");
    voices.thirty = Lines::load(&manifest, &server, "sound.announcer.battle_in_30");
    voices.begins = Lines::load(&manifest, &server, "sound.announcer.battle_begins");
    voices.day = Lines::load(&manifest, &server, "sound.day");
    voices.night = Lines::load(&manifest, &server, "sound.night");
    for (i, side) in ["radiant", "dire"].into_iter().enumerate() {
        voices.fortified[i] = Lines::load(
            &manifest,
            &server,
            &format!("sound.announcer.fortified.{side}"),
        );
        voices.horns[i] = Lines::of(
            &manifest,
            &server,
            manifest
                .sound_files("sound.horn")
                .filter(|p| p.contains(side)),
        );
    }
}

/// Plays a sound once; the entity playing it.
fn say(commands: &mut Commands, source: Option<Handle<AudioSource>>, gain: f32) -> Option<Entity> {
    source.map(|source| {
        commands
            .spawn((
                AudioPlayer::new(source),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
            ))
            .id()
    })
}

/// One's hero answers a move or an attack order, then keeps quiet a while. Muted, the hero
/// still says it, unheard: its portrait's mouth moves (Dota's responses do not hang on the
/// volume).
fn answer(
    mut commands: Commands,
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    mut voices: ResMut<Voices>,
    mut speech: ResMut<Speech>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    time: Res<Time>,
    real: Res<Time<bevy::time::Real>>,
    mut heard: Local<Heard>,
) {
    let core = &session.0;
    heard.rest = (heard.rest - time.delta_secs()).max(0.0);
    if core.seq() == heard.seq {
        return;
    }
    heard.seq = core.seq();
    if !settings.voice || heard.rest > 0.0 {
        return;
    }
    let (Some(slot), Some(view)) = (core.my_slot, core.view()) else {
        return;
    };
    let Some(order) = core
        .shown_orders
        .iter()
        .find(|s| s.slot == slot && s.unit.is_none_or(|u| Some(u) == core.my_hero()))
        .map(|s| s.order)
    else {
        return;
    };
    let attack = match order {
        Order::Move { .. } => false,
        Order::Attack { .. } => true,
        _ => return,
    };
    let Some(name) = view
        .player(slot)
        .and_then(|p| manifest.dota_name("hero", p.hero.0))
    else {
        return;
    };
    let Some((_, moves, attacks)) = voices.heroes.iter_mut().find(|(n, _, _)| n == name) else {
        return;
    };
    let Some((source, face)) = (if attack {
        attacks.take_line()
    } else {
        moves.take_line()
    }) else {
        return;
    };
    heard.rest = VOICE_REST;
    let player = (!settings.muted && !args.mute)
        .then(|| say(&mut commands, Some(source), 0.8))
        .flatten();
    if let Some(face) = face {
        speech.say(core.my_hero(), face, player, real.elapsed_secs());
    }
}

/// One's hero speaks of a rune it took into effect, at once or out of its Bottle, and of one it
/// put into its Bottle, as Dota's heroes do, to its own player alone. Counted in the bound
/// effects' [`crate::fx::Heard`] by the line's logical name, muted or not.
fn rune_lines(
    mut commands: Commands,
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    mut voices: ResMut<Voices>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut heard: ResMut<crate::fx::Heard>,
) {
    let core = &session.0;
    let (Some(me), Some(view)) = (core.my_hero(), core.view()) else {
        return;
    };
    let Some(hero) = core
        .my_slot
        .and_then(|slot| view.player(slot))
        .and_then(|p| manifest.dota_name("hero", p.hero.0))
    else {
        return;
    };
    for event in &core.told {
        let what = match *event {
            EventKind::RuneActivated { unit, kind, .. } if unit == me => crate::runes::art(kind),
            EventKind::RuneBottled { unit, .. } if unit == me => "bottle",
            _ => continue,
        };
        let logical = format!("sound.voice.{hero}.rune.{what}");
        let Some((_, lines)) = voices.runes.iter_mut().find(|(name, _)| *name == logical) else {
            continue;
        };
        *heard.0.entry(logical).or_default() += 1;
        if settings.voice && !settings.muted && !args.mute {
            say(&mut commands, lines.take(), 0.8);
        }
    }
}

/// The announcer asks for a hero once the selection opens; through the pre-game the announcer
/// and the ancients play [`PREGAME_CUES`] on the game clock, each once.
#[allow(clippy::too_many_arguments)]
fn announce(
    mut commands: Commands,
    session: NonSend<Session>,
    mut voices: ResMut<Voices>,
    mut announcer: ResMut<Announcer>,
    mut played: ResMut<PregameHeard>,
    time: Res<Time>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut heard: Local<Heard>,
) {
    let core = &session.0;
    let silent = settings.muted || args.mute;
    if core.phase == Phase::Lobby && core.my_slot.is_some() && !heard.asked {
        heard.asked = true;
        if !silent {
            let source = voices.choose.take();
            announcer.push(source, 0.9, time.elapsed_secs());
        }
    }
    let Some(view) = core.view() else {
        played.0.clear();
        return;
    };
    let rate = u32::from(core.tick_rate().max(1));
    let pregame = core.pregame_ticks();
    // On the game clock: `-startgame` brings the horn on early.
    let now = view.game_tick();
    for (cue, before) in PREGAME_CUES {
        let at = pregame.checked_sub(before * rate);
        let due = at.is_some_and(|at| now >= at && now < at + CUE_GRACE_SECS * rate);
        if !due || played.0.iter().any(|(c, _)| *c == cue) {
            continue;
        }
        played.0.push((cue, now));
        if silent {
            continue;
        }
        match cue {
            PregameCue::Prepare => {
                let line = voices.prepare.take();
                announcer.push(line, 0.9, time.elapsed_secs());
            }
            PregameCue::ThirtySeconds => {
                let line = voices.thirty.take();
                announcer.push(line, 0.9, time.elapsed_secs());
            }
            PregameCue::Horn => {
                // The horn of one's own side's ancient.
                let dire = core.my_team() == Some(bota_proto::Team::Dire);
                let horn = voices.horns[usize::from(dire)].take();
                say(&mut commands, horn, 0.8);
            }
            PregameCue::BattleBegins => {
                let line = voices.begins.take();
                announcer.push(line, 0.9, time.elapsed_secs());
            }
        }
    }
}

/// The announcer's line when a side uses its Glyph, heard by every seat and
/// spectator as Dota's announcer says it to both teams.
fn fortified(
    session: NonSend<Session>,
    mut voices: ResMut<Voices>,
    mut announcer: ResMut<Announcer>,
    time: Res<Time>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut was: Local<Option<[bool; 2]>>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let now = [Team::Radiant, Team::Dire].map(|team| {
        view.glyphs
            .iter()
            .any(|g| g.team == team && g.active_for > 0)
    });
    // Never for the first view: opening a replay mid-glyph announces nothing.
    let before = was.replace(now);
    let Some(before) = before else {
        return;
    };
    if settings.muted || args.mute {
        return;
    }
    for side in 0..2 {
        if now[side] && !before[side] {
            let line = voices.fortified[side].take();
            announcer.push(line, 0.9, time.elapsed_secs());
        }
    }
}

/// Starts the next announcer line once the one before it has ended.
fn speak(
    mut commands: Commands,
    mut announcer: ResMut<Announcer>,
    time: Res<Time>,
    lines: Query<(), With<AnnouncerLine>>,
) {
    let now = time.elapsed_secs();
    // A line spawned this frame is not in the query yet.
    let speaking =
        announcer.speaking.is_some() && (now - announcer.began < 0.2 || !lines.is_empty());
    if !speaking {
        announcer.speaking = None;
    }
    if let Some((source, gain)) = announcer.next(now, speaking) {
        let line = commands
            .spawn((
                AnnouncerLine,
                AudioPlayer::new(source),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
            ))
            .id();
        announcer.speaking = Some(line);
        announcer.began = now;
    }
}

/// Daybreak and nightfall, as Dota sounds them (`General.Morning`,
/// `General.Night`), when the time of day turns between two views of the
/// match; never for the first view, so joining or opening a replay at night
/// howls at nobody.
fn herald(
    mut commands: Commands,
    session: NonSend<Session>,
    mut voices: ResMut<Voices>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    mut was_night: Local<Option<bool>>,
) {
    let night = session
        .0
        .view()
        .map(|view| bota_core::time_of_day(view.day_tick()).night);
    let turned = matches!((*was_night, night), (Some(was), Some(now)) if was != now);
    *was_night = night;
    if !turned || settings.muted || args.mute {
        return;
    }
    let source = if night == Some(true) {
        voices.night.take()
    } else {
        voices.day.take()
    };
    say(&mut commands, source, 0.5);
}

/// A button that sounds as Dota's quit button does.
#[derive(Component)]
pub struct QuitClick;

/// The click of any button of the interface.
fn click(
    mut commands: Commands,
    mut voices: ResMut<Voices>,
    buttons: Query<(&Interaction, Has<QuitClick>), (Changed<Interaction>, With<Button>)>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
) {
    if settings.muted || args.mute {
        return;
    }
    let Some((_, quit)) = buttons.iter().find(|(i, _)| **i == Interaction::Pressed) else {
        return;
    };
    let source = if quit {
        voices.quit.take()
    } else {
        voices.click.take()
    };
    say(&mut commands, source, 0.5);
}

/// The menu's own sounds, there from the start: the clicks of its buttons.
fn load_menu(mut voices: ResMut<Voices>, manifest: Res<Manifest>, server: Res<AssetServer>) {
    voices.click = Lines::load(&manifest, &server, "sound.ui.click");
    voices.quit = Lines::load(&manifest, &server, "sound.ui.quit");
}

/// The match's voices' part of the loading: the heroes' lines (their runes'
/// too), the announcer, the horns, day and night.
fn report(voices: Res<Voices>, server: Res<AssetServer>, mut parts: ResMut<crate::loading::Parts>) {
    let lines = voices
        .heroes
        .iter()
        .flat_map(|(_, moves, attacks)| [moves, attacks])
        .chain([
            &voices.choose,
            &voices.prepare,
            &voices.thirty,
            &voices.begins,
        ])
        .chain([&voices.day, &voices.night])
        .chain(voices.fortified.iter())
        .chain(voices.horns.iter())
        .chain(voices.runes.iter().map(|(_, lines)| lines));
    let (done, total) = lines.flat_map(|l| &l.sources).fold((0, 0), |(d, t), h| {
        (d + usize::from(crate::loading::settled(&server, h)), t + 1)
    });
    if total > 0 {
        parts.set("voices", done, total);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_glyphs_at_once_speak_one_after_the_other_and_a_stale_line_is_dropped() {
        let mut announcer = Announcer::default();
        announcer.push(Some(Handle::default()), 0.9, 10.0);
        announcer.push(Some(Handle::default()), 0.9, 10.0);
        assert!(announcer.next(10.0, false).is_some(), "the first at once");
        assert!(announcer.next(10.5, true).is_none(), "the second waits");
        assert!(
            announcer.next(12.0, false).is_some(),
            "and speaks when the first ends"
        );
        announcer.push(Some(Handle::default()), 0.9, 12.0);
        assert!(
            announcer
                .next(12.0 + ANNOUNCER_PATIENCE + 0.1, false)
                .is_none()
        );
    }
}
