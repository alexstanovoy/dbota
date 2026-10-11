//! Debug: with `DBOTA_FRAMES` set, frame times go to the log: every ten
//! seconds their median, 95th and 99th percentiles and worst, and every
//! frame over 50 ms on its own, to find what makes the frame rate swing.

use bevy::prelude::*;

#[derive(Default)]
struct Window {
    times: Vec<f32>,
    since: f32,
    all: Vec<f32>,
    /// The process's CPU time at the window's start, in clock ticks.
    cpu: Option<u64>,
}

/// The CPU time all of this process's threads have used, in clock ticks
/// (hundredths of a second): what a frame costs whatever else runs.
fn cpu_ticks() -> Option<u64> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    let rest = stat.rsplit_once(')')?.1;
    let fields: Vec<&str> = rest.split_whitespace().collect();
    Some(fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?)
}

pub fn plugin(app: &mut App) {
    if std::env::var_os("DBOTA_FRAMES").is_some() {
        // GPU and CPU time of every render pass, logged with the rest of
        // the diagnostics every ten seconds.
        app.add_plugins((
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
            bevy::diagnostic::LogDiagnosticsPlugin {
                wait_duration: std::time::Duration::from_secs(10),
                ..default()
            },
        ))
        .add_systems(Last, record);
    }
}

fn record(time: Res<Real>, mut window: Local<Window>) {
    let dt = time.delta_secs() * 1000.0;
    if dt <= 0.0 {
        return;
    }
    window.times.push(dt);
    window.all.push(dt);
    window.since += dt / 1000.0;
    if dt > 50.0 {
        info!(
            "frames: slow frame {dt:.1} ms at {:.2} s",
            crate::since_start()
        );
    }
    if window.cpu.is_none() {
        window.cpu = cpu_ticks();
    }
    if window.since >= 10.0 {
        let frames = window.times.len();
        let stats = stats(&mut window.times);
        let cpu = match (window.cpu, cpu_ticks()) {
            (Some(a), Some(b)) => format!(
                ", CPU {:.1} ms a frame",
                (b - a) as f32 * 10.0 / frames as f32
            ),
            _ => String::new(),
        };
        window.cpu = cpu_ticks();
        info!("frames: last 10 s: {stats}{cpu}");
        let all = stats_of(&window.all);
        info!("frames: whole run: {all}");
        window.times.clear();
        window.since = 0.0;
    }
}

fn stats_of(times: &[f32]) -> String {
    let mut sorted = times.to_vec();
    stats(&mut sorted)
}

fn stats(times: &mut [f32]) -> String {
    times.sort_by(f32::total_cmp);
    let at = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
    format!(
        "{} frames, p50 {:.1} ms, p95 {:.1} ms, p99 {:.1} ms, max {:.1} ms",
        times.len(),
        at(0.5),
        at(0.95),
        at(0.99),
        at(1.0)
    )
}

/// The real clock, whatever the virtual time's cap on a long frame.
type Real = Time<bevy::time::Real>;
