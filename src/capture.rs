//! `--frames N [--screenshot PATH]`: after N frames print frame statistics, optionally
//! save a PNG of the primary window, then exit.

use std::time::Instant;

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

use crate::Args;

/// Frames skipped before timing starts (shader compilation, asset loads).
const WARMUP_FRAMES: u32 = 60;

#[derive(Default)]
struct FrameStats {
    frame: u32,
    timing_start: Option<Instant>,
    done: bool,
}

pub fn plugin(app: &mut App) {
    app.add_systems(
        Last,
        count_frames.run_if(|args: Res<Args>| args.frames.is_some()),
    );
}

fn count_frames(
    mut commands: Commands,
    args: Res<Args>,
    diagnostics: Res<DiagnosticsStore>,
    mut stats: Local<FrameStats>,
    mut exit: MessageWriter<AppExit>,
) {
    let target = args.frames.expect("run_if checked --frames");
    if stats.done {
        return;
    }
    stats.frame += 1;
    if stats.frame == WARMUP_FRAMES.min(target) {
        stats.timing_start = Some(Instant::now());
    }
    if stats.frame < target {
        return;
    }
    stats.done = true;
    let timed = stats.frame.saturating_sub(WARMUP_FRAMES.min(target));
    if let Some(start) = stats.timing_start
        && timed > 0
    {
        let ms = start.elapsed().as_secs_f64() * 1e3 / f64::from(timed);
        info!(
            "frames {timed} after warmup: mean {ms:.3} ms/frame = {:.1} fps",
            1e3 / ms
        );
    }
    let avg = |path| diagnostics.get(path).and_then(|d| d.average());
    info!(
        "FrameTimeDiagnostics (last {} frames): fps={:.1?} frame_time_ms={:.3?}",
        FrameTimeDiagnosticsPlugin::default().max_history_length,
        avg(&FrameTimeDiagnosticsPlugin::FPS),
        avg(&FrameTimeDiagnosticsPlugin::FRAME_TIME),
    );
    match &args.screenshot {
        Some(path) => {
            let mut save = save_to_disk(path.clone());
            // The capture completes a frame or two later; exit only once the file is written.
            commands.spawn(Screenshot::primary_window()).observe(
                move |captured: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    info!(
                        "screenshot {}x{} captured",
                        captured.image.width(),
                        captured.image.height()
                    );
                    save(captured);
                    exit.write(AppExit::Success);
                },
            );
        }
        None => {
            exit.write(AppExit::Success);
        }
    }
}
