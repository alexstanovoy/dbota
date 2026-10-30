//! Debug: `--talk-check HERO` puts the hero in the portrait (animated), saves its frames idle
//! and saying one of its move lines (`<prefix><hero>-idle-NN.png`, `-talk-NN.png`) into
//! `--screenshot-dir`, then checks that its face says the line: with its body and idle
//! expression held still, frames along the line (`-check-NN.png`) must differ from the face at
//! rest, and from each other, while the rest face drawn twice is the same picture. A mouth
//! that stays still through a line fails. Then every move and attack line of the hero, each
//! held at points along it: each must have its lip sync, and move the face's morph weights off
//! the rest face's. Prints `visual check: PASS|FAIL talking portrait`.

use std::sync::Arc;

use bevy::{
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
};

use crate::manifest::Manifest;
use crate::portrait::Stage;
use crate::talk::{LineFace, Speech};
use crate::units::ModelBank;

/// Frames of each sequence saved, and seconds between them.
const IDLE_FRAMES: usize = 8;
const TALK_FRAMES: usize = 12;
const FRAME_STEP: f32 = 0.1;
/// Frames checked along the line.
const CHECK_FRAMES: usize = 10;
/// Frames waited after a change before it is read back: the frame drawn then shows it.
const SETTLE_FRAMES: u8 = 3;
/// A pixel counts as changed when a channel moves by more than this, of 255.
const CHANGED: u8 = 8;
/// Pixels the face must move at its most against its rest, and from one checked frame to the
/// next (on at least [`MOVING_SHARE`] of the steps), out of the portrait's 352 x 312.
const MIN_MOVED: usize = 150;
const MIN_STEP: usize = 30;
const MOVING_SHARE: f32 = 0.4;
/// Points each of the hero's lines is held at, and how far at least one morph weight moves off
/// its rest at one of them.
const LINE_POINTS: usize = 6;
const MIN_LINE_MOVE: f32 = 0.1;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    /// Waiting for the stage's model and its face.
    Loading,
    Idle(usize),
    Talk(usize),
    /// Held still: the rest face, then along the line, then the rest face again.
    Rest,
    Along(usize),
    RestAgain,
    /// Each of the hero's lines (by number): its rest face (0), then held at points along it.
    Every(usize, usize),
    Done,
}

#[derive(Resource)]
struct Check {
    hero: String,
    line: Option<Arc<LineFace>>,
    phase: Phase,
    /// Seconds since the start when the phase began, and frames waited in it.
    began: f32,
    waited: u8,
    /// Whether a read-back is under way.
    reading: bool,
    rest: Option<Vec<u8>>,
    along: Vec<Vec<u8>>,
    rest_again: Option<Vec<u8>>,
    /// Every move and attack line's file, and its lip sync when it reads.
    lines: Vec<(String, Option<Arc<LineFace>>)>,
    /// The weights at rest before the line held, and how far each line moved one at most.
    line_rest: Vec<f32>,
    moved: Vec<f32>,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, start)
        .add_systems(Update, step.run_if(resource_exists::<Check>));
}

fn start(args: Res<crate::Args>, manifest: Res<Manifest>, mut commands: Commands) {
    let Some(hero) = args.talk_check.clone() else {
        return;
    };
    // The move line with the most to say.
    let line = manifest
        .sound_files(&format!("sound.voice.{hero}.move"))
        .filter_map(|rel| crate::talk::lipsync(&manifest, rel))
        .max_by(|a, b| a.lipsync.end().total_cmp(&b.lipsync.end()));
    let names = ["move", "attack"].map(|kind| format!("sound.voice.{hero}.{kind}"));
    let lines = names
        .iter()
        .flat_map(|name| manifest.sound_files(name))
        .map(|rel| (rel.to_owned(), crate::talk::lipsync(&manifest, rel)))
        .collect();
    commands.insert_resource(Check {
        hero,
        line,
        phase: Phase::Loading,
        began: 0.0,
        waited: 0,
        reading: false,
        rest: None,
        along: Vec::new(),
        rest_again: None,
        lines,
        line_rest: Vec::new(),
        moved: Vec::new(),
    });
}

/// What the check moves: the stage, the line said, the model's clips.
#[derive(bevy::ecs::system::SystemParam)]
struct Scene<'w, 's> {
    stage: ResMut<'w, Stage>,
    speech: ResMut<'w, Speech>,
    tree: Query<'w, 's, &'static Children>,
    players: Query<'w, 's, &'static mut AnimationPlayer>,
    weights: Query<'w, 's, &'static bevy::mesh::morph::MorphWeights>,
}

fn step(
    mut check: ResMut<Check>,
    mut scene: Scene,
    bank: Res<ModelBank>,
    ready: Res<crate::warmup::PipelinesReady>,
    time: Res<Time<bevy::time::Real>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs();
    if check.reading {
        return;
    }
    let phase = match check.phase {
        Phase::Every(i, _) if i >= check.lines.len() => Phase::Done,
        phase => phase,
    };
    match phase {
        Phase::Loading => {
            if let Err(why) = load(&mut check, &mut scene.stage, &bank, ready.get(), now) {
                tell(format!(
                    "visual check: FAIL talking portrait {}: {why}",
                    check.hero
                ));
                exit.write(AppExit::error());
            }
        }
        Phase::Done => {
            let pass = verdict(&check);
            exit.write(if pass {
                AppExit::Success
            } else {
                AppExit::error()
            });
        }
        Phase::Idle(_) | Phase::Talk(_) if now - check.began < FRAME_STEP => {}
        Phase::Every(i, k) => {
            if check.waited == 0 {
                hold_line(&check, &mut scene, i, k, now);
            }
            if settle(&mut check) {
                measure_line(&mut check, &scene, k);
                next(&mut check, after(phase), now);
            }
        }
        _ => {
            if check.waited == 0 {
                enter(&check, &mut scene, now);
            }
            if settle(&mut check) {
                let (name, shot) = shot(phase);
                read(&mut commands, &scene.stage, &check.hero, name, shot);
                check.reading = true;
                next(&mut check, after(phase), now);
            }
        }
    }
}

/// Puts the hero on the stage and waits for its face and every pipeline it needs.
fn load(
    check: &mut Check,
    stage: &mut Stage,
    bank: &ModelBank,
    ready: bool,
    now: f32,
) -> Result<(), String> {
    if stage.forced.is_none() {
        // The models are asked for when the loading begins, a frame or so in.
        if bank.0.is_empty() {
            return Ok(());
        }
        let logical = format!("hero.{}.model", check.hero);
        let model = bank.0.iter().position(|m| m.model.logical == logical);
        stage.forced = Some(model.ok_or(format!("no model {logical}"))?);
        stage.every = true;
    }
    if check.line.is_none() {
        return Err("no line with a lip sync".into());
    }
    if !(ready && stage.actor.as_ref().is_some_and(|a| a.face.is_some())) {
        check.began = now;
    } else if now - check.began >= 1.0 {
        next(check, Phase::Idle(0), now);
    }
    Ok(())
}

/// What a phase changes as it begins: the line starts, stops, or is held; the stage is held.
fn enter(check: &Check, scene: &mut Scene, now: f32) {
    let line = check.line.clone().expect("checked when loading");
    match check.phase {
        Phase::Talk(0) => scene.speech.say(None, line, None, now),
        Phase::Rest => {
            scene.speech.stop();
            hold(scene, now);
        }
        Phase::Along(i) => {
            let t = line.lipsync.end() * (i + 1) as f32 / (CHECK_FRAMES + 1) as f32;
            scene.speech.say(None, line, None, now);
            scene.speech.hold(t);
        }
        Phase::RestAgain => scene.speech.stop(),
        _ => {}
    }
}

/// The file a phase's frame is saved as, and what it is to the check.
fn shot(phase: Phase) -> (Option<String>, Shot) {
    match phase {
        Phase::Idle(i) => (Some(format!("idle-{i:02}")), Shot::Sequence),
        Phase::Talk(i) => (Some(format!("talk-{i:02}")), Shot::Sequence),
        Phase::Rest => (Some("check-00".into()), Shot::Rest),
        Phase::Along(i) => (Some(format!("check-{:02}", i + 1)), Shot::Along),
        _ => (None, Shot::RestAgain),
    }
}

fn after(phase: Phase) -> Phase {
    match phase {
        Phase::Idle(i) if i + 1 < IDLE_FRAMES => Phase::Idle(i + 1),
        Phase::Idle(_) => Phase::Talk(0),
        Phase::Talk(i) if i + 1 < TALK_FRAMES => Phase::Talk(i + 1),
        Phase::Talk(_) => Phase::Rest,
        Phase::Rest => Phase::Along(0),
        Phase::Along(i) if i + 1 < CHECK_FRAMES => Phase::Along(i + 1),
        Phase::Along(_) => Phase::RestAgain,
        Phase::RestAgain => Phase::Every(0, 0),
        Phase::Every(i, k) if k < LINE_POINTS => Phase::Every(i, k + 1),
        Phase::Every(i, _) => Phase::Every(i + 1, 0),
        _ => Phase::Done,
    }
}

/// Line `i` of the hero's held at its `k`th point (none at 0: the rest face).
fn hold_line(check: &Check, scene: &mut Scene, i: usize, k: usize, now: f32) {
    scene.speech.stop();
    if let Some(line) = check.lines.get(i).and_then(|(_, l)| l.clone())
        && k > 0
    {
        let t = line.lipsync.end() * k as f32 / (LINE_POINTS + 1) as f32;
        scene.speech.say(None, line, None, now);
        scene.speech.hold(t);
    }
}

/// The face's weights as drawn now, all its meshes'.
fn drawn_weights(scene: &Scene) -> Vec<f32> {
    let face = scene.stage.actor.as_ref().and_then(|a| a.face.as_ref());
    face.into_iter()
        .flat_map(|f| f.nodes())
        .filter_map(|(node, _)| scene.weights.get(node).ok())
        .flat_map(|w| w.weights().iter().copied())
        .collect()
}

/// Keeps the rest face of a line (point 0), or how far the line moves it at point `k`.
fn measure_line(check: &mut Check, scene: &Scene, k: usize) {
    let drawn = drawn_weights(scene);
    if k == 0 {
        check.line_rest = drawn;
        check.moved.push(0.0);
        return;
    }
    let off = drawn
        .iter()
        .zip(&check.line_rest)
        .map(|(w, r)| (w - r).abs())
        .fold(0.0, f32::max);
    if let Some(most) = check.moved.last_mut() {
        *most = most.max(off);
    }
}

/// Counts a frame waited; whether the change is drawn by now.
fn settle(check: &mut Check) -> bool {
    check.waited += 1;
    check.waited > SETTLE_FRAMES
}

fn next(check: &mut Check, phase: Phase, now: f32) {
    check.phase = phase;
    check.began = now;
    check.waited = 0;
}

/// Holds the stage still: its clips paused where they are, its idle expression at its time now.
fn hold(scene: &mut Scene, now: f32) {
    let Some(actor) = scene.stage.actor.as_mut() else {
        return;
    };
    for e in scene.tree.iter_descendants(actor.pivot) {
        if let Ok(mut player) = scene.players.get_mut(e) {
            player.pause_all();
        }
    }
    if let Some(face) = actor.face.as_mut() {
        face.frozen = Some(now - face.began);
    }
}

#[derive(Clone, Copy)]
enum Shot {
    Sequence,
    Rest,
    Along,
    RestAgain,
}

/// Reads the portrait's image back, saves it as `name` (when given) and keeps it for the check.
fn read(commands: &mut Commands, stage: &Stage, hero: &str, name: Option<String>, shot: Shot) {
    let hero = hero.to_owned();
    commands
        .spawn(Readback::texture(stage.live().clone()))
        .observe(
            move |done: On<ReadbackComplete>,
                  args: Res<crate::Args>,
                  mut check: ResMut<Check>,
                  mut commands: Commands| {
                commands.entity(done.entity).despawn();
                check.reading = false;
                let pixels = unpad(&done.data);
                if let Some(name) = &name {
                    let path = args
                        .screenshot_dir
                        .join(format!("{}{hero}-{name}.png", args.screenshot_prefix));
                    save(&pixels, &path);
                }
                match shot {
                    Shot::Sequence => {}
                    Shot::Rest => check.rest = Some(pixels),
                    Shot::Along => check.along.push(pixels),
                    Shot::RestAgain => check.rest_again = Some(pixels),
                }
            },
        );
}

/// The portrait's size in pixels.
fn size() -> (usize, usize) {
    (
        (crate::hud::PORTRAIT.0 * 2.0) as usize,
        (crate::hud::PORTRAIT.1 * 2.0) as usize,
    )
}

/// The read-back's BGRA pixels without the rows' padding.
fn unpad(data: &[u8]) -> Vec<u8> {
    let (w, h) = size();
    let stride = data.len() / h.max(1);
    (0..h)
        .flat_map(|row| data.get(row * stride..row * stride + w * 4).unwrap_or(&[]))
        .copied()
        .collect()
}

fn save(bgra: &[u8], path: &std::path::Path) {
    let (w, h) = size();
    let image = Image::new(
        Extent3d {
            width: w as u32,
            height: h as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bgra.to_vec(),
        TextureFormat::Bgra8UnormSrgb,
        bevy::asset::RenderAssetUsages::MAIN_WORLD,
    );
    let saved = image
        .try_into_dynamic()
        .map_err(|e| e.to_string())
        .and_then(|d| d.to_rgba8().save(path).map_err(|e| e.to_string()));
    if let Err(why) = saved {
        warn!("{}: not saved: {why}", path.display());
    }
}

/// Pixels of two frames that differ by more than [`CHANGED`] in a channel.
fn changed(a: &[u8], b: &[u8]) -> usize {
    a.as_chunks::<4>()
        .0
        .iter()
        .zip(b.as_chunks::<4>().0)
        .filter(|(p, q)| {
            p.iter()
                .zip(q.iter())
                .any(|(x, y)| x.abs_diff(*y) > CHANGED)
        })
        .count()
}

/// Says a verdict on the standard output (where `visual_check.sh` reads it) and in the log
/// (a browser's console).
fn tell(line: String) {
    println!("{line}");
    info!("{line}");
}

/// Prints the check's measures and whether it passed.
fn verdict(check: &Check) -> bool {
    let (Some(rest), Some(again)) = (&check.rest, &check.rest_again) else {
        tell(format!(
            "visual check: FAIL talking portrait {}: no frames read",
            check.hero
        ));
        return false;
    };
    let still = changed(rest, again);
    let moved: Vec<usize> = check.along.iter().map(|f| changed(rest, f)).collect();
    let steps: Vec<usize> = check
        .along
        .windows(2)
        .map(|w| changed(&w[0], &w[1]))
        .collect();
    let most = moved.iter().copied().max().unwrap_or(0);
    let moving = steps.iter().filter(|&&s| s >= MIN_STEP).count();
    let still_lines: Vec<&str> = check
        .lines
        .iter()
        .zip(check.moved.iter().chain(std::iter::repeat(&0.0)))
        .filter(|((_, line), moved)| line.is_none() || **moved < MIN_LINE_MOVE)
        .map(|((file, _), _)| file.as_str())
        .collect();
    let least = check.moved.iter().copied().fold(f32::INFINITY, f32::min);
    let pass = still == 0
        && most >= MIN_MOVED
        && moving as f32 >= MOVING_SHARE * steps.len() as f32
        && !steps.is_empty()
        && !check.lines.is_empty()
        && still_lines.is_empty();
    tell(format!(
        "visual check: {} talking portrait {}: rest face drawn twice differs on {still} px (0), \
         the line moves {most} px at most against the rest face (>= {MIN_MOVED}; {moved:?}), \
         {moving} of {} steps along it move >= {MIN_STEP} px ({steps:?}); its {} move and attack \
         lines each move a morph weight >= {MIN_LINE_MOVE} (the least {least:.2}; without a lip \
         sync or still: {still_lines:?})",
        if pass { "PASS" } else { "FAIL" },
        check.hero,
        steps.len(),
        check.lines.len(),
    ));
    pass
}
