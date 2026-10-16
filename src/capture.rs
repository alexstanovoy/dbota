//! Screenshots for checking the picture: of a replay at chosen ticks, or of
//! any run after a number of frames, then the app exits.

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};
use bota_client_core::Source;

use crate::Args;
use crate::session::Session;

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
    /// Frames left to settle after the camera moved for a shot.
    settle: u32,
    /// Frames waited at the wanted tick for the fog to ease in.
    settled: u32,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Shots>().add_systems(Last, shoot);
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
    mut cam: Single<&mut crate::camera::RtsCamera>,
    photo: Res<crate::minimap::PhotoTaken>,
) {
    if shots.waiting {
        return;
    }
    shots.frames += 1;
    let core = &mut session.0;
    let tick = core.view().map(|v| v.tick);
    let path = if let Some(&next) = args.screenshot_at.get(shots.taken) {
        if let Source::Replay(player) = &core.source
            && let Some(err) = player.error()
        {
            error!("the replay cannot be read: {err}");
            exit.write(AppExit::error());
            return;
        }
        let Some(tick) = tick else {
            return;
        };
        if let Source::Replay(player) = &mut core.source {
            if player.finished() || player.error().is_some() {
                // A visual check that never reached its tick checked nothing.
                if args.visual_check {
                    println!("visual check: FAIL the replay ended before tick {next}");
                    exit.write(AppExit::error());
                } else {
                    exit.write(AppExit::Success);
                }
                return;
            }
            player.speed = 1.0;
        }
        // Seek: far from the wanted tick the replay is read and played
        // through in this frame, as fast as the reader goes, nothing drawn
        // between; the last ticks before it play at their own pace, so
        // motion and effects have settled when the shot is taken.
        if tick + SETTLE_TICKS < next {
            let began = std::time::Instant::now();
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
            return;
        }
        if tick < next {
            return;
        }
        // Not before the scene is whole: the minimap photographed and the
        // fog eased in (a seek can reach an early tick in a second).
        if !photo.0 {
            return;
        }
        if shots.settled < 45 {
            shots.settled += 1;
            return;
        }
        // Several places to look at: the camera goes to this shot's and the
        // frame settles a few frames before it is taken.
        let k = shots.taken * 2;
        if args.look_at.len() > 2
            && let (Some(&x), Some(&y)) = (args.look_at.get(k), args.look_at.get(k + 1))
        {
            if Vec2::new(cam.focus.x, cam.focus.z).distance(Vec2::new(x, -y)) > 1.0 {
                cam.focus.x = x;
                cam.focus.z = -y;
                shots.settle = 20;
                return;
            }
            if shots.settle > 0 {
                shots.settle -= 1;
                return;
            }
        }
        args.screenshot_dir
            .join(format!("{}{next:06}.png", args.screenshot_prefix))
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
    let fps = diagnostics
        .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|d| d.average());
    info!("frame time over the last frames: {fps:.2?} ms");
    let last = shots.taken >= args.screenshot_at.len().max(1);
    let checking = args.visual_check;
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
