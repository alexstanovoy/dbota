//! Debug: `--visual-check` measures the picture a `--screenshot-at` shot
//! takes against the picture's requirements, prints every measure and its
//! bound, and exits non-zero when one fails. `tools/visual_check.sh` runs it
//! from both seats.
//!
//! - The fog: ground the watched side does not see is darker and duller
//!   than the same ground drawn again without the veil (a second shot of the
//!   held frame), ground it sees is left alone, and the border is neither
//!   hard nor smeared.
//! - Heroes are drawn with their models, not stand-ins, each part on the
//!   hero's own live skeleton, and no model's scene is replaced once drawn.
//! - Buildings never turn.
//! - No range ring is drawn while the show-ranges key is up.
//! - The top-left block tells no gold and no souls.
//! - Buff icons are cut round.
//! - Every hero's minimap mark is its player's colour (Dota's, from the
//!   cache) ringed in its side's, and the top bar's strip over each seat its
//!   player's colour: a Dire hero is never marked in green.
//! - Every health bar shown marks its health every 250 and every 1000 where
//!   [`crate::bars::tick_marks`] puts the marks; with `--bars-check` (the
//!   camera on the watched seat's courier) only the bars are measured, and a
//!   bar of health counted in hits must stand as one block a hit, apart,
//!   as many standing as the health has hits left.
//! - With `--water-check` (the camera on the river) only the river is
//!   measured: one surface, no straight sharp step in its shading where one
//!   of the map's water pieces meets the next.
//! - A frame with too little ground on screen, seen and fogged, for the fog
//!   to be measured (the hero dead in its fountain, the camera on a dark
//!   corner) checks nothing: the replay plays on and the whole check is made
//!   [`FOG_RETAKE_TICKS`] later, again, until a frame within
//!   [`FOG_WINDOW_TICKS`] of the shot has enough; a window without one fails.
//! - A click near a unit with no other near it picks it. A frame with no such
//!   unit for the seat checks nothing: the replay plays on and the check is
//!   made on the first frame within [`CLICK_WINDOW_TICKS`] that has one; a
//!   window without any fails.

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

/// How far from its unit's place a joint of a hero's skeleton may be, in
/// world units: twice the tallest hero. A part farther off is drawn on some
/// other skeleton than the one walking.
pub const JOINT_REACH: f32 = 400.0;

/// The sharpest straight step the river's shading may take, in sRGB
/// luminance (0 to 255) above the steps beside it. Water pieces each reading
/// the normal map their own way stepped 3.3 to 7.0 (windowed and at 4K, by
/// day and by night); one surface steps 1.5 to 1.8.
pub const WATER_STEP_MAX: f32 = 2.5;
/// The share of the measured middle of the screen that must be water.
pub const WATER_SHARE_MIN: f32 = 0.6;

/// Ticks after a shot within which a frame with a unit to click near is
/// looked for, when the shot has none.
pub const CLICK_WINDOW_TICKS: u32 = 900;

/// Ticks after a shot within which a frame with enough ground for the fog is
/// looked for, when the shot has too little.
pub const FOG_WINDOW_TICKS: u32 = 900;
/// Ticks between the frames tried for it.
pub const FOG_RETAKE_TICKS: u32 = 150;
/// Samples each side of the fog's border a frame needs for the fog to be
/// measured.
const FOG_SAMPLES_MIN: usize = 30;

/// The verdict on a shot: whether every check made passed, whether the
/// click check is still to be made on a later frame, and whether the frame
/// had too little ground for the fog and the whole check is to be made on a
/// later one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub pass: bool,
    pub clicks_pending: bool,
    pub retake: bool,
}

/// What the checks read.
#[derive(SystemParam)]
pub struct Checks<'w, 's> {
    args: Res<'w, crate::Args>,
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
    wearables: Query<'w, 's, &'static crate::units::Wearable>,
    skins: Query<'w, 's, &'static bevy::mesh::skinning::SkinnedMesh>,
    places: Query<'w, 's, &'static GlobalTransform>,
    replaced: Res<'w, crate::units::ReplacedScenes>,
    texts: Query<'w, 's, &'static Text>,
    buttons: Query<'w, 's, (), With<Button>>,
    fills: Query<'w, 's, &'static BackgroundColor>,
    extents: Query<'w, 's, (Entity, &'static crate::ui::Extent)>,
    hud:
        Query<'w, 's, (&'static ComputedNode, &'static UiGlobalTransform), With<crate::ui::Extent>>,
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
    colours: Res<'w, crate::player_colour::PlayerColours>,
    hero_marks: Query<
        'w,
        's,
        (
            &'static crate::minimap::HeroMark,
            &'static ImageNode,
            &'static Visibility,
        ),
    >,
    strips: Query<'w, 's, (&'static crate::hud::SeatColour, &'static BackgroundColor)>,
    laid: Query<'w, 's, (&'static Node, &'static Visibility)>,
}

impl Checks<'_, '_> {
    /// The tick the view stands at.
    pub fn tick(&self) -> u32 {
        self.session.0.view().map_or(0, |v| v.tick)
    }
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

/// Runs every check on a screenshot; prints each.
pub fn run(image: &Image, unveiled: &Image, c: &Checks, may_wait: bool) -> Verdict {
    let failed = Verdict {
        pass: false,
        clicks_pending: false,
        retake: false,
    };
    let Some(bare) = Shot::of(unveiled) else {
        error!("visual check: the unveiled screenshot has no data");
        return failed;
    };
    let Some(shot) = Shot::of(image) else {
        return failed;
    };
    let mut ok = true;
    let mut report = |name: &str, pass: bool, detail: String| {
        println!(
            "visual check: {} {name}: {detail}",
            if pass { "PASS" } else { "FAIL" }
        );
        ok &= pass;
    };
    if c.args.bars_check {
        bar_ticks(c, &mut report);
        bar_blocks(c, &mut report);
        return Verdict {
            pass: ok,
            clicks_pending: false,
            retake: false,
        };
    }
    if c.args.water_check {
        water(&bare, &mut report);
        return Verdict {
            pass: ok,
            clicks_pending: false,
            retake: false,
        };
    }
    let bins = fog_bins(&shot, &bare, c);
    if may_wait && bins.as_ref().is_some_and(|b| !enough_ground(b)) {
        println!(
            "visual check: too little ground for the fog at tick {}; the check is made {} \
             ticks on",
            c.tick(),
            FOG_RETAKE_TICKS
        );
        return Verdict {
            pass: true,
            clicks_pending: false,
            retake: true,
        };
    }
    hero_parts(c, &mut report);
    fog(bins, &mut report);
    heroes(&shot, c, &mut report);
    buildings(c, &mut report);
    report(
        "no range rings without the key",
        c.rings.0 == 0,
        format!("{} range drawings this frame", c.rings.0),
    );
    corner(c, &mut report);
    bar_sides(c, &mut report);
    bar_ticks(c, &mut report);
    player_colours(c, &mut report);
    buff_icons(c, &mut report);
    let clicks_pending = !report_clicks(c, &mut report);
    if clicks_pending {
        let tick = c.tick();
        println!(
            "visual check: no unit alone on screen at tick {tick}; clicks near units are \
             checked on the next frame within {CLICK_WINDOW_TICKS} ticks that has one"
        );
    }
    Verdict {
        pass: ok,
        clicks_pending,
        retake: false,
    }
}

/// The click check on the frame drawn now, reported, if the frame has a unit
/// alone on screen to click near; false when it has none.
pub fn report_clicks(c: &Checks, report: &mut impl FnMut(&str, bool, String)) -> bool {
    let Some((tried, wrong)) = clicks(c) else {
        return false;
    };
    report(
        "clicks near units pick them",
        wrong.is_empty(),
        format!("{tried} clicks off the middle of isolated units; wrong: {wrong:?}"),
    );
    true
}

/// Every mesh of every hero's model and wearables is there and drawn, and
/// every wearable rides its hero's own live skeleton: the counts, printed
/// in one line, must match from run to run. No model's scene was replaced
/// under the units drawn with it (a file loaded twice).
fn hero_parts(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let mut parts: Vec<String> = Vec::new();
    let mut skeleton: Vec<String> = Vec::new();
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
        let pending = all
            .iter()
            .filter(|e| c.wearables.get(**e).is_ok_and(|w| !w.bound()))
            .count();
        parts.push(format!(
            "{:?}: {meshes} meshes, {drawn} drawn, {pending} wearables unbound",
            unit.team
        ));
        skeleton.push(worn_skins(c, *entity, unit, &all));
    }
    parts.sort();
    skeleton.sort();
    println!("visual check: hero parts: {}", parts.join("; "));
    report(
        "hero parts all loaded",
        !parts.is_empty() && parts.iter().all(|p| p.ends_with(" 0 wearables unbound")),
        parts.join("; "),
    );
    report(
        "hero parts ride the hero's skeleton",
        !skeleton.is_empty() && skeleton.iter().all(|s| s.contains(" 0 joints off it")),
        skeleton.join("; "),
    );
    report(
        "no model scene replaced once drawn",
        c.replaced.0 == 0,
        format!("{} scenes replaced", c.replaced.0),
    );
}

/// Where the skins a hero wears point: joints that are gone (left where
/// the skeleton they were bound to was), not on the wearer's skeleton
/// (nor the wearable's own, for a joint the wearer lacks), or out of the
/// hero's reach.
fn worn_skins(c: &Checks, root: Entity, unit: &Unit, all: &[Entity]) -> String {
    let at = c.places.get(root).map_or(Vec3::ZERO, |p| p.translation());
    let (mut joints, mut off, mut farthest) = (0, 0, 0.0f32);
    for (worn, wearable) in all
        .iter()
        .filter_map(|e| Some((*e, c.wearables.get(*e).ok()?)))
    {
        let on: Vec<Entity> = c
            .tree
            .iter_descendants(wearable.wearer())
            .chain(c.tree.iter_descendants(worn))
            .collect();
        for skin in c
            .tree
            .iter_descendants(worn)
            .filter_map(|e| c.skins.get(e).ok())
        {
            for joint in &skin.joints {
                joints += 1;
                let reach = c
                    .places
                    .get(*joint)
                    .ok()
                    .filter(|_| on.contains(joint))
                    .map(|place| place.translation().distance(at));
                match reach {
                    Some(d) if d <= JOINT_REACH => farthest = farthest.max(d),
                    _ => off += 1,
                }
            }
        }
    }
    format!(
        "{:?}: {joints} worn joints, {off} joints off it, farthest {farthest:.0} \
         from the hero (at most {JOINT_REACH})",
        unit.team
    )
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
    // The fountain and the map's objectives have no bar, in Dota as here.
    let barless = enemy
        .iter()
        .filter(|(e, _)| {
            c.units
                .get(*e)
                .is_ok_and(|u| crate::bars::shows_bar(u.kind))
                && !c.bars.has(*e)
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

/// The top-left block holds the menu's buttons and tells nothing: no gold,
/// no souls, no last hits or denies, whose ever they are.
fn corner(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let parts: Vec<Entity> = c
        .extents
        .iter()
        .filter(|(_, extent)| **extent == crate::ui::Extent::Corner)
        .flat_map(|(block, _)| c.tree.iter_descendants(block))
        .collect();
    let buttons = parts.iter().filter(|e| c.buttons.contains(**e)).count();
    let told: Vec<&str> = parts
        .iter()
        .filter_map(|part| c.texts.get(*part).ok())
        .map(|t| t.0.as_str())
        .filter(|t| !t.is_empty())
        .collect();
    report(
        "the corner holds the menu and no numbers",
        buttons == 2 && told.is_empty(),
        format!("{buttons} buttons, texts {told:?}"),
    );
}

/// Every hero's health bar is its side's colour, whichever side watches:
/// Radiant green, Dire red, the watching side's own hero's too.
fn bar_sides(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let mut told = Vec::new();
    for (unit, e) in c
        .index
        .0
        .values()
        .filter_map(|e| Some((c.units.get(*e).ok()?, *e)))
    {
        if unit.kind != UnitKind::Hero {
            continue;
        }
        let Some(fill) = c.bars.fill(e).and_then(|f| c.fills.get(f).ok()) else {
            continue;
        };
        let rgb = fill.0.to_srgba();
        let green = rgb.green > rgb.red;
        let right = match unit.team {
            bota_proto::Team::Radiant => green,
            _ => !green,
        };
        told.push((unit.team, right));
    }
    report(
        "health bars in their side's colour",
        told.iter().any(|(side, _)| Some(*side) == c.fog.team)
            && told.iter().all(|(_, right)| *right),
        format!("heroes' bars (side, right colour): {told:?}"),
    );
}

/// Every hero's mark on the minimap is its player's colour, and every seat's
/// strip in the top bar its player's colour.
fn player_colours(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let Some(view) = c.session.0.view() else {
        report(
            "hero marks in their player's colour",
            false,
            "no view".into(),
        );
        return;
    };
    let mut told = Vec::new();
    for (mark, image, shown) in &c.hero_marks {
        let (Some((_, seat)), false) = (mark.0, *shown == Visibility::Hidden) else {
            continue;
        };
        let right = image.color == c.colours.of(view, seat);
        told.push(("minimap", seat.0, right));
    }
    for (strip, fill) in &c.strips {
        if let Some(p) = crate::hud::seat_of(view, strip.0) {
            told.push(("top bar", p.slot.0, fill.0 == c.colours.of(view, p.slot)));
        }
    }
    report(
        "hero marks in their player's colour",
        told.iter().any(|(on, ..)| *on == "minimap") && told.iter().all(|(.., right)| *right),
        format!("marks (where, seat, right colour): {told:?}"),
    );
}

/// The shown bars of units, with their unit's view: entity, width and the
/// view.
fn shown_bars<'a>(c: &'a Checks) -> Vec<(Entity, f32, &'a bota_proto::UnitView)> {
    let Some(view) = c.session.0.view() else {
        return Vec::new();
    };
    c.index
        .0
        .values()
        .filter(|e| c.bars.has(**e))
        .filter_map(|e| {
            let unit = c.units.get(*e).ok()?;
            let root = c.bars.root(*e)?;
            let (node, shown) = c.laid.get(root).ok()?;
            let Val::Px(w) = node.width else { return None };
            (*shown != Visibility::Hidden).then_some((*e, w, view.unit(unit.id)?))
        })
        .collect()
}

/// The left edge's middle of a node, in its parent's pixels: where a mark
/// stands.
fn mark_at(node: &Node) -> Option<(f32, f32)> {
    match (node.left, node.width) {
        (Val::Px(left), Val::Px(width)) => Some((left + width / 2.0, width)),
        _ => None,
    }
}

/// Every shown bar marks its health where Dota does: a small mark every 250,
/// a big one every 1000, none past its maximum; a hero's bar has some.
fn bar_ticks(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let mut told = Vec::new();
    for (e, w, live) in shown_bars(c) {
        if !c.bars.segments(e).is_empty() {
            continue;
        }
        let drawn: Vec<(f32, bool)> = c
            .bars
            .ticks(e)
            .iter()
            .filter_map(|t| c.laid.get(*t).ok())
            .filter(|(_, shown)| **shown != Visibility::Hidden)
            .filter_map(|(node, _)| {
                mark_at(node).map(|(x, width)| (x, width >= crate::bars::BIG_TICK_WIDTH))
            })
            .collect();
        let wanted = crate::bars::tick_marks(live.max_hp, w);
        let right = drawn.len() == wanted.len()
            && drawn
                .iter()
                .zip(&wanted)
                .all(|(d, want)| (d.0 - want.0).abs() < 0.5 && d.1 == want.1);
        told.push((live.kind, live.max_hp, drawn.len(), right));
    }
    // A hero's bar is on screen in the full check; the bars' check watches a
    // courier and needs a marked bar of any kind.
    let hero_marked = told.iter().any(|(kind, max, n, _)| {
        (*kind == UnitKind::Hero || c.args.bars_check) && (*max <= 250 || *n > 0)
    });
    report(
        "health marks every 250 and 1000",
        hero_marked && told.iter().all(|(.., right)| *right),
        format!("bars (kind, max health, marks, right): {told:?}"),
    );
}

/// Every shown bar of health counted in hits is one block a hit, the blocks
/// apart, as many standing as the health has hits left.
fn bar_blocks(c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let mut told = Vec::new();
    for (e, _, live) in shown_bars(c) {
        let Some((block, blocks)) = crate::bars::hit_blocks(live.kind, live.max_hp) else {
            continue;
        };
        let parts: Vec<(&Node, &Visibility)> = c
            .bars
            .segments(e)
            .iter()
            .filter_map(|s| c.laid.get(*s).ok())
            .collect();
        let standing = parts
            .iter()
            .filter(|(_, v)| **v != Visibility::Hidden)
            .count();
        let edges: Vec<(f32, f32)> = parts
            .iter()
            .filter_map(|(n, _)| match (n.left, n.width) {
                (Val::Px(l), Val::Px(w)) => Some((l, l + w)),
                _ => None,
            })
            .collect();
        let apart = edges.windows(2).all(|p| p[1].0 - p[0].1 >= 1.0);
        let left = (live.hp.max(0) as u32).div_ceil(block as u32) as usize;
        let right = parts.len() == blocks && standing == left && apart;
        told.push((live.kind, live.hp, blocks, standing, right));
    }
    report(
        "health counted in hits as one block a hit",
        !told.is_empty() && told.iter().all(|(.., right)| *right),
        format!("bars (kind, health, blocks, standing, right): {told:?}"),
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
/// Whether the frame has ground enough, seen and fogged, for the fog to be
/// measured.
fn enough_ground(bins: &[Bin; 11]) -> bool {
    Bin::sum(&bins[..2]).n >= FOG_SAMPLES_MIN && Bin::sum(&bins[9..]).n >= FOG_SAMPLES_MIN
}

/// The ground on screen drawn with the fog and without it, summed by signed
/// distance to the fog's border; none without a fog.
fn fog_bins(fogged: &Shot, clear: &Shot, c: &Checks) -> Option<[Bin; 11]> {
    let grid = &*c.fog;
    if !grid.on || grid.n == 0 {
        return None;
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
    Some(bins)
}

fn fog(bins: Option<[Bin; 11]>, report: &mut impl FnMut(&str, bool, String)) {
    let Some(bins) = bins else {
        report("fog", false, "no fog: run with --pov".into());
        return;
    };
    let inside = Bin::sum(&bins[..2]);
    let outside = Bin::sum(&bins[9..]);
    if !enough_ground(&bins) {
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

/// Logical pixels around a hero's body point its detail is sampled over.
const HERO_SAMPLE: f32 = 12.0;

/// Every hero the watched side sees, on screen and not under the
/// interface, shows detail where its body is drawn; a hero the side does not
/// see (the whole map's view says so) is not drawn at all.
fn heroes(shot: &Shot, c: &Checks, report: &mut impl FnMut(&str, bool, String)) {
    let Some(view) = c.session.0.view() else {
        return;
    };
    let mut checked = 0;
    for hero in view.units.iter().filter(|u| u.kind == UnitKind::Hero) {
        let drawn = c.index.0.get(&hero.id).and_then(|e| c.units.get(*e).ok());
        if c.fog.hides(hero.seen_by) {
            report(
                "a hero the side does not see is not drawn",
                drawn.is_none(),
                format!("{:?} hero, unseen by {:?}", hero.team, c.fog.team),
            );
            continue;
        }
        let Some(unit) = drawn else {
            continue;
        };
        let Some(lums) = hero_samples(shot, c, unit) else {
            continue;
        };
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

/// The luminance of the screen around a hero's body, none when the body is
/// off screen or under the interface (the minimap, the hero panel, ...),
/// whose pixels say nothing of the hero.
fn hero_samples(shot: &Shot, c: &Checks, unit: &Unit) -> Option<Vec<f32>> {
    let (camera, transform) = *c.camera;
    let scale = c.window.scale_factor();
    let world = Vec3::new(
        unit.at.x,
        c.ground.at(unit.at.x, unit.at.y) + 110.0,
        -unit.at.y,
    );
    let p = camera.world_to_viewport(transform, world).ok()?;
    // Interface nodes' centres and sizes are in physical pixels.
    let around = Rect::from_center_half_size(p * scale, Vec2::splat(HERO_SAMPLE * scale));
    let covered = c.hud.iter().any(|(node, at)| {
        let rect = Rect::from_center_size(at.translation, node.size());
        !rect.intersect(around).is_empty()
    });
    if covered {
        return None;
    }
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
    (lums.len() >= 50).then_some(lums)
}

/// A click near a unit, not on it, gets it: for every unit on screen with
/// no other close by, clicks a little off the middle of its body in four
/// directions (within the screen-space minimum reach) and asks the picker
/// what it gets. The clicks made and the misses; none when no unit on
/// screen stands alone (nothing to check: no fault of the picker).
fn clicks(c: &Checks) -> Option<(u32, Vec<String>)> {
    use crate::pick::{Candidate, Lens, MIN_SCREEN_RADIUS, Side, Want, pick, stands};
    let (camera, transform) = *c.camera;
    let view = c.session.0.view()?;
    let fov = match *c.projection {
        Projection::Perspective(p) => p.fov,
        _ => return None,
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
    (tried > 0).then_some((tried, wrong))
}

/// The river, unfogged, in the middle of the screen: no straight sharp step
/// in its shading. Water is told by its colour, near the middle's median
/// colour; in bands across the screen each column's (and each row's) mean
/// over the water, and how far one step of that profile rises above the
/// steps beside it: a seam between two pieces drawn apart steps the same way
/// all along, ripples and the light's gradients are smooth.
fn water(shot: &Shot, report: &mut impl FnMut(&str, bool, String)) {
    let (x0, y0) = (shot.w * 3 / 10, shot.h * 2 / 9);
    let (w, h) = (shot.w * 2 / 5, shot.h * 5 / 9);
    let at = |x: usize, y: usize| -> [f32; 3] {
        let i = ((y0 + y) * shot.w + x0 + x) * 4;
        let p = &shot.data[i..i + 3];
        let rgb = if shot.bgra {
            [p[2], p[1], p[0]]
        } else {
            [p[0], p[1], p[2]]
        };
        rgb.map(f32::from)
    };
    let median = |c: usize| {
        let mut hist = [0usize; 256];
        for y in (0..h).step_by(2) {
            for x in (0..w).step_by(2) {
                hist[at(x, y)[c] as usize] += 1;
            }
        }
        let half = hist.iter().sum::<usize>() / 2;
        let mut seen = 0;
        hist.iter()
            .position(|n| {
                seen += n;
                seen > half
            })
            .unwrap_or(0) as f32
    };
    let mid = [median(0), median(1), median(2)];
    let lum: Vec<Option<f32>> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .map(|(x, y)| {
            let c = at(x, y);
            let water = (0..3).all(|i| (c[i] - mid[i]).abs() < WATER_COLOUR_REACH);
            water.then(|| luminance(c))
        })
        .collect();
    let share = lum.iter().filter(|l| l.is_some()).count() as f32 / lum.len().max(1) as f32;
    if share < WATER_SHARE_MIN {
        report(
            "the river is one surface",
            false,
            format!(
                "{:.0} % of the middle is water (at least {:.0} %): the river is not on screen",
                share * 100.0,
                WATER_SHARE_MIN * 100.0
            ),
        );
        return;
    }
    let band = (h / 18).max(8);
    let mut sharpest = (0.0f32, String::new());
    for (across, along, name) in [(w, h, "vertical"), (h, w, "horizontal")] {
        for start in (0..=along.saturating_sub(band)).step_by(band / 2) {
            let profile: Vec<Option<f32>> = (0..across)
                .map(|i| {
                    let (mut sum, mut n) = (0.0, 0);
                    for j in start..start + band {
                        let (x, y) = if name == "vertical" { (i, j) } else { (j, i) };
                        if let Some(l) = lum[y * w + x] {
                            sum += l;
                            n += 1;
                        }
                    }
                    (n * 10 > band * 9).then(|| sum / n as f32)
                })
                .collect();
            if let Some((rise, i)) = sharpest_step(&profile)
                && rise > sharpest.0
            {
                sharpest = (
                    rise,
                    format!(
                        "{name} at {} px",
                        i + if name == "vertical" { x0 } else { y0 }
                    ),
                );
            }
        }
    }
    report(
        "the river is one surface",
        sharpest.0 <= WATER_STEP_MAX,
        format!(
            "sharpest straight step {:.2} (at most {WATER_STEP_MAX}), {}; {:.0} % of the middle is water",
            sharpest.0,
            sharpest.1,
            share * 100.0
        ),
    );
}

/// Colour distance (each channel, 0 to 255) from the middle's median within
/// which a pixel counts as water: lily pads, reeds, rocks and shadows are
/// left out.
const WATER_COLOUR_REACH: f32 = 14.0;

/// The step of a profile that rises most above the two steps beside it, and
/// where; steps next to a gap (not water) are not measured.
fn sharpest_step(profile: &[Option<f32>]) -> Option<(f32, usize)> {
    let steps: Vec<Option<f32>> = profile
        .windows(2)
        .map(|p| Some((p[1]? - p[0]?).abs()))
        .collect();
    steps
        .windows(3)
        .enumerate()
        .filter_map(|(i, s)| Some((s[1]? - (s[0]? + s[2]?) / 2.0, i + 1)))
        .max_by(|a, b| a.0.total_cmp(&b.0))
}
