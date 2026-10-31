//! Dota's soundscapes: the map's ambience by place and by time of day, as the map's
//! `env_soundscape`s and Dota's soundscape scripts lay it out (read by the importer into
//! `map/mid/soundscapes.txt`). The soundscape heard is the nearest one holding the listener
//! (the camera's focus, as for every sound of the client) that the time of day allows: a name
//! ending in `_day` by day, `_night` by night, any other always; turning from one to the next
//! fades over the soundscape's fade time. Its loops play while they are heard, quieter with
//! distance by their sound level; its random sounds play one of their waves now and then.

use bevy::{
    audio::{AudioSink, AudioSinkPlayback, GlobalVolume, Volume},
    prelude::*,
};

use crate::cache_text::{ScapeLine, ScapeSound};
use crate::camera::RtsCamera;
use crate::config::Settings;
use crate::daylight::Daylight;
use crate::session::Session;

/// Lines of the file read at most.
const MAX_LINES: usize = 4096;
/// Soundscapes, and sounds of all of them, kept at most.
const MAX_SCAPES: usize = 512;
const MAX_SOUNDS: usize = 2048;
/// Loops sounding at once at most: the loudest.
const MAX_LOOPS: usize = 24;
/// A loop quieter than this is not played; a random sound quieter than [`QUIETEST_RANDOM`]
/// is skipped.
const QUIETEST_LOOP: f32 = 0.005;
const QUIETEST_RANDOM: f32 = 0.03;
/// The sound level a sound placed without one has: Source's normal level.
const NORMAL_DB: f32 = 75.0;
/// bota = Dota + this, on x and y.
const DOTA_TO_BOTA: f32 = 9216.0;

/// What the soundscape plays now, whether or not there is a sound device: each soundscape's
/// name and how far it has faded in, the one heard, and for each sound its soundscape, its first
/// wave, whether it loops, its gain now (before the player's volume; a random sound's at its loudest draw) and
/// how many waves it has played. The tests and tools read it.
#[derive(Resource, Default, Debug)]
pub struct Ambience {
    pub scapes: Vec<String>,
    pub weights: Vec<f32>,
    pub heard: Option<usize>,
    pub sounds: Vec<(usize, String, bool)>,
    pub gains: Vec<f32>,
    pub played: Vec<u32>,
}

/// The soundscapes of the cache and what they play, and the state of their sounds.
#[derive(Resource, Default)]
struct Scapes {
    scapes: Vec<ScapeLine>,
    sounds: Vec<Placed>,
    /// How far each soundscape has faded in, 0 to 1.
    weights: Vec<f32>,
    /// Whether the weights were set for the view under way: the first is heard as it is.
    settled: bool,
    /// The loops' gains this frame, kept for their capacity.
    scratch: Vec<f32>,
    rng: u64,
}

/// A soundscape's sound: its line, its soundscape's index, its waves loaded, the entity of its
/// loop while it sounds, the seconds to its next random wave and the waves it played.
struct Placed {
    line: ScapeSound,
    scape: usize,
    waves: Vec<Handle<AudioSource>>,
    playing: Option<Entity>,
    next: f32,
    played: u32,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Ambience>()
        .init_resource::<Scapes>()
        .add_systems(Startup, load)
        .add_systems(Update, play);
}

/// Reads the cache's soundscapes; a cache without them plays none.
fn load(
    mut scapes: ResMut<Scapes>,
    mut ambience: ResMut<Ambience>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
) {
    let Some(text) = crate::cachefs::read_small(&manifest.root, "map/mid/soundscapes.txt")
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return;
    };
    let (mut lines, sounds) = crate::cache_text::soundscapes(&text, MAX_LINES);
    lines.truncate(MAX_SCAPES);
    let sounds: Vec<Placed> = sounds
        .into_iter()
        .filter_map(|line| {
            let scape = lines.iter().position(|s| s.name == line.scape)?;
            let waves = line
                .waves
                .iter()
                .map(|w| server.load(crate::manifest::cache_path(w)))
                .collect();
            Some(Placed {
                line,
                scape,
                waves,
                playing: None,
                next: 0.0,
                played: 0,
            })
        })
        .take(MAX_SOUNDS)
        .collect();
    *ambience = Ambience {
        scapes: lines.iter().map(|s| s.name.clone()).collect(),
        weights: vec![0.0; lines.len()],
        heard: None,
        sounds: sounds
            .iter()
            .map(|p| (p.scape, p.line.waves[0].clone(), p.line.looped))
            .collect(),
        gains: vec![0.0; sounds.len()],
        played: vec![0; sounds.len()],
    };
    info!(
        "soundscapes: {} ({} sounds)",
        ambience.scapes.join(", "),
        ambience.sounds.len()
    );
    scapes.weights = vec![0.0; lines.len()];
    scapes.scratch = Vec::with_capacity(sounds.len());
    scapes.scapes = lines;
    scapes.sounds = sounds;
    scapes.rng = 0x9E37_79B9_7F4A_7C15;
}

/// A number drawn in `[lo, hi]`.
fn draw(rng: &mut u64, [lo, hi]: [f32; 2]) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    lo + (hi - lo) * ((*rng >> 40) as f32 / (1u64 << 24) as f32)
}

/// The share of its volume a sound placed at `at` (Dota) with sound level `db` keeps for a
/// listener at `ear` (bota): all of it for a sound placed nowhere, none past Source's distance
/// for its level, `50 * (db - 50)` units, fading evenly on the way.
fn reach(at: Option<[f32; 3]>, db: f32, ear: Vec2) -> f32 {
    let Some([x, y, _]) = at else {
        return 1.0;
    };
    let db = if db > 0.0 { db } else { NORMAL_DB };
    let far = 50.0 * (db - 50.0).max(1.0);
    let d = Vec2::new(x + DOTA_TO_BOTA, y + DOTA_TO_BOTA).distance(ear);
    (1.0 - d / far).clamp(0.0, 1.0)
}

/// The soundscape to be heard by a listener at `ear` (bota), at night or by day.
fn chosen(scapes: &[ScapeLine], ear: Vec2, night: bool) -> Option<usize> {
    let allowed = |name: &str| {
        if name.ends_with("_day") {
            !night
        } else if name.ends_with("_night") {
            night
        } else {
            true
        }
    };
    scapes
        .iter()
        .enumerate()
        .filter(|(_, s)| allowed(&s.name))
        .map(|(i, s)| {
            let at = Vec2::new(s.at[0] + DOTA_TO_BOTA, s.at[1] + DOTA_TO_BOTA);
            (i, at.distance(ear), s.radius)
        })
        .filter(|(_, d, radius)| d <= radius)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, ..)| i)
}

/// Fades the soundscapes towards the one to be heard and plays their sounds.
#[allow(clippy::too_many_arguments)]
fn play(
    mut commands: Commands,
    session: NonSend<Session>,
    time: Res<Time>,
    daylight: Res<Daylight>,
    camera: Query<&RtsCamera>,
    settings: Res<Settings>,
    args: Res<crate::Args>,
    global: Res<GlobalVolume>,
    mut sinks: Query<&mut AudioSink>,
    mut scapes: ResMut<Scapes>,
    mut ambience: ResMut<Ambience>,
) {
    let scapes = &mut *scapes;
    let Ok(camera) = camera.single() else {
        return silence(&mut commands, scapes, &mut ambience);
    };
    if session.0.view().is_none() {
        return silence(&mut commands, scapes, &mut ambience);
    }
    let ear = Vec2::new(camera.focus.x, -camera.focus.z);
    let heard = chosen(&scapes.scapes, ear, daylight.night >= 0.5);
    fade(scapes, heard, time.delta_secs());
    // A replay held holds the random sounds; the loops keep sounding, as Dota's ambience does
    // in a pause.
    let pace = match &session.0.source {
        bota_client_core::Source::Replay(player) if player.paused => 0.0,
        bota_client_core::Source::Replay(player) => player.speed,
        _ => 1.0,
    };
    let master = if settings.muted || args.mute {
        0.0
    } else {
        settings.volume
    } * global.volume.to_linear();
    loops(&mut commands, scapes, ear, master, &mut sinks);
    randoms(&mut commands, scapes, ear, time.delta_secs() * pace, master);
    tell(scapes, heard, ear, &mut ambience);
}

/// Moves each soundscape's weight towards 1 for the one heard and 0 for the rest, over its
/// fade time; the first view's are set at once.
fn fade(scapes: &mut Scapes, heard: Option<usize>, dt: f32) {
    for (i, weight) in scapes.weights.iter_mut().enumerate() {
        let goal = if Some(i) == heard { 1.0 } else { 0.0 };
        let fade = scapes.scapes[i].fade;
        *weight = if !scapes.settled || fade <= 0.0 {
            goal
        } else if *weight < goal {
            (*weight + dt / fade).min(goal)
        } else {
            (*weight - dt / fade).max(goal)
        };
    }
    scapes.settled = true;
}

/// A loop's gain now, before the player's volume.
fn loop_gain(p: &Placed, weights: &[f32], ear: Vec2) -> f32 {
    p.line.volume[0] * weights[p.scape] * reach(p.line.at, p.line.db, ear)
}

/// Plays the loudest loops that are heard, at their gains, and stops the others.
fn loops(
    commands: &mut Commands,
    scapes: &mut Scapes,
    ear: Vec2,
    master: f32,
    sinks: &mut Query<&mut AudioSink>,
) {
    let Scapes {
        sounds,
        weights,
        scratch,
        ..
    } = scapes;
    scratch.clear();
    scratch.extend(
        sounds
            .iter()
            .filter(|p| p.line.looped)
            .map(|p| loop_gain(p, weights, ear))
            .filter(|g| *g > QUIETEST_LOOP),
    );
    let floor = if scratch.len() > MAX_LOOPS {
        let at = scratch.len() - MAX_LOOPS;
        *scratch.select_nth_unstable_by(at, f32::total_cmp).1
    } else {
        QUIETEST_LOOP
    };
    for p in sounds.iter_mut().filter(|p| p.line.looped) {
        let gain = loop_gain(p, weights, ear);
        let on = master > 0.0 && gain > QUIETEST_LOOP && gain >= floor;
        match (p.playing, on) {
            (Some(entity), false) => {
                commands.entity(entity).despawn();
                p.playing = None;
            }
            (Some(entity), true) => {
                if let Ok(mut sink) = sinks.get_mut(entity) {
                    sink.set_volume(Volume::Linear(gain * master));
                }
            }
            (None, true) => {
                let speed = p.line.pitch[0] / 100.0;
                let settings = PlaybackSettings::LOOP
                    .with_volume(Volume::Linear(gain * master))
                    .with_speed(speed);
                let player = AudioPlayer::new(p.waves[0].clone());
                p.playing = Some(commands.spawn((player, settings)).id());
            }
            (None, false) => {}
        }
    }
}

/// Plays a wave of each random sound whose time has come, if it is heard.
fn randoms(commands: &mut Commands, scapes: &mut Scapes, ear: Vec2, dt: f32, master: f32) {
    let Scapes {
        sounds,
        weights,
        rng,
        ..
    } = scapes;
    for p in sounds.iter_mut().filter(|p| !p.line.looped) {
        p.next -= dt;
        if p.next > 0.0 {
            continue;
        }
        p.next = draw(rng, p.line.every);
        let weight = weights[p.scape];
        let gain = draw(rng, p.line.volume) * weight * reach(p.line.at, p.line.db, ear);
        if gain < QUIETEST_RANDOM {
            continue;
        }
        let pick = (draw(rng, [0.0, 1.0]) * p.waves.len() as f32) as usize;
        let wave = p.waves[pick.min(p.waves.len() - 1)].clone();
        p.played = p.played.saturating_add(1);
        if master <= 0.0 {
            continue;
        }
        let speed = draw(rng, p.line.pitch) / 100.0;
        commands.spawn((
            AudioPlayer::new(wave),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(gain * master))
                .with_speed(speed),
        ));
    }
}

/// Stops every sound: no match is on screen.
fn silence(commands: &mut Commands, scapes: &mut Scapes, ambience: &mut Ambience) {
    for p in &mut scapes.sounds {
        if let Some(entity) = p.playing.take() {
            commands.entity(entity).despawn();
        }
    }
    scapes.weights.fill(0.0);
    scapes.settled = false;
    ambience.heard = None;
    ambience.weights.fill(0.0);
    ambience.gains.fill(0.0);
}

/// Tells what plays now, for the tests and tools.
fn tell(scapes: &Scapes, heard: Option<usize>, ear: Vec2, ambience: &mut Ambience) {
    ambience.heard = heard;
    ambience.weights.copy_from_slice(&scapes.weights);
    for (i, p) in scapes.sounds.iter().enumerate() {
        ambience.gains[i] = if p.line.looped {
            loop_gain(p, &scapes.weights, ear)
        } else {
            p.line.volume[1] * scapes.weights[p.scape] * reach(p.line.at, p.line.db, ear)
        };
        ambience.played[i] = p.played;
    }
}
