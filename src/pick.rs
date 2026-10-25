//! What the cursor points at in the world, the way Dota forgives: every unit
//! stands in a generous upright cylinder, flat bottom on the ground (its body's bound, a margin, never less than the rings dbota draws under a unit, and a
//! screen-space minimum so a creep far away stays easy to hit), the cursor's
//! ray is tested against those few cylinders and no mesh, and among the
//! overlapping ones the action in hand decides: a click that attacks takes an
//! enemy before a friend, an ability on enemies never takes a friend.
//!
//! The click, the highlight and the cursor shape all read [`Hover`], so what
//! is lit is what a click gets.

use bevy::{prelude::*, window::PrimaryWindow};
use bota_client_core::{Slot, Source};
use bota_proto::{Aim, EntityId, Team, UnitKind, WorldView};

use crate::camera::RtsCamera;
use crate::input::OverHud;
use crate::map::Ground;
use crate::session::Session;
use crate::units::{Unit, UnitIndex, stand_in_size};

/// How far past a body's bound a click still lands, in world units.
const MARGIN: f32 = 15.0;
/// The smallest reach of a cylinder, in world units, before the margin.
const MIN_BOUND: f32 = 20.0;
/// The smallest radius a cylinder shows on screen, as a fraction of the
/// window's height: about 24 pixels at 1080p.
pub const MIN_SCREEN_RADIUS: f32 = 0.022;
/// Height of a cylinder over the bound, at least, so a tall neutral is not
/// shorter than it looks.
const HEIGHT_PER_BOUND: f32 = 2.2;
/// The biggest ring dbota draws on the ground under a unit (the aim mark; the
/// selection ring is the body's bound and a little more), so a click
/// anywhere in or on a ring picks the unit.
pub const RING: f32 = 60.0;

/// The hover ring's colour: Dota's yellow ring under the unit a click would get.
const HOVER_RING: Color = Color::srgba(1.0, 0.85, 0.2, 0.9);

/// A world unit more than the radius still hits, so the very edge of a ring
/// is inside whatever the rounding.
const EDGE_SLACK: f32 = 1.0;

/// The radius of a unit's cylinder.
pub fn radius_of(bound: f32, collision: f32) -> f32 {
    (bound.max(collision).max(MIN_BOUND) + MARGIN).max(RING)
}

/// Which side of the cursor's owner a target must be on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Any,
    Enemy,
    Ally,
    /// No unit: the action takes the ground.
    Nobody,
}

/// What a click is after.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Want {
    /// The side asked for.
    pub side: Side,
    /// Whether another side never matches, rather than losing to the asked one.
    pub strict: bool,
    /// A unit that never matches.
    pub exclude: Option<EntityId>,
    /// Whether the click gives orders, which fountains do not take.
    pub orders: bool,
}

/// One unit as a volume: an upright cylinder from `base` up by `height`.
#[derive(Clone, Copy, Debug)]
pub struct Candidate {
    pub id: EntityId,
    pub kind: UnitKind,
    pub enemy: bool,
    /// Where it stands, in Bevy coordinates.
    pub base: Vec3,
    pub radius: f32,
    pub height: f32,
}

/// How wide the screen's minimum radius is in the world: it grows with
/// distance from the camera.
#[derive(Clone, Copy, Debug)]
pub struct Lens {
    /// World units across the minimum radius, per unit of depth.
    pub min_per_depth: f32,
}

impl Lens {
    /// The lens of a camera with this vertical field of view; the minimum is a
    /// fraction of the window's height, so it holds at any resolution.
    pub fn new(fov_y: f32) -> Lens {
        Lens {
            min_per_depth: MIN_SCREEN_RADIUS * 2.0 * (fov_y / 2.0).tan(),
        }
    }
}

/// A unit under the cursor and where it stands, in bota coordinates.
pub type Hit = Option<(EntityId, Vec2)>;

/// What the left button does now.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Left {
    #[default]
    Select,
    /// Attack-move is armed.
    Attack,
    /// A slot is taken up for aiming.
    Aim,
}

/// What the cursor is over this frame, by what a click would do with it.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct Hover {
    /// What a plain left click picks.
    pub select: Hit,
    /// What a right click, or an armed attack-move, targets.
    pub attack: Hit,
    /// What a slot taken up for aiming targets.
    pub aim: Hit,
    pub left: Left,
}

impl Hover {
    /// What the left button takes.
    pub fn left(&self) -> Hit {
        match self.left {
            Left::Select => self.select,
            Left::Attack => self.attack,
            Left::Aim => self.aim,
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Hover>()
        .add_systems(Update, draw_hover.after(update));
}

/// The volume of one unit standing at a Bevy point.
pub fn candidate(unit: &bota_proto::UnitView, base: Vec3, mine: Option<Team>) -> Candidate {
    let bound = unit
        .bound
        .to_f32()
        .max(unit.collision.to_f32())
        .max(MIN_BOUND);
    let (_, height) = stand_in_size(unit.kind, unit.bound.to_f32());
    Candidate {
        id: unit.id,
        kind: unit.kind,
        enemy: Some(unit.team) != mine,
        base,
        radius: radius_of(unit.bound.to_f32(), unit.collision.to_f32()),
        height: height.max(bound * HEIGHT_PER_BOUND),
    }
}

/// How near a ray comes to the axis of an upright cylinder while inside its
/// height, measured across, and how far along the ray that is. Nothing when
/// the ray never is between the cylinder's bottom and top.
fn ray_to_cylinder(ray: Ray3d, base: Vec3, height: f32) -> Option<(f32, f32)> {
    let (o, v) = (ray.origin, *ray.direction);
    let (mut lo, mut hi) = (0.0, f32::INFINITY);
    if v.y.abs() < 1e-6 {
        if o.y < base.y || o.y > base.y + height {
            return None;
        }
    } else {
        let (a, b) = ((base.y - o.y) / v.y, (base.y + height - o.y) / v.y);
        lo = a.min(b).max(0.0);
        hi = a.max(b);
        if hi < lo {
            return None;
        }
    }
    let w = Vec2::new(o.x - base.x, o.z - base.z);
    let u = Vec2::new(v.x, v.z);
    let along = u.length_squared();
    let t = if along > 1e-9 {
        (-w.dot(u) / along).clamp(lo, hi)
    } else {
        lo
    };
    Some(((w + u * t).length(), t))
}

/// Whether a candidate may be taken for this want, and whether it is the side
/// asked for.
fn admits(c: &Candidate, want: &Want) -> Option<bool> {
    if Some(c.id) == want.exclude || (want.orders && c.kind == UnitKind::Fountain) {
        return None;
    }
    let liked = match want.side {
        Side::Any => true,
        Side::Enemy => c.enemy,
        Side::Ally => !c.enemy,
        Side::Nobody => return None,
    };
    (liked || !want.strict).then_some(liked)
}

/// The unit the ray points at: among those whose cylinder it passes through,
/// one of the side asked for before any other, then the nearest to the ray's
/// line, then the nearest to the camera.
pub fn pick(
    ray: Ray3d,
    lens: Lens,
    candidates: impl Iterator<Item = Candidate>,
    want: &Want,
) -> Option<EntityId> {
    let mut best: Option<((bool, f32, f32), EntityId)> = None;
    for c in candidates {
        let Some(liked) = admits(&c, want) else {
            continue;
        };
        let Some((dist, depth)) = ray_to_cylinder(ray, c.base, c.height) else {
            continue;
        };
        if dist > c.radius.max(lens.min_per_depth * depth) + EDGE_SLACK {
            continue;
        }
        let key = (!liked, dist, depth);
        if best.is_none_or(|(b, _)| key < b) {
            best = Some((key, c.id));
        }
    }
    best.map(|(_, id)| id)
}

/// What each kind of click wants right now: selecting, ordering, and
/// aiming, and what the left button does.
fn wants(core: &bota_client_core::App) -> (Want, Want, Want, Left) {
    let mine = core.my_team();
    let commands = core.my_hero().is_some()
        && core.controls_selection()
        && core.over.is_none()
        && matches!(core.source, Source::Live(_));
    let select = Want {
        side: Side::Any,
        strict: false,
        exclude: None,
        orders: false,
    };
    let attack = Want {
        side: if mine.is_some() {
            Side::Enemy
        } else {
            Side::Any
        },
        strict: false,
        exclude: core.my_hero(),
        orders: true,
    };
    if !commands {
        return (select, select, select, Left::Select);
    }
    let Some(slot) = core.aiming else {
        let left = if core.attack_move_armed {
            Left::Attack
        } else {
            Left::Select
        };
        return (select, attack, attack, left);
    };
    let aim = match core.aim_in(slot) {
        Some(Aim::Unit) => Want {
            side: if at_an_enemy(core, slot) {
                Side::Enemy
            } else {
                Side::Ally
            },
            strict: true,
            exclude: None,
            orders: true,
        },
        // Aimed at ground or a tree: no unit is the target.
        _ => Want {
            side: Side::Nobody,
            ..attack
        },
    };
    (select, attack, aim, Left::Aim)
}

/// Whether the ability in a slot is cast at enemies; an item at a unit is
/// used on a friend.
fn at_an_enemy(core: &bota_client_core::App, slot: Slot) -> bool {
    match slot {
        Slot::Ability(at) => core
            .slot_unit()
            .and_then(|u| u.abilities.get(usize::from(at)))
            .and_then(|a| bota_core::ability_def(a.id))
            .is_some_and(|def| def.at_an_enemy),
        Slot::Item(_) => false,
    }
}

pub fn stands(
    view: &WorldView,
    index: &UnitIndex,
    units: &Query<&Unit>,
    ground: &Ground,
    mine: Option<Team>,
) -> impl Iterator<Item = (Candidate, Vec2)> {
    view.units.iter().map(move |u| {
        let drawn = index
            .0
            .get(&u.id)
            .and_then(|e| units.get(*e).ok())
            .map_or(Vec2::new(u.pos.x.to_f32(), u.pos.y.to_f32()), |d| d.at);
        let base = Vec3::new(drawn.x, ground.at(drawn.x, drawn.y), -drawn.y);
        (
            candidate(u, base, mine),
            Vec2::new(u.pos.x.to_f32(), u.pos.y.to_f32()),
        )
    })
}

/// Works out what the cursor is over for this frame.
pub fn update(
    session: NonSend<Session>,
    over: Res<OverHud>,
    screen: Res<crate::settings::Screen>,
    ground: Res<Ground>,
    index: Res<UnitIndex>,
    units: Query<&Unit>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform, &Projection), With<RtsCamera>>,
    mut hover: ResMut<Hover>,
) {
    *hover = Hover::default();
    let core = &session.0;
    let (Some(view), Some(cursor)) = (core.view(), window.cursor_position()) else {
        return;
    };
    if over.0 || screen.open {
        return;
    }
    let (camera, transform, projection) = *camera;
    let (Ok(ray), Projection::Perspective(lens)) =
        (camera.viewport_to_world(transform, cursor), projection)
    else {
        return;
    };
    let lens = Lens::new(lens.fov);
    let (select, attack, aim, left) = wants(core);
    let all: Vec<(Candidate, Vec2)> =
        stands(view, &index, &units, &ground, core.my_team()).collect();
    let find = |want: &Want| {
        pick(ray, lens, all.iter().map(|(c, _)| *c), want).and_then(|id| {
            all.iter()
                .find(|(c, _)| c.id == id)
                .map(|(c, at)| (c.id, *at))
        })
    };
    *hover = Hover {
        select: find(&select),
        attack: find(&attack),
        aim: find(&aim),
        left,
    };
}

/// Dota's ring under what a click would get: yellow, round the unit's feet
/// for as long as the cursor is on it.
fn draw_hover(
    hover: Res<Hover>,
    session: NonSend<Session>,
    ground: Res<Ground>,
    mut gizmos: Gizmos,
) {
    let Some((id, at)) = hover.left() else {
        return;
    };
    let Some(unit) = session.0.view().and_then(|v| v.unit(id)) else {
        return;
    };
    let centre = Vec3::new(at.x, ground.at(at.x, at.y) + 3.0, -at.y);
    gizmos.circle(
        Isometry3d::new(centre, Quat::from_rotation_arc(Vec3::Z, Vec3::Y)),
        ring_radius(unit),
        HOVER_RING,
    );
}

/// The radius of the rings at a unit's feet: its body's, a little out.
pub fn ring_radius(unit: &bota_proto::UnitView) -> f32 {
    unit.bound
        .to_f32()
        .max(unit.collision.to_f32())
        .max(MIN_BOUND)
        + 8.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const FOV: f32 = 25.0 * std::f32::consts::PI / 180.0;
    const SIZES: [(f32, f32); 3] = [(1280.0, 720.0), (1920.0, 1080.0), (3840.0, 2160.0)];
    const DISTANCES: [f32; 3] = [1600.0, 4400.0, 6500.0];

    fn id(n: u32) -> EntityId {
        EntityId {
            idx: n,
            generation: 0,
        }
    }

    fn at(n: u32, kind: UnitKind, enemy: bool, x: f32, z: f32) -> Candidate {
        Candidate {
            id: id(n),
            kind,
            enemy,
            base: Vec3::new(x, 0.0, z),
            radius: radius_of(24.0, 0.0),
            height: 90.0,
        }
    }

    /// The camera the game uses, looking at the origin from a distance.
    fn camera(distance: f32) -> Transform {
        let rotation = crate::camera::camera_rotation(58.0);
        Transform::from_translation(-(rotation * Vec3::NEG_Z) * distance).with_rotation(rotation)
    }

    /// The ray through a pixel of a window.
    fn ray_at(cam: &Transform, (w, h): (f32, f32), px: Vec2) -> Ray3d {
        let half = (FOV / 2.0).tan();
        let ndc = Vec2::new(px.x / w * 2.0 - 1.0, 1.0 - px.y / h * 2.0);
        let local = Vec3::new(ndc.x * half * w / h, ndc.y * half, -1.0);
        Ray3d::new(cam.translation, Dir3::new(cam.rotation * local).unwrap())
    }

    /// The pixel a world point shows at.
    fn pixel_of(cam: &Transform, (w, h): (f32, f32), p: Vec3) -> Vec2 {
        let local = cam.rotation.inverse() * (p - cam.translation);
        let half = (FOV / 2.0).tan();
        let ndc = Vec2::new(
            local.x / (-local.z * half * w / h),
            local.y / (-local.z * half),
        );
        Vec2::new((ndc.x + 1.0) / 2.0 * w, (1.0 - ndc.y) / 2.0 * h)
    }

    fn pick_at(
        cam: &Transform,
        size: (f32, f32),
        px: Vec2,
        cands: &[Candidate],
        want: &Want,
    ) -> Option<EntityId> {
        pick(
            ray_at(cam, size, px),
            Lens::new(FOV),
            cands.iter().copied(),
            want,
        )
    }

    const ANY: Want = Want {
        side: Side::Any,
        strict: false,
        exclude: None,
        orders: false,
    };

    #[test]
    fn a_click_near_a_creep_gets_it_at_every_zoom_and_resolution() {
        let creep = [at(1, UnitKind::CreepMelee, true, 300.0, -200.0)];
        for size in SIZES {
            for distance in DISTANCES {
                let cam = camera(distance);
                let body = pixel_of(&cam, size, creep[0].base + Vec3::Y * 45.0);
                // A little more than a tenth of a percent of the window short
                // of the minimum reach, in each direction.
                let slack = size.1 * 0.017;
                for off in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                    let click = body + Vec2::new(off.0, off.1) * slack;
                    assert_eq!(
                        pick_at(&cam, size, click, &creep, &ANY),
                        Some(id(1)),
                        "{size:?} at {distance}, offset {off:?}"
                    );
                }
                let far = pixel_of(
                    &cam,
                    size,
                    creep[0].base + Vec3::X * RING * 2.5 + Vec3::Y * 45.0,
                );
                assert_eq!(pick_at(&cam, size, far, &creep, &ANY), None);
            }
        }
    }

    #[test]
    fn a_click_on_or_in_the_ring_under_a_unit_picks_it_at_every_zoom() {
        // Ring points all round, on the ground, at the biggest ring drawn and
        // inside it: the flat bottom of the cylinder loses none.
        let creep = [at(1, UnitKind::CreepMelee, true, -400.0, 250.0)];
        for size in SIZES {
            for distance in DISTANCES {
                let cam = camera(distance);
                for step in 0..24 {
                    let a = step as f32 / 24.0 * std::f32::consts::TAU;
                    for r in [RING, RING * 0.5, 0.0] {
                        let spot = creep[0].base + Vec3::new(a.cos(), 0.0, a.sin()) * r;
                        let click = pixel_of(&cam, size, spot);
                        assert_eq!(
                            pick_at(&cam, size, click, &creep, &ANY),
                            Some(id(1)),
                            "{size:?} at {distance}, angle {a}, radius {r}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_body_above_the_feet_is_clickable() {
        let creep = [at(1, UnitKind::CreepRanged, true, 0.0, 0.0)];
        let cam = camera(1600.0);
        let size = SIZES[1];
        for h in [5.0, 45.0, 85.0] {
            let click = pixel_of(&cam, size, creep[0].base + Vec3::Y * h);
            assert_eq!(pick_at(&cam, size, click, &creep, &ANY), Some(id(1)));
        }
    }

    #[test]
    fn an_attack_prefers_an_enemy_and_an_ability_never_takes_the_wrong_side() {
        // A friend stands exactly on the cursor's line, an enemy a hand away.
        let units = [
            at(1, UnitKind::CreepMelee, false, 0.0, 0.0),
            at(2, UnitKind::CreepMelee, true, 30.0, -20.0),
        ];
        let cam = camera(4400.0);
        let size = SIZES[1];
        let click = pixel_of(&cam, size, units[0].base + Vec3::Y * 45.0);
        let want = |side, strict| Want {
            side,
            strict,
            exclude: None,
            orders: true,
        };
        assert_eq!(pick_at(&cam, size, click, &units, &ANY), Some(id(1)));
        assert_eq!(
            pick_at(&cam, size, click, &units, &want(Side::Enemy, false)),
            Some(id(2))
        );
        assert_eq!(
            pick_at(&cam, size, click, &units, &want(Side::Enemy, true)),
            Some(id(2))
        );
        assert_eq!(
            pick_at(&cam, size, click, &units, &want(Side::Ally, true)),
            Some(id(1))
        );
        // Only a friend near: the soft want still takes it, the strict one does not.
        let alone = &units[..1];
        assert_eq!(
            pick_at(&cam, size, click, alone, &want(Side::Enemy, false)),
            Some(id(1))
        );
        assert_eq!(
            pick_at(&cam, size, click, alone, &want(Side::Enemy, true)),
            None
        );
        assert_eq!(
            pick_at(&cam, size, click, alone, &want(Side::Nobody, true)),
            None
        );
    }

    #[test]
    fn among_equals_the_nearest_to_the_cursor_wins_and_fountains_take_no_orders() {
        let units = [
            at(1, UnitKind::CreepMelee, true, 0.0, 0.0),
            at(2, UnitKind::CreepMelee, true, 60.0, 0.0),
            at(3, UnitKind::Fountain, true, 0.0, 0.0),
        ];
        let cam = camera(1600.0);
        let size = SIZES[1];
        let near_second = pixel_of(&cam, size, Vec3::new(45.0, 45.0, 0.0));
        let selecting = Want {
            side: Side::Enemy,
            ..ANY
        };
        assert_eq!(
            pick_at(&cam, size, near_second, &units[..2], &selecting),
            Some(id(2))
        );
        let on_first = pixel_of(&cam, size, Vec3::new(0.0, 45.0, 0.0));
        let ordering = Want {
            orders: true,
            exclude: Some(id(1)),
            ..selecting
        };
        assert_eq!(pick_at(&cam, size, on_first, &units[..1], &ordering), None);
        assert_eq!(pick_at(&cam, size, on_first, &units[2..], &ordering), None);
        assert_eq!(
            pick_at(&cam, size, on_first, &units[2..], &selecting),
            Some(id(3))
        );
    }
}
