//! Debug: with `DBOTA_FRAMES` set, frame times go to the log: every ten
//! seconds their median, 95th and 99th percentiles and worst, and every
//! frame over 50 ms on its own, to find what makes the frame rate swing.
//! With `--bench`, one stretch of frames is timed once the scene is whole
//! and printed with the startup timings and the memory taken, then the app
//! exits: what `tools/bench.sh` reads.

use bevy::prelude::*;

/// Frames `--bench` keeps at most: a minute at well over 10 000 fps.
const BENCH_FRAMES: usize = 1 << 20;

/// What `--bench` has seen so far.
#[derive(Default)]
struct Bench {
    /// Seconds from the start to the first frame.
    first_frame: Option<f32>,
    /// Seconds from the start to the scene being whole.
    ready: Option<f32>,
    /// Seconds from the start the timing began at, and the process's CPU
    /// time then.
    began: Option<(f32, Option<u64>)>,
    times: Vec<f32>,
}

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
    app.add_systems(Last, bench);
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Last, browser_memory);
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

/// The scene is whole: the minimap photographed (so the ground and the
/// forest are in), every unit model loaded, and a `--seek` tick reached.
fn whole(
    args: &crate::Args,
    session: &crate::session::Session,
    photo: &crate::minimap::PhotoTaken,
    bank: &crate::units::ModelBank,
) -> bool {
    let tick = session.0.view().map(|v| v.tick);
    photo.0
        && bank.0.iter().all(|m| m.loaded && m.graph.is_some())
        && args.seek.is_none_or(|seek| tick.is_some_and(|t| t >= seek))
}

/// Debug `--bench`: times the frames of a stretch of play once the scene is
/// whole, prints them on one line and exits.
fn bench(
    args: Res<crate::Args>,
    time: Res<Real>,
    session: NonSend<crate::session::Session>,
    photo: Res<crate::minimap::PhotoTaken>,
    bank: Res<crate::units::ModelBank>,
    units: Res<crate::units::UnitIndex>,
    entities: Query<()>,
    assets: (Res<Assets<Image>>, Res<Assets<Mesh>>),
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    mut state: Local<Bench>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(seconds) = args.bench else {
        return;
    };
    let now = crate::since_start();
    state.first_frame.get_or_insert(now);
    if state.ready.is_none() {
        if whole(&args, &session, &photo, &bank) {
            state.ready = Some(now);
        }
        return;
    }
    let ready = state.ready.unwrap_or(now);
    let Some((began, cpu)) = state.began else {
        if now - ready >= args.bench_warmup {
            state.began = Some((now, cpu_ticks()));
        }
        return;
    };
    let dt = time.delta_secs() * 1000.0;
    if dt > 0.0 && state.times.len() < BENCH_FRAMES {
        state.times.push(dt);
    }
    let wall = now - began;
    if wall < seconds || state.times.is_empty() {
        return;
    }
    let cpu = match (cpu, cpu_ticks()) {
        (Some(a), Some(b)) => (b - a) as f32 / wall,
        _ => f32::NAN,
    };
    let times = &mut state.times;
    let mean = times.iter().sum::<f32>() / times.len() as f32;
    times.sort_by(f32::total_cmp);
    let at = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
    let (rss, peak, anon) = memory_mb();
    let line = format!(
        "bench: frames={} mean_ms={mean:.2} p50_ms={:.2} p95_ms={:.2} p99_ms={:.2} max_ms={:.2} cpu_pct={cpu:.0} rss_mb={rss} peak_rss_mb={peak} anon_mb={anon} first_frame_s={:.2} ready_s={ready:.2} units={} entities={} images_mb={} meshes_mb={}",
        times.len(),
        at(0.5),
        at(0.95),
        at(0.99),
        at(1.0),
        state.first_frame.unwrap_or(f32::NAN),
        units.0.len(),
        entities.iter().count(),
        assets
            .0
            .iter()
            .map(|(_, i)| i.data.as_ref().map_or(0, Vec::len))
            .sum::<usize>()
            >> 20,
        assets
            .1
            .iter()
            .filter_map(|(_, m)| m.try_attributes().ok())
            .flat_map(|attributes| attributes.map(|(_, values)| values.get_bytes().len()))
            .sum::<usize>()
            >> 20,
    );
    println!("{line}");
    // A browser has no stdout: the page reads the console.
    #[cfg(target_arch = "wasm32")]
    info!("{line}");
    if let Some(device) = device {
        gpu_report(&device);
    }
    exit.write(AppExit::Success);
}

/// The GPU memory the allocator holds, by what it was allocated for, on
/// stderr: `vram:` lines, largest first.
fn gpu_report(device: &bevy::render::renderer::RenderDevice) {
    let Some(report) = device.wgpu_device().generate_allocator_report() else {
        eprintln!("vram: no allocator report on this backend");
        return;
    };
    let mut by_name: std::collections::HashMap<&str, (u64, usize)> = Default::default();
    for a in &report.allocations {
        let name = a.name.split(['(', '#']).next().unwrap_or("").trim();
        let entry = by_name.entry(name).or_default();
        entry.0 += a.size;
        entry.1 += 1;
    }
    let mut rows: Vec<_> = by_name.into_iter().collect();
    rows.sort_by_key(|(_, (bytes, _))| std::cmp::Reverse(*bytes));
    eprintln!(
        "vram: allocated {} MiB, reserved {} MiB in {} blocks",
        report.total_allocated_bytes >> 20,
        report.total_reserved_bytes >> 20,
        report.blocks.len()
    );
    for (name, (bytes, count)) in rows.into_iter().take(16) {
        eprintln!(
            "vram: {:7.1} MiB  {count:5}x  {name}",
            bytes as f64 / 1048576.0
        );
    }
}

/// In a browser: the client's memory every five seconds, on the console. A
/// wasm memory only grows, so this is its high-water mark.
#[cfg(target_arch = "wasm32")]
fn browser_memory(time: Res<Real>, mut next: Local<f32>) {
    let now = time.elapsed_secs();
    if now < *next {
        return;
    }
    *next = now + 5.0;
    let bytes = core::arch::wasm32::memory_size(0) * 65536;
    info!("memory: {} MiB at {now:.0} s", bytes >> 20);
}

/// The process's resident memory now and at its peak, and how much of it
/// is anonymous (the heap, and the driver's mappings), in MiB (0 where
/// `/proc` cannot tell).
fn memory_mb() -> (u64, u64, u64) {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let field = |name: &str| {
        status
            .lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
            .map_or(0, |kb| kb / 1024)
    };
    (field("VmRSS:"), field("VmHWM:"), field("RssAnon:"))
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
