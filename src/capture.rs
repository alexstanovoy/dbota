//! Screenshots for checking the picture: of a replay at chosen ticks, or of
//! any run after a number of frames, then the app exits.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use bota_client_core::Source;

use crate::Args;
use crate::session::Session;

/// Seconds a replay is held at a wanted tick before the shot: five times the
/// fog's easing time.
const HOLD_SECONDS: f32 = 1.0;

/// Ticks before a wanted tick a replay slows back to its own pace.
const SETTLE_TICKS: u32 = 90;

/// The screenshots still to take.
#[derive(Resource, Default)]
struct Shots {
    taken: usize,
    frames: u32,
    waiting: bool,
    /// Whether a visual check has failed.
    failed: bool,
    /// Seconds the replay has been held at the wanted tick, for the fog to
    /// ease in.
    held: f32,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Shots>()
        .add_systems(Last, shoot)
        .add_systems(PreUpdate, seek.after(crate::session::pump));
}

/// Reads a replay through towards `next`, as fast as the reader goes and
/// nothing drawn between, for at most a quarter of a second a frame; stops
/// [`SETTLE_TICKS`] short, so the last ticks play at their own pace and
/// motion and effects have settled there. Whether it is still on its way.
fn fast_forward(core: &mut bota_client_core::App, next: u32) -> bool {
    if core.view().is_none_or(|v| v.tick + SETTLE_TICKS >= next) {
        return false;
    }
    let began = bevy::platform::time::Instant::now();
    while core.view().is_some_and(|v| v.tick + SETTLE_TICKS < next)
        && began.elapsed() < std::time::Duration::from_millis(250)
    {
        let before = core.view().map(|v| v.tick);
        core.pump(1.0);
        core.cues.clear();
        core.floaters.clear();
        if core.view().map(|v| v.tick) == before {
            break;
        }
    }
    true
}

/// Debug: takes a replay to `--seek`'s tick at the start.
fn seek(args: Res<Args>, mut session: NonSendMut<Session>, mut done: Local<bool>) {
    let Some(next) = args.seek.filter(|_| !*done) else {
        return;
    };
    let core = &mut session.0;
    if let Source::Replay(player) = &mut core.source
        && (player.finished() || player.error().is_some())
    {
        *done = true;
        return;
    }
    if core.view().is_some() && !fast_forward(core, next) {
        *done = true;
    }
}

/// Steers a replay towards the next wanted tick and saves the screen there;
/// or saves it after `--frames` frames. Exits once the last one is written.
fn shoot(
    mut commands: Commands,
    args: Res<Args>,
    mut shots: ResMut<Shots>,
    mut session: NonSendMut<Session>,
    mut exit: MessageWriter<AppExit>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    photo: Res<crate::minimap::PhotoTaken>,
    time: Res<Time<bevy::time::Real>>,
) {
    if shots.waiting {
        return;
    }
    shots.frames += 1;
    let core = &mut session.0;
    let path = if let Some(&next) = args.screenshot_at.get(shots.taken) {
        let Some(path) = replay_shot(
            next,
            &args,
            &mut shots,
            core,
            &mut exit,
            photo.0,
            time.delta_secs(),
        ) else {
            return;
        };
        path
    } else if let (Some(frames), Some(path)) = (args.frames, &args.screenshot) {
        if shots.frames < frames || shots.taken > 0 {
            return;
        }
        path.clone()
    } else {
        return;
    };
    shots.taken += 1;
    shots.waiting = true;
    shots.held = 0.0;
    // The frame being drawn is the one saved; the replay goes on after it.
    if let Source::Replay(player) = &mut session.0.source {
        player.paused = false;
    }
    let fps = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.average());
    info!("frame time over the last frames: {fps:.2?} ms");
    let last = shots.taken >= args.screenshot_at.len().max(1);
    save_shot(&mut commands, path, last, args.visual_check);
}

/// Steers a replay towards the shot at tick `next`: its path once the
/// replay is held there and the scene whole.
fn replay_shot(
    next: u32,
    args: &Args,
    shots: &mut Shots,
    core: &mut bota_client_core::App,
    exit: &mut MessageWriter<AppExit>,
    photo_taken: bool,
    dt: f32,
) -> Option<std::path::PathBuf> {
    if let Source::Replay(player) = &core.source
        && let Some(err) = player.error()
    {
        error!("the replay cannot be read: {err}");
        exit.write(AppExit::error());
        return None;
    }
    let tick = core.view()?.tick;
    if let Source::Replay(player) = &mut core.source {
        if player.finished() || player.error().is_some() {
            // A visual check that never reached its tick checked nothing.
            if args.visual_check {
                println!("visual check: FAIL the replay ended before tick {next}");
                exit.write(AppExit::error());
            } else {
                exit.write(AppExit::Success);
            }
            return None;
        }
        player.speed = 1.0;
    }
    if fast_forward(core, next) {
        return None;
    }
    if tick < next {
        return None;
    }
    // Not before the scene is whole: the minimap photographed and the
    // fog eased in (a seek can reach an early tick in a second).
    if !photo_taken {
        return None;
    }
    // The replay is held at the wanted tick while the fog eases in for a
    // second, whatever the frame rate: a count of frames let a fast
    // machine shoot a later tick through a fog still easing, and the
    // border measured wider or narrower from run to run.
    if let Source::Replay(player) = &mut core.source {
        player.paused = true;
    }
    if shots.held < HOLD_SECONDS {
        shots.held += dt;
        return None;
    }
    Some(
        args.screenshot_dir
            .join(format!("{}{next:06}.png", args.screenshot_prefix)),
    )
}

/// Saves the screen to `path` once drawn, measured against the visual
/// checks when `checking`; exits after the `last`.
fn save_shot(commands: &mut Commands, path: std::path::PathBuf, last: bool, checking: bool) {
    if let Some(dir) = path.parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        error!("screenshot folder {}: {err}", dir.display());
    }
    let mut save = save_to_disk(path.clone());
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>,
              mut shots: ResMut<Shots>,
              mut exit: MessageWriter<AppExit>,
              checks: crate::visual::Checks,
              mut failed: Local<bool>| {
            if checking && !crate::visual::run(&captured.image, &checks) {
                *failed = true;
            }
            save(captured);
            info!("saved {}", path.display());
            shots.waiting = false;
            if *failed {
                shots.failed = true;
            }
            if last {
                exit.write(if shots.failed {
                    AppExit::error()
                } else {
                    AppExit::Success
                });
            }
        },
    );
}
