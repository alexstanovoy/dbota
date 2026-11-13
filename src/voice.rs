//! The voices: the announcer at hero selection and through the pre-game with the
//! ancients' horn, the rooster at daybreak and the wolf at nightfall, and the
//! click of the interface. What one's hero says is its own rules' (`hero_voice.rs`).
//! Silent where the cache has no such sound.

use std::time::Duration;

use bevy::{audio::Volume, prelude::*};
use bevy::{
    picking::{events::PointerPress, pointer::PointerButton},
    ui_widgets::Button,
};
use bota_client_core::Phase;
use bota_proto::{EventKind, Team, UnitKind};

use crate::config::Settings;
use crate::manifest::{Manifest, cache_path};
use crate::session::Session;

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
/// The announcer's lines of the buildings that fell, by their logical name, as they were called
/// (muted, or with no sound device, too): for the tests, which keep it from growing in a match.
#[cfg(test)]
#[derive(Resource, Default, Debug)]
pub struct BuildingsHeard(pub Vec<String>);
/// The sounds of one logical name, played in turn.
#[derive(Default)]
struct Lines {
    sources: Vec<Handle<AudioSource>>,
    next: usize,
}

impl Lines {
    fn load(manifest: &Manifest, server: &AssetServer, name: &str) -> Lines {
        Lines::of(server, manifest.sound_files(name))
    }

    fn of<'a>(server: &AssetServer, files: impl Iterator<Item = &'a str>) -> Lines {
        Lines {
            sources: files.map(|rel| server.load(cache_path(rel))).collect(),
            next: 0,
        }
    }

    /// The next of the sounds, if there are any.
    fn take(&mut self) -> Option<Handle<AudioSource>> {
        let at = self.next % self.sources.len().max(1);
        let source = self.sources.get(at)?.clone();
        self.next += 1;
        Some(source)
    }
}

/// Seconds a queued announcer line may wait for the one before it; older
/// ones are dropped, as a line about something long past would mislead.
const ANNOUNCER_PATIENCE: f32 = 3.0;

/// How long after a building's fall the announcer calls it. The install holds no such delay (the
/// response rules and the line's sound event carry none), so this is ours, the owner's "slight
/// delay after the destruction sound": the fall and its destruction sound come at once, the call
/// follows. Presentation only: the match does not wait for it.
pub const BUILDING_CALL_DELAY: Duration = Duration::from_millis(1500);

/// The announcer's lines in the order they were called: one speaks at a time,
/// and the next starts when it ends, as in Dota.
#[derive(Resource, Default)]
pub(crate) struct Announcer {
    /// Each line with its volume and the time since startup from which it may speak.
    waiting: std::collections::VecDeque<(Handle<AudioSource>, f32, f32)>,
    /// The line being spoken.
    speaking: Option<Entity>,
    /// Seconds since startup at which it began.
    began: f32,
}

impl Announcer {
    /// Forgets the lines waiting, for the tests.
    #[cfg(test)]
    pub(crate) fn waiting_clear(&mut self) {
        self.waiting.clear();
    }

    pub(crate) fn push(&mut self, source: Option<Handle<AudioSource>>, gain: f32, now: f32) {
        self.push_after(source, gain, now, Duration::ZERO);
    }

    /// [`Announcer::push`] for a line that speaks `delay` after `now`; the lines behind it wait
    /// for it, as they wait for one speaking.
    pub(crate) fn push_after(
        &mut self,
        source: Option<Handle<AudioSource>>,
        gain: f32,
        now: f32,
        delay: Duration,
    ) {
        if let Some(source) = source {
            self.waiting
                .push_back((source, gain, now + delay.as_secs_f32()));
        }
    }

    /// The line to start now, if none is speaking: the oldest, once its time has come, unless it
    /// is stale.
    fn next(&mut self, now: f32, speaking: bool) -> Option<(Handle<AudioSource>, f32)> {
        if speaking {
            return None;
        }
        while let Some((_, _, due)) = self.waiting.front() {
            if *due > now {
                return None;
            }
            let (source, gain, due) = self.waiting.pop_front()?;
            if now - due <= ANNOUNCER_PATIENCE {
                return Some((source, gain));
            }
        }
        None
    }
}

#[derive(Component)]
struct AnnouncerLine;

/// The mix group of the announcer's lines (Dota's mixer groups them by their path,
/// `announcer_`, which these lines are not played through an event to match).
const ANNOUNCER_GROUP: &str = "Announcer";

#[derive(Resource, Default)]
struct Voices {
    click: Lines,
    /// The power button's own click (Dota's `ui.menu_quit`).
    quit: Lines,
    choose: Lines,
    prepare: Lines,
    thirty: Lines,
    begins: Lines,
    /// "Radiant's / Dire's structures are fortified."
    fortified: [Lines; 2],
    /// The announcer's lines of a tower or barracks that fell, by their logical name
    /// (`sound.announcer.tower.fall.yr.top`, ...).
    buildings: Vec<(String, Lines)>,
    roshan: [Lines; 2],
    /// The horn at each side's ancient: Radiant's, then Dire's.
    horns: [Lines; 2],
    /// Daybreak's birds and rooster, and nightfall's howl.
    day: Lines,
    night: Lines,
}

/// What the voices remember between frames.
#[derive(Default)]
struct Heard {
    /// The game the memory is of: another game starts it afresh.
    epoch: u32,
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
        .add_observer(click)
        .add_systems(
            Update,
            (announce, fortified, building_fell, boss_fell, herald, speak),
        );
    #[cfg(test)]
    app.init_resource::<BuildingsHeard>();
}

fn load(mut voices: ResMut<Voices>, manifest: Res<Manifest>, server: Res<AssetServer>) {
    voices.choose = Lines::load(&manifest, &server, "sound.announcer.choose_hero");
    voices.prepare = Lines::load(&manifest, &server, "sound.announcer.battle_prepare");
    voices.thirty = Lines::load(&manifest, &server, "sound.announcer.battle_in_30");
    voices.begins = Lines::load(&manifest, &server, "sound.announcer.battle_begins");
    for what in ["tower.fall", "tower.deny", "rax.fall"] {
        for who in ["yr", "enm", "rad", "dire"] {
            for lane in ["top", "mid", "bot"] {
                let logical = format!("sound.announcer.{what}.{who}.{lane}");
                let lines = Lines::load(&manifest, &server, &logical);
                if !lines.sources.is_empty() {
                    voices.buildings.push((logical, lines));
                }
            }
        }
    }
    for who in ["you", "enemy", "radiant", "dire"] {
        let logical = format!("sound.announcer.mega.{who}");
        let lines = Lines::load(&manifest, &server, &logical);
        if !lines.sources.is_empty() {
            voices.buildings.push((logical, lines));
        }
    }
    voices.day = Lines::load(&manifest, &server, "sound.day");
    voices.night = Lines::load(&manifest, &server, "sound.night");
    for (i, side) in ["radiant", "dire"].into_iter().enumerate() {
        voices.roshan[i] = Lines::load(
            &manifest,
            &server,
            &format!("sound.announcer.roshan.{side}"),
        );
        voices.fortified[i] = Lines::load(
            &manifest,
            &server,
            &format!("sound.announcer.fortified.{side}"),
        );
        voices.horns[i] = Lines::of(
            &server,
            manifest
                .sound_files("sound.horn")
                .filter(|p| p.contains(side)),
        );
    }
}

/// Plays a sound of the game once: it ends with the match.
fn say_in_game(
    commands: &mut Commands,
    source: Option<Handle<AudioSource>>,
    gain: f32,
) -> Option<Entity> {
    let line = say(commands, source, gain)?;
    commands.entity(line).insert(crate::mixer::GameAudio);
    Some(line)
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
    epoch: Res<crate::main_menu::GameEpoch>,
) {
    if heard.epoch != epoch.0 {
        *heard = Heard {
            epoch: epoch.0,
            ..default()
        };
    }
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
                say_in_game(&mut commands, horn, 0.8);
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
        *was = None;
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

/// The logical name of the announcer's line when a building fell, as Dota's announcer rules
/// choose it (`response_rules_announcer`): a tower or barracks only; for a player the line of
/// the side that lost it (`yr`: one's own, `enm`: the other's), for a spectator that of the
/// side's own (`rad`, `dire`); a tower the lost side's own struck is a deny; by lane (0 mid,
/// 1 top, 2 bottom).
///
/// The mapping is written here, not read from the install's rules: `response_rules_announcer` is
/// a general rule engine (criteria over the speaker, the team, the event and its context) with
/// no table of these lines to read, and an engine for the whole file is far more code, and more
/// room for error, than these 36 names. The sound events themselves are the install's data (the
/// manifest), and a test holds that every name this function can make is among them.
pub fn building_line(
    my_team: Option<Team>,
    lost: Team,
    kind: UnitKind,
    lane: Option<u8>,
    denied: bool,
) -> Option<String> {
    let what = match (kind, denied) {
        (UnitKind::Tower, false) => "tower.fall",
        (UnitKind::Tower, true) => "tower.deny",
        (UnitKind::Barracks, _) => "rax.fall",
        _ => return None,
    };
    let who = match (my_team, lost) {
        (Some(mine), lost) if mine == lost => "yr",
        (Some(_), _) => "enm",
        (None, Team::Radiant) => "rad",
        (None, _) => "dire",
    };
    let lane = match lane? {
        0 => "mid",
        1 => "top",
        _ => "bot",
    };
    Some(format!("sound.announcer.{what}.{who}.{lane}"))
}

/// The install's response rules distinguish player-relative and spectator mega announcements.
pub fn mega_line(my_team: Option<Team>, team: Team) -> Option<String> {
    let who = match (my_team, team) {
        (_, Team::Neutral) => return None,
        (Some(mine), side) if mine == side => "you",
        (Some(_), _) => "enemy",
        (None, Team::Radiant) => "radiant",
        (None, Team::Dire) => "dire",
    };
    Some(format!("sound.announcer.mega.{who}"))
}

/// The announcer's line for each tower or barracks that fell in what the last pump told, told
/// to every seat and spectator as Dota's announcer does.
fn building_fell(
    session: NonSend<Session>,
    mut voices: ResMut<Voices>,
    #[cfg(test)] mut heard: ResMut<BuildingsHeard>,
    mut announcer: ResMut<Announcer>,
    time: Res<Time>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
) {
    let core = &session.0;
    let told = &core.told;
    for event in told {
        let name = if let EventKind::MegaCreeps { team } = *event {
            mega_line(core.my_team(), team)
        } else {
            let EventKind::StructureDestroyed {
                unit,
                team,
                kind,
                lane,
                ..
            } = *event
            else {
                continue;
            };
            let denied = told.iter().any(
                |e| matches!(e, EventKind::Died { unit: dead, denied: true, .. } if *dead == unit),
            );
            building_line(core.my_team(), team, kind, lane, denied)
        };
        let Some(name) = name else {
            continue;
        };
        #[cfg(test)]
        heard.0.push(name.clone());
        if settings.muted || args.mute {
            continue;
        }
        if let Some((_, lines)) = voices.buildings.iter_mut().find(|(n, _)| *n == name) {
            announcer.push_after(lines.take(), 0.9, time.elapsed_secs(), BUILDING_CALL_DELAY);
        }
    }
}

/// Runs the building announcer once over what the core was told, and answers how many lines it
/// queued for the announcer and how many of Dota's it has loaded: for the tests.
#[cfg(test)]
pub fn announce_buildings_once(world: &mut World) -> (usize, usize) {
    use bevy::ecs::system::RunSystemOnce;
    let before = world.resource::<Announcer>().waiting.len();
    world
        .run_system_once(building_fell)
        .expect("the system runs");
    let voices = world.resource::<Voices>();
    let waiting = world.resource::<Announcer>().waiting.len();
    (waiting - before, voices.buildings.len())
}

/// Starts the next announcer line once the one before it has ended.
fn speak(
    mut commands: Commands,
    mut announcer: ResMut<Announcer>,
    time: Res<Time>,
    lines: Query<(), With<AnnouncerLine>>,
    mut mixer: ResMut<crate::mixer::Mixer>,
    mut seen_ends: Local<u32>,
) {
    let now = time.elapsed_secs();
    // The lines called in a match that has ended are not read in the next.
    if *seen_ends != mixer.ends() {
        *seen_ends = mixer.ends();
        announcer.waiting.clear();
    }
    // A line spawned this frame is not in the query yet.
    let speaking =
        announcer.speaking.is_some() && (now - announcer.began < 0.2 || !lines.is_empty());
    if !speaking {
        announcer.speaking = None;
    } else {
        // The mixer's layers duck the world while the announcer speaks (Dota's `Announcer`
        // layer); a line is a sound of the announcer's group at the event's full volume.
        mixer.note_activity(ANNOUNCER_GROUP, 1.0);
    }
    if let Some((source, gain)) = announcer.next(now, speaking) {
        let line = commands
            .spawn((
                crate::mixer::GameAudio,
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
    let night = crate::daylight::night(&session.0);
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
    say_in_game(&mut commands, source, 0.5);
}

/// A button that sounds as Dota's quit button does.
#[derive(Component)]
pub struct QuitClick;

/// The click of any button of the interface.
fn click(
    event: On<PointerPress>,
    mut commands: Commands,
    mut voices: ResMut<Voices>,
    buttons: Query<Has<QuitClick>, With<Button>>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
) {
    if event.button != PointerButton::Primary || settings.muted || args.mute {
        return;
    }
    let Ok(quit) = buttons.get(event.entity) else {
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

/// The match's voices' part of the loading: the announcer, Roshan's lines, the
/// horns, day and night (one's hero's lines are its voice pack's, `hero_voice.rs`).
fn report(voices: Res<Voices>, server: Res<AssetServer>, mut parts: ResMut<crate::loading::Parts>) {
    let lines = [
        &voices.choose,
        &voices.prepare,
        &voices.thirty,
        &voices.begins,
    ]
    .into_iter()
    .chain([&voices.day, &voices.night])
    .chain(voices.fortified.iter())
    .chain(voices.buildings.iter().map(|(_, lines)| lines))
    .chain(voices.roshan.iter())
    .chain(voices.horns.iter());
    let (done, total) = lines.flat_map(|l| &l.sources).fold((0, 0), |(d, t), h| {
        (d + usize::from(crate::loading::settled(&server, h)), t + 1)
    });
    if total > 0 {
        parts.set("voices", done, total);
    }
}

fn boss_fell(
    session: NonSend<Session>,
    mut voices: ResMut<Voices>,
    mut announcer: ResMut<Announcer>,
    time: Res<Time>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
) {
    if settings.muted || args.mute {
        return;
    }
    for event in &session.0.told {
        if let EventKind::BossFell {
            objective: bota_proto::ObjectiveKind::Roshan,
            team: Some(team),
        } = *event
        {
            let line = voices.roshan[usize::from(team == Team::Dire)].take();
            announcer.push(line, 0.9, time.elapsed_secs());
        }
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
