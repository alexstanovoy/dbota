//! The visible contribution of the objectives' ground effects through the whole production
//! renderer, by day and by night: for each state the same frame is captured with the effect on
//! and off, and the pixels it changes must have the SHAPE the authored data gives them (a ring
//! at the authored radius, a glow inside the objective's footprint, nothing where the game
//! shows nothing), a visible thickness, and a coverage bounded from above as well as below.
use super::{
    creep_walk::ControlledFrames,
    harness::{Client, Patience, Server},
};
use bevy::{
    camera::{ImageRenderTarget, RenderTarget},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
    time::TimeUpdateStrategy,
};
use bota_proto::{Cheat, HeroId, UnitKind};
use std::{f32::consts::TAU, sync::Arc, time::Duration};

const SIZE: UVec2 = UVec2::new(1600, 900);
/// The least change of a colour channel that counts as a changed pixel.
const PIXEL_CONTRAST: u8 = 8;
/// A ring's drawn band must be at least this many pixels thick (a range indicator's stroke is
/// two) and change a colour channel by at least [`MIN_RING_PEAK`] where it is brightest.
const MIN_RING_THICKNESS: f32 = 2.0;
const MIN_RING_PEAK: u8 = 24;
/// How far (pixels) from an authored ring the band across it is measured.
const RING_REACH: f32 = 14.0;
/// A ring point is drawn when a changed pixel lies this far from it.
const RING_HIT: f32 = 4.0;
/// The share of an authored ring's points that must be drawn: the rest hide behind the stone
/// basins, the shrine and the hero.
const RING_RECALL: f32 = 0.2;
/// The ring radii the game's data feeds the effects (the manifest's `cp1=` lines).
const LOTUS_RING: f32 = 350.0;
const SHRINE_RING: f32 = 300.0;
/// Where an ambient glow may reach beyond the authored footprint, in world units: card sizes and
/// the height of what glows project upward on the screen.
const GLOW_MARGIN: f32 = 300.0;
/// An ambient effect's pixels are bounded above: a share of its footprint, and how strongly
/// the strongest tenth of them differs (an opaque quad replaces the ground; water motion shades
/// it). The shimmer of the pools covers about 4-5% of theirs; the distortion map drawn as a colour
/// (a grid over every basin) twice that.
const WATER_MAX_COVERAGE: f32 = 0.07;
const WATER_MAX_P90: u8 = 110;
/// The same for the shrine's glow, which lights its stones and sparks up: about 12%.
const SHRINE_MAX_COVERAGE: f32 = 0.25;
const SHRINE_MAX_P90: u8 = 200;

#[derive(Resource)]
struct Surface(Handle<Image>);
#[derive(Resource, Default)]
struct Capture {
    pending: Option<Entity>,
    pixels: Option<Vec<u8>>,
}

fn prepare(app: &mut App) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    app.init_resource::<Capture>()
        .add_systems(PostStartup, surface);
}
fn surface(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut camera: Single<&mut RenderTarget, With<crate::camera::RtsCamera>>,
) {
    let mut image = Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = images.add(image);
    **camera = RenderTarget::Image(ImageRenderTarget {
        handle: handle.clone(),
        scale_factor: 1.0,
    });
    commands.insert_resource(Surface(handle));
}
fn received(event: On<ReadbackComplete>, mut result: ResMut<Capture>, mut commands: Commands) {
    if result.pending != Some(event.entity) {
        return;
    }
    let stride = RenderDevice::align_copy_bytes_per_row(SIZE.x as usize * 4);
    let data = event
        .data
        .chunks_exact(stride)
        .take(SIZE.y as usize)
        .flat_map(|row| row[..SIZE.x as usize * 4].iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(data.len(), SIZE.x as usize * SIZE.y as usize * 4);
    result.pixels = Some(data);
    result.pending = None;
    commands.entity(event.entity).despawn();
}
fn pixels(client: &mut Client) -> Vec<u8> {
    let handle = client.world().resource::<Surface>().0.clone();
    let entity = client
        .world_mut()
        .spawn(Readback::texture(handle))
        .observe(received)
        .id();
    *client.world_mut().resource_mut::<Capture>() = Capture {
        pending: Some(entity),
        pixels: None,
    };
    let mut patience = Patience::of(Duration::from_secs(60));
    while client.world().resource::<Capture>().pixels.is_none() {
        patience.pass("the offscreen objective pixels");
        client.frames(1);
    }
    client
        .world_mut()
        .resource_mut::<Capture>()
        .pixels
        .take()
        .expect("captured pixels")
}
/// Where the frames go: `OBJECTIVE_EVIDENCE_DIR` when set (a clean run's evidence is copied from
/// there by hand), else a folder of this run's own, so a mutant run never overwrites evidence.
fn evidence_directory() -> std::path::PathBuf {
    let directory = std::env::var_os("OBJECTIVE_EVIDENCE_DIR").map_or_else(
        || std::env::temp_dir().join(format!("objective_pixels_{}", std::process::id())),
        std::path::PathBuf::from,
    );
    std::fs::create_dir_all(&directory).expect("the pixel evidence directory");
    directory
}
fn save(name: &str, bytes: &[u8]) {
    let image = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: SIZE.x,
            height: SIZE.y,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    image
        .try_into_dynamic()
        .expect("RGBA capture")
        .save(evidence_directory().join(format!("{name}.png")))
        .expect("pixel evidence");
}

// ---- what a difference of two frames shows -------------------------------------------------

/// The pixels two captures of one frame differ at.
struct Diff {
    /// Per pixel, the largest change of a colour channel.
    delta: Vec<u8>,
    /// Per pixel, the signed change of red, green and blue (effect on less effect off).
    signed: Vec<[i16; 3]>,
}
impl Diff {
    fn of(on: &[u8], off: &[u8], unstable: &[usize]) -> Diff {
        let pixels = (SIZE.x * SIZE.y) as usize;
        let mut diff = Diff {
            delta: vec![0; pixels],
            signed: vec![[0; 3]; pixels],
        };
        for p in 0..pixels {
            if unstable.contains(&(p * 4)) {
                continue;
            }
            for c in 0..3 {
                let change = i16::from(on[p * 4 + c]) - i16::from(off[p * 4 + c]);
                diff.signed[p][c] = change;
                diff.delta[p] = diff.delta[p].max(change.unsigned_abs() as u8);
            }
        }
        diff
    }
    fn changed(&self, x: i32, y: i32) -> bool {
        (0..SIZE.x as i32).contains(&x)
            && (0..SIZE.y as i32).contains(&y)
            && self.delta[(y as u32 * SIZE.x + x as u32) as usize] >= PIXEL_CONTRAST
    }
    fn changed_pixels(&self) -> impl Iterator<Item = Vec2> + '_ {
        self.delta
            .iter()
            .enumerate()
            .filter(|(_, d)| **d >= PIXEL_CONTRAST)
            .map(|(p, _)| Vec2::new((p as u32 % SIZE.x) as f32, (p as u32 / SIZE.x) as f32))
    }
    fn count(&self) -> usize {
        self.changed_pixels().count()
    }
}

/// How far (pixels) either side of a ring the profile across it is taken.
const RING_PROFILE: i32 = 40;
/// A ring's band must change the pixels on it at least this many times as much as the average
/// of its profile.
const RING_PEAKEDNESS: f32 = 1.5;

/// How well the changed pixels draw one authored ring, given its radius projected to the screen.
struct RingStats {
    /// The share of the ring's points with a changed pixel within [`RING_HIT`].
    recall: f32,
    /// The median thickness, in pixels, of the band across the ring where it is drawn.
    thickness: f32,
    /// The largest change of a colour channel within [`RING_HIT`] of the ring.
    peak: u8,
    /// How far (pixels) the strongest change, averaged over the ring's points, lies across the
    /// ring from the authored radius: the band's middle sits on the radius, within the band's
    /// own thickness (a soft band's strongest point is pulled toward the glow beside it).
    centre_offset: f32,
    /// That strongest change over the average of all offsets: a band stands out from the glow
    /// around it, a blob that merely covers the ring does not.
    peakedness: f32,
}
fn ring_stats(diff: &Diff, ring: &[Vec2]) -> RingStats {
    let n = ring.len();
    let mut drawn = 0;
    let mut peak = 0;
    let mut thicknesses = Vec::new();
    let mut profile = vec![0.0f32; 2 * RING_PROFILE as usize + 1];
    for (i, p) in ring.iter().enumerate() {
        let tangent = (ring[(i + 1) % n] - ring[(i + n - 1) % n]).normalize_or_zero();
        let across = Vec2::new(-tangent.y, tangent.x);
        let strongest = |at: Vec2| {
            let mut strongest = 0u8;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (x, y) = (at.x.round() as i32 + dx, at.y.round() as i32 + dy);
                    if (0..SIZE.x as i32).contains(&x) && (0..SIZE.y as i32).contains(&y) {
                        strongest =
                            strongest.max(diff.delta[(y as u32 * SIZE.x + x as u32) as usize]);
                    }
                }
            }
            f32::from(strongest)
        };
        for (k, at) in profile.iter_mut().enumerate() {
            *at += strongest(*p + across * (k as i32 - RING_PROFILE) as f32);
        }
        let near = |reach: f32| {
            (-(reach as i32)..=reach as i32).any(|dy| {
                (-(reach as i32)..=reach as i32).any(|dx| {
                    diff.changed(p.x.round() as i32 + dx, p.y.round() as i32 + dy)
                        && Vec2::new(dx as f32, dy as f32).length() <= reach
                })
            })
        };
        if !near(RING_HIT) {
            continue;
        }
        drawn += 1;
        for dy in -(RING_HIT as i32)..=RING_HIT as i32 {
            for dx in -(RING_HIT as i32)..=RING_HIT as i32 {
                let at = (p.x.round() as i32 + dx, p.y.round() as i32 + dy);
                if (0..SIZE.x as i32).contains(&at.0) && (0..SIZE.y as i32).contains(&at.1) {
                    peak = peak.max(diff.delta[(at.1 as u32 * SIZE.x + at.0 as u32) as usize]);
                }
            }
        }
        // The band's thickness across the ring: the changed one-pixel steps on the line through
        // the point, perpendicular to the ring, a bounded reach each way.
        let steps = (-(RING_REACH as i32)..=RING_REACH as i32)
            .filter(|k| {
                let at = *p + across * *k as f32;
                diff.changed(at.x.round() as i32, at.y.round() as i32)
            })
            .count();
        thicknesses.push(steps as f32);
    }
    thicknesses.sort_by(f32::total_cmp);
    let (best, strongest) = profile
        .iter()
        .copied()
        .enumerate()
        .fold((0, 0.0f32), |a, (k, v)| if v > a.1 { (k, v) } else { a });
    let mean = profile.iter().sum::<f32>() / profile.len() as f32;
    RingStats {
        centre_offset: (best as i32 - RING_PROFILE).unsigned_abs() as f32,
        peakedness: strongest / mean.max(1e-3),
        recall: drawn as f32 / n.max(1) as f32,
        thickness: thicknesses
            .get(thicknesses.len() / 2)
            .copied()
            .unwrap_or(0.0),
        peak,
    }
}
/// The mean signed change of the changed pixels within [`RING_HIT`] of some authored ring.
fn colour_on_rings(diff: &Diff, rings: &[Vec<Vec2>]) -> [f32; 3] {
    let (mut sum, mut n) = ([0.0f32; 3], 0.0f32);
    for pixel in diff.changed_pixels() {
        if rings
            .iter()
            .flatten()
            .any(|p| p.distance(pixel) <= RING_HIT)
        {
            let s = diff.signed[(pixel.y as u32 * SIZE.x + pixel.x as u32) as usize];
            (0..3).for_each(|c| sum[c] += f32::from(s[c]));
            n += 1.0;
        }
    }
    sum.map(|v| v / n.max(1.0))
}

/// The convex hull of the points, counterclockwise.
fn hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut out: Vec<Vec2> = Vec::new();
    for pass in 0..2 {
        let start = out.len();
        for p in points.iter().copied() {
            while out.len() >= start + 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0
            {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
        if pass == 0 {
            points.reverse();
        }
    }
    out
}
fn inside(polygon: &[Vec2], p: Vec2) -> bool {
    (0..polygon.len()).all(|i| {
        let (a, b) = (polygon[i], polygon[(i + 1) % polygon.len()]);
        (b - a).perp_dot(p - a) >= 0.0
    })
}
fn area(polygon: &[Vec2]) -> f32 {
    (0..polygon.len())
        .map(|i| polygon[i].perp_dot(polygon[(i + 1) % polygon.len()]))
        .sum::<f32>()
        / 2.0
}

/// How the changed pixels sit in an ambient effect's footprint.
struct GlowStats {
    /// The share of changed pixels inside the footprint.
    inside: f32,
    /// The changed pixels' share of the footprint's area.
    coverage: f32,
    /// How much the strongest tenth of the changed pixels differ.
    p90: u8,
    /// How many separate patches of the footprint the changes make.
    patches: usize,
}
fn glow_stats(diff: &Diff, footprint: &[Vec2]) -> GlowStats {
    let changed: Vec<Vec2> = diff.changed_pixels().collect();
    let within = changed.iter().filter(|p| inside(footprint, **p)).count();
    let mut deltas: Vec<u8> = diff
        .delta
        .iter()
        .copied()
        .filter(|d| *d >= PIXEL_CONTRAST)
        .collect();
    deltas.sort_unstable();
    GlowStats {
        inside: within as f32 / changed.len().max(1) as f32,
        coverage: within as f32 / area(footprint).max(1.0),
        p90: deltas.get(deltas.len() * 9 / 10).copied().unwrap_or(0),
        patches: patches(&changed),
    }
}
/// The separate patches of changed pixels: 8-pixel cells with at least four changed pixels,
/// joined when they touch, each at least four cells.
fn patches(changed: &[Vec2]) -> usize {
    const CELL: f32 = 8.0;
    let columns = (SIZE.x as f32 / CELL).ceil() as usize;
    let rows = (SIZE.y as f32 / CELL).ceil() as usize;
    let mut counts = vec![0u16; columns * rows];
    for p in changed {
        counts[(p.y / CELL) as usize * columns + (p.x / CELL) as usize] += 1;
    }
    let mut seen = vec![false; counts.len()];
    let mut found = 0;
    for start in 0..counts.len() {
        if seen[start] || counts[start] < 4 {
            continue;
        }
        let (mut stack, mut size) = (vec![start], 0);
        seen[start] = true;
        while let Some(cell) = stack.pop() {
            size += 1;
            let (cx, cy) = ((cell % columns) as i32, (cell / columns) as i32);
            for (dx, dy) in (-1..=1).flat_map(|dx| (-1..=1).map(move |dy| (dx, dy))) {
                let (x, y) = (cx + dx, cy + dy);
                if (0..columns as i32).contains(&x) && (0..rows as i32).contains(&y) {
                    let next = y as usize * columns + x as usize;
                    if !seen[next] && counts[next] >= 4 {
                        seen[next] = true;
                        stack.push(next);
                    }
                }
            }
        }
        found += usize::from(size >= 4);
    }
    found
}

// ---- the states and what each must show ----------------------------------------------------

#[derive(Clone, Debug)]
enum Expect {
    /// The effect is not playing: no pixel differs.
    Absent,
    /// A glow within the objective's footprint (world radius around it), of at least this many
    /// separate patches (a lotus pool has a basin on each of its three sides).
    Glow {
        radius: f32,
        patches: usize,
        max_coverage: f32,
        max_p90: u8,
    },
    /// Rings of these world radii, in the authored colour (sRGB, 0..255).
    Rings { radii: Vec<f32>, colour: [f32; 3] },
}
struct Check {
    /// The logical effects hidden for the "off" frame.
    hide: &'static [&'static str],
    /// Hides only these of an effect's systems, by what their path contains (the rings of an
    /// effect that also lights its stones): empty hides the whole effect.
    only: &'static str,
    expect: Expect,
}
#[derive(Clone)]
struct Case {
    name: &'static str,
    tick: u32,
    at: bota_proto::Vec2,
    /// The share a gathering has run, 0 when none runs.
    fraction: f32,
}

fn checks(case: &Case, footprint: impl Fn(&str) -> f32) -> Vec<Check> {
    let ring = |hide: &'static [&'static str], full: f32, colour: [f32; 3]| Check {
        hide,
        only: "progress_ring",
        expect: Expect::Rings {
            radii: vec![full, full * case.fraction],
            colour,
        },
    };
    match case.name {
        "wisdom_unready" => vec![Check {
            hide: &["shrine.ambient", "shrine.active"],
            only: "",
            expect: Expect::Absent,
        }],
        "lotus_available" => vec![
            Check {
                hide: &["lotus_pool.ambient"],
                only: "",
                expect: Expect::Glow {
                    radius: footprint("lotus_pool.ambient"),
                    patches: 3,
                    max_coverage: WATER_MAX_COVERAGE,
                    max_p90: WATER_MAX_P90,
                },
            },
            Check {
                hide: &["lotus_pool.gathering"],
                only: "",
                expect: Expect::Absent,
            },
        ],
        "lotus_gathering" => vec![ring(
            &["lotus_pool.gathering"],
            LOTUS_RING,
            [9.0, 142.0, 231.0],
        )],
        "wisdom_ready" => vec![
            Check {
                hide: &["shrine.ambient"],
                only: "",
                expect: Expect::Glow {
                    radius: SHRINE_RING,
                    patches: 1,
                    max_coverage: SHRINE_MAX_COVERAGE,
                    max_p90: SHRINE_MAX_P90,
                },
            },
            Check {
                hide: &["shrine.active"],
                only: "",
                expect: Expect::Absent,
            },
        ],
        "wisdom_gathering" => vec![ring(&["shrine.active"], SHRINE_RING, [190.0, 9.0, 231.0])],
        other => panic!("no expectation for {other}"),
    }
}

/// The screen points of the circle of `radius` around `at` on the ground.
fn circle(client: &mut Client, at: Vec2, radius: f32) -> Vec<Vec2> {
    (0..256)
        .filter_map(|i| {
            let angle = i as f32 * TAU / 256.0;
            let p = at + Vec2::new(angle.cos(), angle.sin()) * radius;
            client.screen_of((p.x, p.y))
        })
        .collect()
}

fn unstable_pixels(on: &[u8], on_again: &[u8], off: &[u8], off_again: &[u8]) -> Vec<usize> {
    let unstable: Vec<usize> = (0..on.len() / 4)
        .map(|p| p * 4)
        .filter(|i| {
            (0..3).any(|c| {
                on[i + c].abs_diff(on_again[i + c]) >= PIXEL_CONTRAST
                    || off[i + c].abs_diff(off_again[i + c]) >= PIXEL_CONTRAST
            })
        })
        .collect();
    // Only a tiny, bounded control mask is excluded; it can never count as effect signal.
    assert!(
        unstable.len() <= 16,
        "held-frame raster changes at {} pixels",
        unstable.len()
    );
    unstable
}

/// The logical effects of the objectives this test compares.
const EFFECTS: [&str; 4] = [
    "lotus_pool.ambient",
    "lotus_pool.gathering",
    "shrine.ambient",
    "shrine.active",
];

fn effect(client: &Client, logical: &str) -> Arc<crate::particles::Effect> {
    client
        .world()
        .resource::<crate::particles::Library>()
        .effect(logical)
        .expect("effect")
        .clone()
}

/// Hides what a frame is compared without: whole effects, or (`only` not empty) just their
/// systems whose path contains it.
fn hide(client: &mut Client, logical: &[&str], only: &str) {
    let effects: Vec<_> = logical.iter().map(|name| effect(client, name)).collect();
    let mut playing = client
        .world_mut()
        .resource_mut::<crate::particles::Playing>();
    if only.is_empty() {
        playing.hide_effects(effects);
    } else {
        for effect in &effects {
            playing.hide_systems(effect, only);
        }
    }
    client.frames(3);
}

fn show_everything(client: &mut Client) {
    let effects: Vec<_> = EFFECTS.iter().map(|name| effect(client, name)).collect();
    let mut playing = client
        .world_mut()
        .resource_mut::<crate::particles::Playing>();
    playing.hide_effects(Vec::new());
    for effect in &effects {
        playing.hide_systems(effect, "");
    }
    client.frames(3);
}

/// The largest horizontal reach, in world units, of the snapshots an effect places particles on.
fn snapshot_reach(client: &Client, logical: &str) -> f32 {
    let effect = client
        .world()
        .resource::<crate::particles::Library>()
        .effect(logical)
        .expect("effect");
    effect
        .defs
        .iter()
        .filter_map(|def| def.system.snapshot.as_ref())
        .filter_map(|snapshot| match snapshot {
            heatel::particle::Snapshot::Points { positions, .. } => Some(positions),
            _ => None,
        })
        .flatten()
        .map(|p| Vec2::new(p[0], p[1]).length())
        .fold(0.0, f32::max)
}

fn compare(client: &mut Client, case: &Case, light: &str, failures: &mut Vec<String>) {
    let at = Vec2::new(case.at.x.to_f32(), case.at.y.to_f32());
    // Every failed property is reported, after every state has been captured.
    let mut expect = |ok: bool, what: String| {
        if !ok {
            failures.push(what);
        }
    };
    client.frames(3);
    let held_tick = client.tick();
    let held_time = client.world().resource::<Time>().elapsed();
    let reaches: Vec<(String, f32)> = ["lotus_pool.ambient"]
        .iter()
        .map(|name| (name.to_string(), snapshot_reach(client, name)))
        .collect();
    let list = checks(case, |name| {
        reaches
            .iter()
            .find(|(n, _)| n == name)
            .map_or(0.0, |(_, r)| *r)
    });
    for (index, check) in list.iter().enumerate() {
        show_everything(client);
        let on = pixels(client);
        let on_again = pixels(client);
        assert_eq!(
            client.tick(),
            held_tick,
            "the captured game frame stays fixed"
        );
        assert_eq!(client.world().resource::<Time>().elapsed(), held_time);
        hide(client, check.hide, check.only);
        let off = pixels(client);
        let off_again = pixels(client);
        show_everything(client);
        let unstable = unstable_pixels(&on, &on_again, &off, &off_again);
        let diff = Diff::of(&on, &off, &unstable);
        if index == 0 {
            save(&format!("{}_{light}_on", case.name), &on);
            save(&format!("{}_{light}_off", case.name), &off);
            let amplified: Vec<u8> = diff
                .delta
                .iter()
                .flat_map(|d| {
                    let v = d.saturating_mul(4);
                    [v, v, v, 255]
                })
                .collect();
            save(&format!("{}_{light}_diff_x4", case.name), &amplified);
        }
        let label = format!("{} ({light}), hiding {:?}", case.name, check.hide);
        match &check.expect {
            Expect::Absent => {
                println!("objective pixels {label}: absent, {} changed", diff.count());
                expect(
                    diff.count() == 0,
                    format!(
                        "{label}: nothing is drawn, yet {} pixels differ",
                        diff.count()
                    ),
                );
            }
            Expect::Glow {
                radius,
                patches,
                max_coverage,
                max_p90,
            } => {
                let polygon = hull(
                    circle(client, at, radius + GLOW_MARGIN)
                        .into_iter()
                        .collect(),
                );
                let stats = glow_stats(&diff, &polygon);
                println!(
                    "objective pixels {label}: glow inside {:.2} coverage {:.3} p90 {} patches {} changed {}",
                    stats.inside,
                    stats.coverage,
                    stats.p90,
                    stats.patches,
                    diff.count()
                );
                expect(
                    stats.inside >= 0.98,
                    format!(
                        "{label}: the glow leaves the footprint ({:.2} inside)",
                        stats.inside
                    ),
                );
                expect(
                    stats.coverage >= 0.01,
                    format!(
                        "{label}: the glow is not visible ({:.3} of the footprint)",
                        stats.coverage
                    ),
                );
                expect(
                    stats.coverage <= *max_coverage,
                    format!(
                        "{label}: the glow fills the footprint ({:.3})",
                        stats.coverage
                    ),
                );
                expect(
                    stats.p90 <= *max_p90,
                    format!("{label}: the glow is opaque (p90 change {})", stats.p90),
                );
                expect(
                    stats.patches >= *patches,
                    format!("{label}: {} patches, expected {patches}", stats.patches),
                );
            }
            Expect::Rings { radii, colour } => {
                let rings: Vec<Vec<Vec2>> = radii.iter().map(|r| circle(client, at, *r)).collect();
                let outer = hull(circle(client, at, radii[0] + GLOW_MARGIN));
                let within = glow_stats(&diff, &outer).inside;
                let mean = colour_on_rings(&diff, &rings);
                let direction = |v: [f32; 3]| {
                    v.map(|c| c / v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6))
                };
                let cosine: f32 = direction(mean)
                    .iter()
                    .zip(direction(*colour))
                    .map(|(a, b)| a * b)
                    .sum();
                println!(
                    "objective pixels {label}: rings {radii:?}, colour on them {mean:?} (cosine {cosine:.2} to {colour:?}), within the footprint {within:.2}, changed {}",
                    diff.count()
                );
                expect(
                    within >= 0.98,
                    format!("{label}: the changes leave the rings' footprint ({within:.2} inside)"),
                );
                expect(
                    cosine >= 0.8,
                    format!("{label}: the rings' colour {mean:?} is not the authored {colour:?}"),
                );
                for (ring, radius) in rings.iter().zip(radii) {
                    let stats = ring_stats(&diff, ring);
                    println!(
                        "  ring {radius}: recall {:.2} thickness {} peak {} centre offset {} peakedness {:.1}",
                        stats.recall,
                        stats.thickness,
                        stats.peak,
                        stats.centre_offset,
                        stats.peakedness
                    );
                    expect(
                        stats.recall >= RING_RECALL,
                        format!(
                            "{label}: ring {radius} is drawn at {:.2} of its points",
                            stats.recall
                        ),
                    );
                    expect(
                        stats.thickness >= MIN_RING_THICKNESS,
                        format!("{label}: ring {radius} is {} px thick", stats.thickness),
                    );
                    expect(
                        stats.centre_offset <= RING_HIT.max(stats.thickness),
                        format!(
                            "{label}: ring {radius} is drawn {} px off its radius",
                            stats.centre_offset
                        ),
                    );
                    expect(
                        stats.peakedness >= RING_PEAKEDNESS,
                        format!(
                            "{label}: ring {radius} does not stand out from its surroundings ({:.1}x)",
                            stats.peakedness
                        ),
                    );
                    expect(
                        stats.peak >= MIN_RING_PEAK,
                        format!(
                            "{label}: ring {radius} changes a channel by only {}",
                            stats.peak
                        ),
                    );
                }
            }
        }
    }
    assert_eq!(client.tick(), held_tick);
    assert_eq!(client.world().resource::<Time>().elapsed(), held_time);
}

// ---- the recorded game -----------------------------------------------------------------------

struct StopAfter(std::rc::Rc<std::cell::Cell<bool>>);
impl bota_client_core::Observer for StopAfter {
    type Error = &'static str;
    fn after_snapshot(&mut self, _: &bota_client_core::ClientSession) -> Result<(), Self::Error> {
        if self.0.get() {
            Err("states captured")
        } else {
            Ok(())
        }
    }
}

/// A match recorded through the real server with the objectives' states reached by cheats, the
/// time of day set before each: the replay file and the ticks to capture.
fn recorded_states(night: bool) -> (std::path::PathBuf, Vec<Case>) {
    use bota_client_core::{IssuedOrder, Link, join, play};
    use bota_proto::{EffectId, Order, Vec2 as Spot};
    let file = crate::test_support::scratch_path(if night {
        "objective_states_night.brp"
    } else {
        "objective_states_day.brp"
    });
    let mut server = Server::start(
        1,
        &[
            "--map",
            "0",
            "--objectives",
            "lotus_pools,shrines",
            "--replay",
            file.to_str().expect("path"),
        ],
    );
    let mut link =
        Link::connect(&server.address, Duration::from_secs(10)).expect("fixture transport");
    let seated = join(&mut link, "observer", HeroId::SHADOW_FIEND, None).expect("fixture joins");
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut observer = StopAfter(done.clone());
    let mut stage = 0;
    let mut cases = Vec::new();
    let time = Cheat::TimeOfDay { night };
    let mut setup = std::collections::VecDeque::from([
        Cheat::CreepSpawning { on: false },
        Cheat::Invulnerable { on: true },
        Cheat::StartGame,
        time,
    ]);
    let result = play(&mut link, seated, &mut observer, |session| {
        let view = session.current().expect("tick");
        let me = session.own_hero().expect("hero");
        let pool = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::LotusPool)
            .expect("pool");
        let shrine = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Shrine)
            .expect("shrine");
        assert!(
            view.tick < 30_000,
            "bounded fixture: stage {stage}, hero {:?}, lotuses {:?}",
            me.pos,
            view.pools
        );
        let teleport = |unit: &bota_proto::UnitView, offset: i32| Cheat::Teleport {
            at: unit.pos + Spot::from_ints(offset, 0),
        };
        let fraction = |unit: &bota_proto::UnitView| {
            unit.gathering
                .map_or(0.0, |g| g.progress as f32 / g.ticks.max(1) as f32)
        };
        let ready = |unit: &bota_proto::UnitView| {
            unit.effects.iter().any(|e| e.id == EffectId::SHRINE_READY)
        };
        let mut capture = |name, unit: &bota_proto::UnitView| {
            cases.push(Case {
                name,
                tick: view.tick,
                at: unit.pos,
                fraction: fraction(unit),
            });
        };
        let cheat = if let Some(cheat) = setup.pop_front() {
            Some(cheat)
        } else {
            // A stage that reached its state sets the time of day again, and the next stage
            // captures: the capture is a tick after the cheat.
            match stage {
                0 => {
                    capture("wisdom_unready", shrine);
                    stage = 1;
                    Some(teleport(pool, -500))
                }
                1 if view.pools.iter().any(|p| p.id == pool.id && p.lotuses > 0) => {
                    stage = 2;
                    Some(time)
                }
                2 => {
                    capture("lotus_available", pool);
                    stage = 3;
                    Some(teleport(pool, -200))
                }
                3 if pool.gathering.is_some_and(|g| g.progress * 2 >= g.ticks) => {
                    stage = 4;
                    Some(time)
                }
                4 => {
                    capture("lotus_gathering", pool);
                    stage = 5;
                    Some(teleport(shrine, -500))
                }
                5 if ready(shrine) => {
                    stage = 6;
                    Some(time)
                }
                6 => {
                    capture("wisdom_ready", shrine);
                    stage = 7;
                    Some(teleport(shrine, 0))
                }
                7 if shrine.gathering.is_some_and(|g| g.progress * 2 >= g.ticks) => {
                    stage = 8;
                    Some(time)
                }
                8 => {
                    capture("wisdom_gathering", shrine);
                    done.set(true);
                    None
                }
                _ => None,
            }
        };
        cheat.map(|cheat| IssuedOrder::hero(Order::Cheat { cheat }))
    });
    assert!(
        matches!(
            result,
            Err(bota_client_core::PlayError::Observer("states captured"))
        ),
        "{result:?}"
    );
    bota_client_core::Transport::send(&mut link, &bota_proto::ClientMsg::LeaveGame)
        .expect("end fixture");
    drop(link);
    server.wait();
    assert_eq!(cases.len(), 5);
    (file, cases)
}

fn seek(client: &mut Client, tick: u32) {
    let delta = tick.checked_sub(client.tick()).expect("increasing cases");
    if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
        player.advance_ticks(f64::from(delta));
    }
    let render = client
        .app
        .remove_sub_app(bevy::render::RenderApp)
        .expect("renderer");
    client.until("the recorded objective tick", |client| {
        client.tick() == tick
    });
    client.app.insert_sub_app(bevy::render::RenderApp, render);
}

/// Steps the manual clock until the light has turned to the time of day recorded.
fn settle_light(client: &mut Client, night: bool) {
    let step = Duration::from_secs_f64(1.0 / 30.0);
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(step));
    let want = if night { 1.0 } else { 0.0 };
    client.until("the light of the recorded time of day", |client| {
        (client.world().resource::<crate::daylight::Daylight>().night - want).abs() < 0.001
    });
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
}

fn contract(night: bool) {
    let light = if night { "night" } else { "day" };
    let (file, cases) = recorded_states(night);
    let mut client = Client::run_rendered(
        &[
            "--replay",
            file.to_str().expect("path"),
            "--mute",
            "--window",
            "1600x900",
            "--no-vsync",
        ],
        prepare,
    );
    if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
    }
    client.until("first replay snapshot", |client| {
        client.core().view().is_some()
    });
    let source = std::mem::replace(
        &mut client.core_mut().source,
        crate::main_menu::idle().0.source,
    );
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    client.until("the loaded scene", |client| {
        client.world().resource::<crate::loading::Loading>().done()
    });
    client.core_mut().source = source;
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    let mut failures = Vec::new();
    for case in cases {
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        seek(&mut client, case.tick);
        let kind = if case.name.starts_with("lotus") {
            UnitKind::LotusPool
        } else {
            UnitKind::Shrine
        };
        let unit = client
            .view()
            .units
            .iter()
            .find(|unit| unit.kind == kind && unit.pos == case.at)
            .expect("recorded objective");
        let id = unit.id;
        let replay = std::mem::replace(
            &mut client.core_mut().source,
            bota_client_core::Source::Live(Box::new(ControlledFrames)),
        );
        client.core_mut().pinned = Some(bota_client_core::Pin::Unit(id));
        client.frames(2);
        settle_light(&mut client, night);
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 30.0,
            )));
        // Long enough for the rings' smoothed radii to settle on what the game feeds them.
        client.frames(120);
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        compare(&mut client, &case, light, &mut failures);
        client.core_mut().source = replay;
    }
    assert!(failures.is_empty(), "{light}: {failures:#?}");
}

#[test]
#[ignore = "offscreen production renderer; visual_check runs this contract"]
fn objective_ground_effects_have_their_authored_shapes_by_day_and_by_night() {
    contract(false);
    contract(true);
}

#[cfg(test)]
mod shape_tests {
    use super::*;

    /// A diff with the given pixels changed by `delta`.
    fn drawn(pixels: impl IntoIterator<Item = Vec2>, delta: u8) -> Diff {
        let mut diff = Diff {
            delta: vec![0; (SIZE.x * SIZE.y) as usize],
            signed: vec![[0; 3]; (SIZE.x * SIZE.y) as usize],
        };
        for p in pixels {
            let i = (p.y as u32 * SIZE.x + p.x as u32) as usize;
            diff.delta[i] = delta;
            diff.signed[i] = [
                i16::from(delta) / 10,
                i16::from(delta) / 2,
                i16::from(delta),
            ];
        }
        diff
    }
    const CENTRE: Vec2 = Vec2::new(800.0, 450.0);
    /// The screen points of a ring of `radius`, the ground seen at a slant.
    fn ring(radius: f32) -> Vec<Vec2> {
        (0..256)
            .map(|i| {
                let a = i as f32 * TAU / 256.0;
                CENTRE + Vec2::new(a.cos(), a.sin() * 0.8) * radius
            })
            .collect()
    }
    /// A band of `width` pixels about a ring.
    fn band(radius: f32, width: f32, delta: u8) -> Diff {
        drawn(
            (0..SIZE.x * SIZE.y).filter_map(|p| {
                let at = Vec2::new((p % SIZE.x) as f32, (p / SIZE.x) as f32);
                let r = ((at - CENTRE) * Vec2::new(1.0, 1.25)).length();
                ((r - radius).abs() <= width / 2.0).then_some(at)
            }),
            delta,
        )
    }

    #[test]
    fn a_band_at_the_authored_radius_is_a_visible_ring() {
        let stats = ring_stats(&band(200.0, 6.0, 100), &ring(200.0));
        assert!(stats.recall > 0.95, "{}", stats.recall);
        assert!(stats.thickness >= MIN_RING_THICKNESS, "{}", stats.thickness);
        assert!(stats.peak >= MIN_RING_PEAK);
    }

    #[test]
    fn a_band_at_half_the_authored_radius_is_not_that_ring() {
        let diff = band(100.0, 6.0, 100);
        assert!(ring_stats(&diff, &ring(200.0)).recall < RING_RECALL);
    }

    #[test]
    fn a_hairline_ring_is_too_thin_to_be_seen() {
        let stats = ring_stats(&band(200.0, 1.0, 100), &ring(200.0));
        assert!(stats.thickness < MIN_RING_THICKNESS, "{}", stats.thickness);
    }

    #[test]
    fn a_faint_ring_is_too_dim_to_be_seen() {
        let stats = ring_stats(&band(200.0, 6.0, MIN_RING_PEAK - 1), &ring(200.0));
        assert!(stats.peak < MIN_RING_PEAK);
    }

    #[test]
    fn a_ring_missing_its_inner_circle_fails_that_circle_alone() {
        let diff = band(200.0, 6.0, 100);
        assert!(ring_stats(&diff, &ring(200.0)).recall >= RING_RECALL);
        assert!(ring_stats(&diff, &ring(100.0)).recall < RING_RECALL);
    }

    #[test]
    fn a_quad_larger_than_the_footprint_leaves_it() {
        let footprint = hull(ring(300.0));
        let quad = drawn(
            (200..1400).flat_map(|x| (50..850).map(move |y| Vec2::new(x as f32, y as f32))),
            100,
        );
        assert!(glow_stats(&quad, &footprint).inside < 0.98);
    }

    #[test]
    fn the_colour_on_a_ring_is_the_colour_drawn_there() {
        let mean = colour_on_rings(&band(200.0, 6.0, 100), &[ring(200.0)]);
        assert!(mean[2] > mean[1] && mean[1] > mean[0], "{mean:?}");
    }

    #[test]
    fn separate_blobs_are_counted_as_separate_patches() {
        let blob = |c: Vec2| {
            (0..60).flat_map(move |x| (0..60).map(move |y| c + Vec2::new(x as f32, y as f32)))
        };
        let three: Vec<Vec2> = [
            Vec2::new(100.0, 100.0),
            Vec2::new(500.0, 100.0),
            Vec2::new(300.0, 500.0),
        ]
        .into_iter()
        .flat_map(blob)
        .collect();
        assert_eq!(patches(&three), 3);
        assert_eq!(
            patches(&blob(Vec2::new(100.0, 100.0)).collect::<Vec<_>>()),
            1
        );
    }

    #[test]
    fn a_footprint_contains_its_circle_and_leaves_out_the_rest() {
        let footprint = hull(ring(300.0));
        assert!(inside(&footprint, CENTRE));
        assert!(!inside(&footprint, CENTRE + Vec2::new(400.0, 0.0)));
        assert!(area(&footprint) > 0.0);
    }

    #[test]
    fn an_opaque_quad_over_the_footprint_is_not_a_glow() {
        let footprint = hull(ring(300.0));
        let quad = drawn(
            (500..1100).flat_map(|x| (300..600).map(move |y| Vec2::new(x as f32, y as f32))),
            200,
        );
        let stats = glow_stats(&quad, &footprint);
        assert!(stats.coverage > 0.3);
        assert!(stats.p90 > WATER_MAX_P90);
    }

    #[test]
    fn a_collapsed_glow_is_one_patch_not_the_three_basins() {
        let footprint = hull(ring(300.0));
        let one = drawn(
            (700..780).flat_map(|x| (400..480).map(move |y| Vec2::new(x as f32, y as f32))),
            60,
        );
        assert_eq!(glow_stats(&one, &footprint).patches, 1);
    }
}
