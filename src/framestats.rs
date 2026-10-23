//! Debug: with `--bench`, one stretch of frames is timed once the scene is whole
//! and printed with the startup timings and the memory taken, then the app
//! exits: what `tools/bench.sh` reads.

use bevy::prelude::*;

/// Frames `--bench` keeps at most: a minute at well over 10 000 fps.
const MAX_FRAMES: usize = 1 << 20;

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

/// MiB of image or mesh data still held on the CPU side.
fn cpu_side_mb<A: CpuBytes>(assets: &Assets<A>) -> usize {
    assets.iter().map(|(_, a)| a.cpu_bytes()).sum::<usize>() >> 20
}

/// The bytes of an asset kept in main memory.
trait CpuBytes: Asset {
    fn cpu_bytes(&self) -> usize;
}

impl CpuBytes for Image {
    fn cpu_bytes(&self) -> usize {
        self.data.as_ref().map_or(0, Vec::len)
    }
}

impl CpuBytes for Mesh {
    fn cpu_bytes(&self) -> usize {
        self.try_attributes()
            .map(|attributes| attributes.map(|(_, values)| values.get_bytes().len()).sum())
            .unwrap_or(0)
    }
}

impl Bench {
    /// Notes a frame at `now`: the first, the one the scene is `whole` at,
    /// the one the timing begins at after `warmup` seconds. True once the
    /// timing has begun.
    fn timing(
        &mut self,
        now: f32,
        whole: impl FnOnce() -> bool,
        warmup: f32,
        threads: Option<&ThreadTimes>,
    ) -> bool {
        self.first_frame.get_or_insert(now);
        let Some(ready) = self.ready else {
            if whole() {
                self.ready = Some(now);
            }
            return false;
        };
        if self.began.is_some() {
            return true;
        }
        if now - ready >= warmup {
            self.began = Some((now, cpu_ticks()));
            if let Some(threads) = threads {
                threads.restart();
            }
        }
        false
    }
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
    if app.world().resource::<crate::Args>().bench.is_some() {
        threads(app);
    }
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Last, browser_memory);
}

/// Milliseconds each frame's work took on the main thread (`First` to
/// `Last`) and on the render thread (the `Render` schedule, the wait for the
/// next swapchain image included), shared by both worlds.
#[derive(Resource, Clone, Default)]
struct ThreadTimes(std::sync::Arc<std::sync::Mutex<(Vec<f32>, Vec<f32>)>>);

/// Times both threads' work per frame, for `--bench`.
fn threads(app: &mut App) {
    use bevy::render::{Render, RenderApp, RenderSystems};
    let times = ThreadTimes::default();
    app.insert_resource(times.clone())
        .add_systems(First, mark.before(bench))
        .add_systems(Last, main_done.before(bench));
    if let Some(render) = app.get_sub_app_mut(RenderApp) {
        render
            .insert_resource(times)
            .add_systems(Render, mark.before(RenderSystems::ExtractCommands))
            .add_systems(Render, render_done.after(RenderSystems::PostCleanup));
    }
}

impl ThreadTimes {
    /// Forgets the frames timed so far.
    fn restart(&self) {
        if let Ok(mut all) = self.0.lock() {
            all.0.clear();
            all.1.clear();
        }
    }

    /// Both threads' milliseconds a frame, as `bench:` fields.
    fn summary(&self) -> String {
        self.0.lock().map_or_else(
            |_| String::new(),
            |mut all| {
                let all = &mut *all;
                spread("main", &mut all.0) + &spread("render", &mut all.1)
            },
        )
    }
}

/// When this world's frame began.
#[derive(Resource)]
struct Began(bevy::platform::time::Instant);

fn mark(mut commands: Commands) {
    commands.insert_resource(Began(bevy::platform::time::Instant::now()));
}

fn main_done(began: Option<Res<Began>>, times: Res<ThreadTimes>) {
    record(began, &times, |t| &mut t.0);
}

fn render_done(began: Option<Res<Began>>, times: Res<ThreadTimes>) {
    record(began, &times, |t| &mut t.1);
}

fn record(
    began: Option<Res<Began>>,
    times: &ThreadTimes,
    which: impl FnOnce(&mut (Vec<f32>, Vec<f32>)) -> &mut Vec<f32>,
) {
    let (Some(began), Ok(mut all)) = (began, times.0.lock()) else {
        return;
    };
    let list = which(&mut all);
    if list.len() < MAX_FRAMES {
        list.push(began.0.elapsed().as_secs_f32() * 1000.0);
    }
}

/// `name_mean_ms=… name_p95_ms=…` of a list of milliseconds.
fn spread(name: &str, list: &mut [f32]) -> String {
    if list.is_empty() {
        return String::new();
    }
    let mean = list.iter().sum::<f32>() / list.len() as f32;
    list.sort_by(f32::total_cmp);
    let p95 = list[((list.len() - 1) as f32 * 0.95) as usize];
    format!(" {name}_mean_ms={mean:.2} {name}_p95_ms={p95:.2}")
}

/// Each render pass's CPU and GPU milliseconds, averaged over Bevy's recent
/// frames, on stderr as `pass:` lines (with `--bench-gpu`).
fn pass_report(store: &bevy::diagnostic::DiagnosticsStore) {
    let mut rows: Vec<(String, f64)> = store
        .iter()
        .filter(|d| d.path().as_str().starts_with("render/"))
        .filter_map(|d| Some((d.path().as_str().to_owned(), d.average()?)))
        .collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (path, ms) in rows {
        eprintln!("pass: {ms:8.3} {path}");
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
        && bank.0.iter().all(|m| m.ready())
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
    (threads, store): (
        Option<Res<ThreadTimes>>,
        Res<bevy::diagnostic::DiagnosticsStore>,
    ),
    mut state: Local<Bench>,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(seconds) = args.bench else {
        return;
    };
    let now = crate::since_start();
    let whole = || whole(&args, &session, &photo, &bank);
    if !state.timing(now, whole, args.bench_warmup, threads.as_deref()) {
        return;
    }
    let (Some(ready), Some((began, cpu))) = (state.ready, state.began) else {
        return;
    };
    let dt = time.delta_secs() * 1000.0;
    if dt > 0.0 && state.times.len() < MAX_FRAMES {
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
        cpu_side_mb(&assets.0),
        cpu_side_mb(&assets.1),
    ) + &threads.map_or_else(String::new, |t| t.summary());
    println!("{line}");
    pass_report(&store);
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

/// The real clock, whatever the virtual time's cap on a long frame.
type Real = Time<bevy::time::Real>;
