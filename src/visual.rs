//! Debug: `--visual-check` measures the picture a `--screenshot-at` shot
//! takes against the picture's requirements, prints every measure and its
//! bound, and exits non-zero when one fails. `tools/visual_check.sh` runs it
//! from both seats.
//!
//! - The fog: ground the watched side does not see is darker and duller
//!   than the same ground drawn again without the veil (a second shot of the
//!   held frame), ground it sees is left alone, and the border is neither
//!   hard nor smeared.
//! - Heroes are drawn with their models, not stand-ins.
//! - Buildings never turn.
//! - No range ring is drawn while the show-ranges key is up.
//! - The top-left block tells no gold and no souls.
//! - Buff icons are cut round.

use bevy::{ecs::system::SystemParam, prelude::*, window::PrimaryWindow};
use bota_proto::UnitKind;

use crate::camera::RtsCamera;
use crate::fog::FogGrid;
use crate::map::Ground;
use crate::session::Session;
use crate::terrain::CELL;
use crate::units::{Unit, UnitIndex};

/// Fogged ground's luminance against the same ground unfogged, at most, in
/// sRGB: the fog's day cast measures 0.42 to 0.45 and its night grey 0.31 to
/// 0.36, whatever ground is on screen.
pub const FOG_LUMINANCE_MAX: f32 = 0.5;
/// Fogged ground's saturation against the same ground unfogged, at most.
pub const FOG_SATURATION_MAX: f32 = 0.45;
/// The border, in cells (64 world units), over which luminance goes from
/// a tenth to nine tenths of the way: readable, neither a hard edge (under a
/// cell) nor a smear (over five, 320 units; Dota's soft edge spans about
/// three to four).
pub const BORDER_CELLS: (f32, f32) = (1.0, 5.0);
/// How far seen ground's luminance may differ from its unfogged luminance.
pub const SEEN_LUMINANCE_DRIFT: f32 = 0.05;
/// The spread of luminance over a hero's body, at least: a model has
/// shading and detail; a stand-in is one flat colour.
pub const HERO_DETAIL_MIN: f32 = 12.0;

/// What the checks read.
#[derive(SystemParam)]
pub struct Checks<'w, 's> {
    session: NonSend<'w, Session>,
    fog: Res<'w, FogGrid>,
    ground: Res<'w, Ground>,
    index: Res<'w, UnitIndex>,
    units: Query<'w, 's, &'static Unit>,
    camera: Single<'w, 's, (&'static Camera, &'static GlobalTransform), With<RtsCamera>>,
    projection: Single<'w, 's, &'static Projection, With<RtsCamera>>,
    window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    rings: Res<'w, crate::overlay::RangeRings>,
    bars: Res<'w, crate::bars::Bars>,
    yaws: Res<'w, crate::units::BuildingYaws>,
    tree: Query<'w, 's, &'static Children>,
    meshes: Query<'w, 's, (&'static Mesh3d, &'static ViewVisibility)>,
    wearables: Query<'w, 's, (), With<crate::units::Wearable>>,
    texts: Query<'w, 's, &'static Text>,
    chips: Query<
        'w,
        's,
        (
            &'static crate::buffs::BuffChip,
            &'static Children,
            &'static Visibility,
        ),
    >,
    icons: Query<'w, 's, &'static ImageNode>,
    images: Res<'w, Assets<Image>>,
}

/// The screenshot as sRGB bytes, by physical pixel.
struct Shot<'a> {
    data: &'a [u8],
    w: usize,
    h: usize,
    bgra: bool,
}

impl<'a> Shot<'a> {
    fn of(image: &'a Image) -> Option<Shot<'a>> {
        Some(Shot {
            data: image.data.as_deref()?,
            w: image.width() as usize,
            h: image.height() as usize,
            bgra: matches!(
                image.texture_descriptor.format,
                bevy::render::render_resource::TextureFormat::Bgra8UnormSrgb
                    | bevy::render::render_resource::TextureFormat::Bgra8Unorm
            ),
        })
    }
}

impl Shot<'_> {
    /// Mean sRGB colour, 0 to 255, of a square around a pixel.
    fn mean(&self, x: f32, y: f32, half: i32) -> Option<[f32; 3]> {
        let (cx, cy) = (x as i32, y as i32);
        let mut sum = [0.0; 3];
        let mut n = 0.0;
        for dy in -half..=half {
            for dx in -half..=half {
                let (px, py) = (cx + dx, cy + dy);
                if px < 0 || py < 0 || px as usize >= self.w || py as usize >= self.h {
                    return None;
                }
                let at = (py as usize * self.w + px as usize) * 4;
                let p = &self.data[at..at + 3];
                let rgb = if self.bgra {
                    [p[2], p[1], p[0]]
                } else {
                    [p[0], p[1], p[2]]
                };
                for c in 0..3 {
                    sum[c] += f32::from(rgb[c]);
                }
                n += 1.0;
            }
        }
        Some(sum.map(|s| s / n))
    }
}

fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn saturation(c: [f32; 3]) -> f32 {
    let max = c[0].max(c[1]).max(c[2]);
    let min = c[0].min(c[1]).min(c[2]);
    if max <= 0.0 { 0.0 } else { (max - min) / max }
}

/// Runs every check on a screenshot; prints each; true when all pass.
pub fn run(image: &Image, unveiled: &Image, c: &Checks) -> bool {
    let Some(bare) = Shot::of(unveiled) else {
        error!("visual check: the unveiled screenshot has no data");
        return false;
    };
    let Some(shot) = Shot::of(image) else {
        return false;
    };
    let mut ok = true;
    let mut report = |name: &str, pass: bool, detail: String| {
        println!(
            "visual check: {} {name}: {detail}",
            if pass { "PASS" } else { "FAIL" }
        );
        ok &= pass;
    };
    hero_parts(c, &mut report);
    fog(&shot, &bare, c, &mut report);
    heroes(&shot, c, &mut report);
    buildings(c, &mut report);
    report(
        "no range rings without the key",
        c.rings.0 == 0,
        format!("{} range drawings this frame", c.rings.0),
    );
    corner(c, &mut report);
    buff_icons(c, &mut report);
    picks(c, &mut report);
    ok
}

/// Every mesh of every hero's model and wearables is there and drawn:
/// the counts, printed in one line, must match from run to run.
fn hero_parts(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let mut parts: Vec<String> = Vec::new();
    for entity in c.index.0.values() {
        let Ok(unit) = c.units.get(*entity) else {
            continue;
        };
        if unit.kind != UnitKind::Hero {
            continue;
        }
        let all: Vec<Entity> = c.tree.iter_descendants(*entity).collect();
        let meshes = all.iter().filter(|e| c.meshes.contains(**e)).count();
        let drawn = all
            .iter()
            .filter(|e| c.meshes.get(**e).is_ok_and(|(_, v)| v.get()))
            .count();
        let pending = all.iter().filter(|e| c.wearables.contains(**e)).count();
        parts.push(format!(
            "{:?}: {meshes} meshes, {drawn} drawn, {pending} wearables unbound",
            unit.team
        ));
    }
    parts.sort();
    println!("visual check: hero parts: {}", parts.join("; "));
    report(
        "hero parts all loaded",
        !parts.is_empty() && parts.iter().all(|p| p.ends_with(" 0 wearables unbound")),
        parts.join("; "),
    );
}

/// Buildings stand as the map places them; every enemy building, in the
/// fog or not, is drawn with its bar; the enemy base is fogged.
fn buildings(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let turned: Vec<String> = c
        .units
        .iter()
        .filter(|u| bota_core::kind_def(u.kind).structure && u.team != bota_proto::Team::Neutral)
        .filter_map(|u| {
            let view = c.session.0.view()?;
            let wire = view.unit(u.id)?;
            let want = c.yaws.facing(wire, u.at);
            match want {
                Some(want)
                    if ((u.facing - want).rem_euclid(std::f32::consts::TAU)) < 1e-3
                        || ((want - u.facing).rem_euclid(std::f32::consts::TAU)) < 1e-3 =>
                {
                    None
                }
                Some(want) => Some(format!(
                    "{:?} {:?} at {:.0},{:.0}: {:.2} against the map's {:.2}",
                    u.team, u.kind, u.at.x, u.at.y, u.facing, want
                )),
                None => Some(format!(
                    "{:?} {:?} at {:.0},{:.0}: not on the map",
                    u.team, u.kind, u.at.x, u.at.y
                )),
            }
        })
        .collect();
    report(
        "buildings stand as the map places them",
        turned.is_empty(),
        if turned.is_empty() {
            "every building at the map's yaw".into()
        } else {
            turned.join(", ")
        },
    );
    // Every enemy building, in the fog or not, is drawn with its bar.
    let side = c.fog.team.unwrap_or(bota_proto::Team::Radiant);
    let enemy: Vec<(Entity, bool)> = c
        .index
        .0
        .values()
        .filter_map(|e| Some((*e, c.units.get(*e).ok()?)))
        .filter(|(_, u)| {
            bota_core::kind_def(u.kind).structure
                && u.team != side
                && u.team != bota_proto::Team::Neutral
        })
        .map(|(e, u)| (e, c.fog.lit(u.at.x, u.at.y)))
        .collect();
    let fogged = enemy.iter().filter(|(_, lit)| !lit).count();
    // The fountain has no bar, in Dota as here.
    let barless = enemy
        .iter()
        .filter(|(e, _)| {
            c.units.get(*e).is_ok_and(|u| u.kind != UnitKind::Fountain) && !c.bars.has(*e)
        })
        .count();
    let expected = c.session.0.view().map_or(0, |v| {
        v.units
            .iter()
            .filter(|u| {
                bota_core::kind_def(u.kind).structure
                    && u.team != side
                    && u.team != bota_proto::Team::Neutral
            })
            .count()
    });
    report(
        "enemy buildings seen through the fog",
        fogged > 0 && enemy.len() == expected && barless == 0,
        format!(
            "{} of {expected} enemy buildings drawn, {fogged} of them fogged, {barless} without a health bar",
            enemy.len()
        ),
    );
    // A side sees by its own units alone: the enemy's fountain, always
    // drawn, lights nothing for the side watching.
    if let (Some(side), Some(view)) = (c.fog.team, c.session.0.view()) {
        let enemy_home = view.units.iter().find(|u| {
            u.kind == UnitKind::Fountain && u.team != side && u.team != bota_proto::Team::Neutral
        });
        if let Some(f) = enemy_home {
            let lit = c.fog.lit(f.pos.x.to_f32(), f.pos.y.to_f32());
            report(
                "the enemy base is fogged",
                !lit,
                format!(
                    "{:?}'s view of the {:?} fountain: {}",
                    side,
                    f.team,
                    if lit { "lit" } else { "fogged" }
                ),
            );
        }
    }
}

/// The top-left block tells no gold and no souls.
fn corner(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let told: Vec<&str> = c
        .texts
        .iter()
        .map(|t| t.0.as_str())
        .filter(|t| t.contains("GOLD") || t.contains("SOULS"))
        .collect();
    report(
        "no gold or souls in the corner",
        told.is_empty(),
        if told.is_empty() {
            "none told".into()
        } else {
            format!("{told:?}")
        },
    );
}

/// Every buff icon shown is cut round: its corner pixel is clear.
fn buff_icons(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let mut shown = 0;
    let mut square = 0;
    for (_, children, visibility) in &c.chips {
        if *visibility == Visibility::Hidden {
            continue;
        }
        let Some(icon) = children.first().and_then(|e| c.icons.get(*e).ok()) else {
            continue;
        };
        let Some(art) = c.images.get(&icon.image) else {
            continue;
        };
        shown += 1;
        let corner = art
            .data
            .as_ref()
            .and_then(|d| d.get(3))
            .copied()
            .unwrap_or(255);
        if corner != 0 {
            square += 1;
        }
    }
    report(
        "buff icons cut round",
        square == 0,
        format!("{shown} icons shown, {square} with a square corner"),
    );
}

/// The fog's effect on the ground: every ground cell on screen away from the
/// HUD, the units and the border is read in `fogged`, the picture as drawn,
/// and in `clear`, the same frame drawn without the veil, so the ground on
/// screen does not decide the verdict.
fn fog(fogged: &Shot, clear: &Shot, c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let grid = &*c.fog;
    if !grid.on || grid.n == 0 {
        report("fog", false, "no fog: run with --pov".into());
        return;
    }
    let (camera, transform) = *c.camera;
    let scale = c.window.scale_factor();
    let (lw, lh) = (c.window.width(), c.window.height());
    let units: Vec<Vec2> = c
        .session
        .0
        .view()
        .map(|v| {
            v.units
                .iter()
                .map(|u| Vec2::new(u.pos.x.to_f32(), u.pos.y.to_f32()))
                .collect()
        })
        .unwrap_or_default();
    // By signed distance to the border, from -5 to 5: sums over the cells.
    let mut bins = [Bin::default(); 11];
    for cy in 0..grid.n {
        for cx in 0..grid.n {
            let (x, y) = ((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL);
            if units.iter().any(|u| u.distance(Vec2::new(x, y)) < 250.0) {
                continue;
            }
            let world = Vec3::new(x, c.ground.at(x, y), -y);
            let Ok(p) = camera.world_to_viewport(transform, world) else {
                continue;
            };
            if p.x < lw * 0.05 || p.x > lw * 0.95 || p.y < lh * 0.1 || p.y > lh * 0.78 {
                continue;
            }
            let (Some(drawn), Some(bare)) = (
                fogged.mean(p.x * scale, p.y * scale, 2),
                clear.mean(p.x * scale, p.y * scale, 2),
            ) else {
                continue;
            };
            bins[(border_distance(grid, cx, cy) + 5) as usize].add(drawn, bare);
        }
    }
    let inside = Bin::sum(&bins[..2]);
    let outside = Bin::sum(&bins[9..]);
    if inside.n < 30 || outside.n < 30 {
        report(
            "fog",
            false,
            format!(
                "too little ground on screen: {} seen and {} fogged samples",
                outside.n, inside.n
            ),
        );
        return;
    }
    let lum = inside.drawn_lum / inside.bare_lum.max(1.0);
    let sat = inside.drawn_sat / inside.bare_sat.max(1e-3);
    let untouched = outside.drawn_lum / outside.bare_lum.max(1.0);
    report(
        "fog darkens",
        lum <= FOG_LUMINANCE_MAX,
        format!(
            "fogged ground at {:.0}% of its own unfogged luminance (at most {:.0}%; {} cells)",
            lum * 100.0,
            FOG_LUMINANCE_MAX * 100.0,
            inside.n
        ),
    );
    report(
        "fog dulls",
        sat <= FOG_SATURATION_MAX,
        format!(
            "fogged ground at {:.0}% of its own unfogged saturation (at most {:.0}%)",
            sat * 100.0,
            FOG_SATURATION_MAX * 100.0
        ),
    );
    report(
        "fog leaves seen ground alone",
        (untouched - 1.0).abs() <= SEEN_LUMINANCE_DRIFT,
        format!(
            "seen ground at {:.0}% of its unfogged luminance (within {:.0}%; {} cells)",
            untouched * 100.0,
            SEEN_LUMINANCE_DRIFT * 100.0,
            outside.n
        ),
    );
    fog_border(&bins, report);
}

/// Sums of what the cells at one distance from the fog's border look like
/// drawn with the fog and without.
#[derive(Clone, Copy, Default)]
struct Bin {
    n: usize,
    drawn_lum: f32,
    bare_lum: f32,
    drawn_sat: f32,
    bare_sat: f32,
}

impl Bin {
    fn add(&mut self, drawn: [f32; 3], bare: [f32; 3]) {
        self.n += 1;
        self.drawn_lum += luminance(drawn);
        self.bare_lum += luminance(bare);
        self.drawn_sat += saturation(drawn);
        self.bare_sat += saturation(bare);
    }

    fn sum(bins: &[Bin]) -> Bin {
        bins.iter().fold(Bin::default(), |mut all, b| {
            all.n += b.n;
            all.drawn_lum += b.drawn_lum;
            all.bare_lum += b.bare_lum;
            all.drawn_sat += b.drawn_sat;
            all.bare_sat += b.bare_sat;
            all
        })
    }

    /// How much of the unfogged luminance is left, in percent.
    fn left(&self) -> Option<f32> {
        (self.n >= 5).then(|| 100.0 * self.drawn_lum / self.bare_lum.max(1.0))
    }
}

/// Signed distance in cells from a cell to the nearest cell of the other
/// state, up to four (five beyond): negative inside the fog.
fn border_distance(grid: &FogGrid, cx: usize, cy: usize) -> i32 {
    let n = grid.n;
    let lit = |cx: usize, cy: usize| grid.cells[cy * n + cx];
    let me = lit(cx, cy);
    for d in 1..=4i32 {
        for dy in -d..=d {
            for dx in -d..=d {
                let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                if x < 0 || y < 0 || x as usize >= n || y as usize >= n {
                    continue;
                }
                if lit(x as usize, y as usize) != me {
                    return if me { d } else { -d };
                }
            }
        }
    }
    if me { 5 } else { -5 }
}

/// The width of the fog's border, from the profile across it: how much of
/// the unfogged luminance the cells at each signed distance keep.
fn fog_border(bins: &[Bin], report: &mut impl FnMut(&str, bool, String)) {
    let profile: Vec<Option<f32>> = bins.iter().map(Bin::left).collect();
    // The levels either side, each the mean of the cells well off the
    // border, so the ground's own shading does not move them.
    let level = |range: std::ops::Range<usize>| -> Option<f32> {
        let v: Vec<f32> = profile[range].iter().flatten().copied().collect();
        (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
    };
    let (Some(lo), Some(hi)) = (level(0..3), level(7..11)) else {
        report("fog border", false, "no profile".into());
        return;
    };
    let at = |share: f32| -> Option<f32> {
        let target = lo + (hi - lo) * share;
        (0..10).find_map(|i| {
            let (a, b) = (profile[i]?, profile[i + 1]?);
            ((a - target) * (b - target) <= 0.0 && a != b)
                .then(|| i as f32 + (target - a) / (b - a))
        })
    };
    let width = match (at(0.1), at(0.9)) {
        (Some(a), Some(b)) => b - a,
        _ => f32::NAN,
    };
    report(
        "fog border readable",
        width >= BORDER_CELLS.0 && width <= BORDER_CELLS.1,
        format!(
            "10-90% over {width:.1} cells (between {} and {}); profile {:?}",
            BORDER_CELLS.0,
            BORDER_CELLS.1,
            profile
                .iter()
                .map(|p| p.map(|v| v.round() as i32))
                .collect::<Vec<_>>()
        ),
    );
}

/// Every hero on screen shows detail where its body is drawn.
fn heroes(shot: &Shot, c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let (camera, transform) = *c.camera;
    let scale = c.window.scale_factor();
    let mut checked = 0;
    for entity in c.index.0.values() {
        let Ok(unit) = c.units.get(*entity) else {
            continue;
        };
        if unit.kind != UnitKind::Hero {
            continue;
        }
        let world = Vec3::new(
            unit.at.x,
            c.ground.at(unit.at.x, unit.at.y) + 110.0,
            -unit.at.y,
        );
        let Ok(p) = camera.world_to_viewport(transform, world) else {
            continue;
        };
        let mut lums = Vec::new();
        for dy in -6..=6 {
            for dx in -6..=6 {
                if let Some(col) = shot.mean(
                    (p.x + dx as f32 * 2.0) * scale,
                    (p.y + dy as f32 * 2.0) * scale,
                    0,
                ) {
                    lums.push(luminance(col));
                }
            }
        }
        if lums.len() < 50 {
            continue;
        }
        checked += 1;
        let mean = lums.iter().sum::<f32>() / lums.len() as f32;
        let spread =
            (lums.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / lums.len() as f32).sqrt();
        report(
            "hero drawn with its model",
            unit.model.is_some() && spread >= HERO_DETAIL_MIN,
            format!(
                "{:?} hero: model {}, luminance spread {spread:.1} (at least {HERO_DETAIL_MIN})",
                unit.team,
                if unit.model.is_some() {
                    "loaded"
                } else {
                    "MISSING (stand-in)"
                }
            ),
        );
    }
    if checked == 0 {
        report(
            "hero drawn with its model",
            false,
            "no hero on screen".into(),
        );
    }
}

/// A click near a unit, not on it, gets it: for every unit on screen with
/// no other close by, clicks a little off the middle of its body in four
/// directions (within the screen-space minimum reach) and asks the picker
/// what it gets.
fn picks(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    use crate::pick::{Candidate, Lens, MIN_SCREEN_RADIUS, Side, Want, pick, stands};
    let (camera, transform) = *c.camera;
    let Some(view) = c.session.0.view() else {
        report("clicks near units pick them", false, "no view".into());
        return;
    };
    let fov = match *c.projection {
        Projection::Perspective(p) => p.fov,
        _ => return,
    };
    let all: Vec<(Candidate, Vec2)> =
        stands(view, &c.index, &c.units, &c.ground, c.session.0.my_team()).collect();
    let slack = c.window.height() * MIN_SCREEN_RADIUS * 0.8;
    let on_screen =
        |p: &Vec2| p.x > 0.0 && p.y > 0.0 && p.x < c.window.width() && p.y < c.window.height();
    // Every unit, off screen and fountains too: a click nearer another unit's
    // body than its own rightly gets that one, so a unit counts as isolated
    // only when nothing at all stands close to it on screen.
    let shown: Vec<(Candidate, Vec2)> = all
        .iter()
        .filter_map(|(u, _)| {
            let mid = u.base + Vec3::Y * (u.height / 2.0).min(60.0);
            camera
                .world_to_viewport(transform, mid)
                .ok()
                .map(|p| (*u, p))
        })
        .collect();
    let (mut tried, mut wrong) = (0, Vec::new());
    for (unit, at) in &shown {
        if unit.kind == UnitKind::Fountain
            || !on_screen(at)
            || shown
                .iter()
                .any(|(o, p)| o.id != unit.id && p.distance(*at) < slack * 6.0)
        {
            continue;
        }
        for dir in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
            let Ok(ray) = camera.viewport_to_world(transform, *at + dir * slack) else {
                continue;
            };
            let want = Want {
                side: Side::Any,
                strict: false,
                exclude: None,
                orders: false,
            };
            tried += 1;
            let got = pick(ray, Lens::new(fov), all.iter().map(|(u, _)| *u), &want);
            if got != Some(unit.id) {
                wrong.push(format!("{:?} {:?} missed at {dir}", unit.kind, unit.id));
            }
        }
    }
    report(
        "clicks near units pick them",
        tried > 0 && wrong.is_empty(),
        format!("{tried} clicks off the middle of isolated units; wrong: {wrong:?}"),
    );
}
