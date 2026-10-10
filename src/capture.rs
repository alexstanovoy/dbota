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
/// The replay speed used to reach a wanted tick.
const FAST: f32 = 16.0;

/// The screenshots still to take.
#[derive(Resource, Default)]
struct Shots {
    taken: usize,
    frames: u32,
    waiting: bool,
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
) {
    if shots.waiting {
        return;
    }
    shots.frames += 1;
    let core = &mut session.0;
    let tick = core.view.as_ref().map(|v| v.tick);
    let path = if let Some(&next) = args.screenshot_at.get(shots.taken) {
        let Some(tick) = tick else {
            return;
        };
        if let Source::Replay(player) = &mut core.source {
            if player.finished() || player.error().is_some() {
                exit.write(AppExit::Success);
                return;
            }
            player.speed = if tick + SETTLE_TICKS < next {
                FAST
            } else {
                1.0
            };
        }
        if tick < next {
            return;
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
    if let Some(dir) = path.parent()
        && let Err(err) = std::fs::create_dir_all(dir)
    {
        error!("screenshot folder {}: {err}", dir.display());
    }
    let mut save = save_to_disk(path.clone());
    commands.spawn(Screenshot::primary_window()).observe(
        move |captured: On<ScreenshotCaptured>,
              mut shots: ResMut<Shots>,
              mut exit: MessageWriter<AppExit>| {
            save(captured);
            info!("saved {}", path.display());
            shots.waiting = false;
            if last {
                exit.write(AppExit::Success);
            }
        },
    );
}
