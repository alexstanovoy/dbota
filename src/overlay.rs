//! Lines laid on the ground: selection, attack range, the three raze spots,
//! enemy tower reach and aim, who fights whom, cast telegraphs, order
//! markers, and the aim of a slot taken up.

use std::f32::consts::TAU;

use bevy::{gizmos::config::GizmoConfigStore, prelude::*};
use bota_client_core::{App as Core, RAZE_NEAR, REQUIEM, unit_under_cursor};
use bota_proto::{Aim, Order, Target, Team, UnitKind, UnitView};

use crate::camera::CursorGround;
use crate::config::{Action, Settings};
use crate::map::Ground;
use crate::session::Session;
use crate::units::{Unit, UnitIndex};

/// How far ahead each raze lands, near to far, in world units.
pub const RAZE_REACH: [f32; 3] = [200.0, 450.0, 700.0];
/// How wide a raze burns, in world units.
pub const RAZE_RADIUS: f32 = 250.0;
/// Seconds from a raze order to its burst.
const RAZE_CAST_POINT: f32 = 0.55;
/// Seconds from a requiem order to its release.
const REQUIEM_CAST_POINT: f32 = 1.67;
/// How far a requiem's lines fly, in world units.
const REQUIEM_REACH: f32 = 1000.0;
/// World units past its reach within which an enemy tower's ring shows.
const TOWER_WARNING: f32 = 650.0;
/// World units the lines float over the ground.
const LIFT: f32 = 8.0;
/// Ticks an order marker lasts.
const ORDER_TICKS: u32 = 15;

const GOOD: Color = Color::srgb(0.42, 0.95, 0.52);
const BAD: Color = Color::srgb(1.0, 0.27, 0.22);
const GOLDEN: Color = Color::srgb(1.0, 0.8, 0.3);
const SOUL_RED: Color = Color::srgb(1.0, 0.25, 0.2);
const SOUL_VIOLET: Color = Color::srgb(0.72, 0.42, 1.0);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, configure)
        .add_systems(Update, draw);
}

fn configure(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 2.5;
    // Rings follow the coarse ground height; a stronger bias keeps rocks
    // and roots from cutting them into dashes.
    config.depth_bias = -0.2;
    config.render_layers = bevy::camera::visibility::RenderLayers::layer(1);
}

/// What the drawing reads.
struct Ctx<'a, 'w, 's> {
    core: &'a Core,
    ground: &'a Ground,
    index: &'a UnitIndex,
    units: &'a Query<'w, 's, &'static Unit>,
}

impl Ctx<'_, '_, '_> {
    /// Where a unit is drawn this frame.
    fn at(&self, id: bota_proto::EntityId) -> Option<&Unit> {
        self.index.0.get(&id).and_then(|e| self.units.get(*e).ok())
    }

    fn point(&self, x: f32, y: f32) -> Vec3 {
        Vec3::new(x, self.ground.at(x, y) + LIFT, -y)
    }
}

/// A circle on the ground, solid or dashed.
fn ring(
    gizmos: &mut Gizmos,
    ctx: &Ctx<'_, '_, '_>,
    (x, y): (f32, f32),
    r: f32,
    color: Color,
    dashed: bool,
) {
    let steps = ((r / 14.0) as usize).clamp(24, 160);
    let point = |i: usize| {
        let a = i as f32 / steps as f32 * TAU;
        ctx.point(x + r * a.cos(), y + r * a.sin())
    };
    if dashed {
        for i in (0..steps).step_by(2) {
            gizmos.line(point(i), point(i + 1), color);
        }
    } else {
        gizmos.linestrip((0..=steps).map(point), color);
    }
}

fn draw(
    mut gizmos: Gizmos,
    session: NonSend<Session>,
    ground: Res<Ground>,
    index: Res<UnitIndex>,
    units: Query<&'static Unit>,
    settings: Res<Settings>,
    keys: Res<ButtonInput<KeyCode>>,
    cursor: Res<CursorGround>,
) {
    let core = &session.0;
    let Some(view) = &core.view else {
        return;
    };
    let ctx = Ctx {
        core,
        ground: &ground,
        index: &index,
        units: &units,
    };
    let show_all = keys.pressed(settings.key(Action::ShowRanges));
    picks(&mut gizmos, &ctx);
    if let Some(me) = core
        .ears()
        .and_then(|id| view.units.iter().find(|u| u.id == id))
    {
        own_ranges(&mut gizmos, &ctx, me, &settings, show_all);
        tower_rings(&mut gizmos, &ctx, me, &settings, show_all);
    }
    tower_shots(&mut gizmos, &ctx);
    aggro(&mut gizmos, &ctx);
    casts(&mut gizmos, &ctx);
    orders(&mut gizmos, &ctx);
    aim(&mut gizmos, &ctx, cursor.0);
}

/// Rings at the feet of one's own hero, of what is picked, and of what the
/// camera follows.
fn picks(gizmos: &mut Gizmos, ctx: &Ctx<'_, '_, '_>) {
    let core = ctx.core;
    if let Some(me) = core.my_hero().and_then(|id| ctx.at(id)) {
        ring(
            gizmos,
            ctx,
            (me.at.x, me.at.y),
            52.0,
            GOOD.with_alpha(0.8),
            false,
        );
    }
    if let Some(picked) = core
        .selected
        .filter(|id| Some(*id) != core.my_hero())
        .and_then(|id| ctx.at(id))
    {
        ring(gizmos, ctx, (picked.at.x, picked.at.y), 56.0, GOLDEN, false);
    }
}

/// The attack range ring and the three raze spots of the watched hero.
fn own_ranges(
    gizmos: &mut Gizmos,
    ctx: &Ctx<'_, '_, '_>,
    me: &UnitView,
    settings: &Settings,
    show_all: bool,
) {
    let Some(body) = ctx.at(me.id) else {
        return;
    };
    let (x, y) = (body.at.x, body.at.y);
    let aiming = ctx.core.attack_move_armed;
    if settings.aids.attack_range || show_all || aiming {
        let reach = me.attack_range.to_f32() + me.bound.to_f32();
        let a = if show_all || aiming { 0.6 } else { 0.22 };
        ring(
            gizmos,
            ctx,
            (x, y),
            reach,
            Color::WHITE.with_alpha(a),
            false,
        );
    }
    if !(settings.aids.raze_marks || show_all) {
        return;
    }
    for ability in &me.abilities {
        let which = usize::from(ability.id.0.wrapping_sub(RAZE_NEAR));
        if which >= 3 || ability.level == 0 {
            continue;
        }
        let ready = ability.cooldown_left == 0 && me.mana >= ability.mana_cost;
        if !ready && !show_all {
            continue;
        }
        let a = if show_all { 0.7 } else { 0.22 };
        let spot = raze_spot(x, y, body.facing, which);
        ring(
            gizmos,
            ctx,
            spot,
            RAZE_RADIUS,
            SOUL_RED.with_alpha(a),
            false,
        );
    }
}

/// Where raze `which` lands from a caster at `(x, y)` facing `facing`.
pub fn raze_spot(x: f32, y: f32, facing: f32, which: usize) -> (f32, f32) {
    let reach = RAZE_REACH[which.min(2)];
    (x + facing.cos() * reach, y + facing.sin() * reach)
}

/// The reach of every enemy tower the watched hero is near, solid when the
/// tower is set on it.
fn tower_rings(
    gizmos: &mut Gizmos,
    ctx: &Ctx<'_, '_, '_>,
    me: &UnitView,
    settings: &Settings,
    show_all: bool,
) {
    if !settings.aids.tower_ranges && !show_all {
        return;
    }
    let Some(view) = &ctx.core.view else {
        return;
    };
    let Some(my_body) = ctx.at(me.id) else {
        return;
    };
    for tower in view
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Tower && u.team != me.team)
    {
        let Some(body) = ctx.at(tower.id) else {
            continue;
        };
        let reach = tower.attack_range.to_f32() + tower.bound.to_f32() + me.bound.to_f32();
        let far = body.at.distance(my_body.at);
        if far > reach + TOWER_WARNING && !show_all {
            continue;
        }
        let on_me = targets(ctx.core, tower.id, me.id);
        let closeness = 1.0 - ((far - reach).max(0.0) / TOWER_WARNING).min(1.0);
        if on_me {
            ring(gizmos, ctx, (body.at.x, body.at.y), reach, BAD, false);
        } else {
            let a = 0.25 + 0.5 * closeness;
            ring(
                gizmos,
                ctx,
                (body.at.x, body.at.y),
                reach,
                BAD.with_alpha(a),
                false,
            );
        }
    }
}

/// Whether one unit is set on another, by the aggro list.
pub fn targets(core: &Core, unit: bota_proto::EntityId, on: bota_proto::EntityId) -> bool {
    core.view.as_ref().is_some_and(|view| {
        view.aggro
            .binary_search_by_key(&unit, |a| a.unit)
            .ok()
            .is_some_and(|at| view.aggro[at].target == Some(on))
    })
}

/// A line from every tower to what it is set on, red when that is the
/// watched hero.
fn tower_shots(gizmos: &mut Gizmos, ctx: &Ctx<'_, '_, '_>) {
    let Some(view) = &ctx.core.view else {
        return;
    };
    let me = ctx.core.ears();
    for entry in &view.aggro {
        let Some(target) = entry.target else {
            continue;
        };
        let (Some(from), Some(to)) = (ctx.at(entry.unit), ctx.at(target)) else {
            continue;
        };
        if from.kind != UnitKind::Tower {
            continue;
        }
        let top = ctx.point(from.at.x, from.at.y) + Vec3::Y * 330.0;
        let end = ctx.point(to.at.x, to.at.y) + Vec3::Y * 60.0;
        let color = if Some(target) == me {
            BAD
        } else {
            team_color(from.team).with_alpha(0.6)
        };
        gizmos.line(top, end, color);
    }
}

/// Lines from whatever fights the picked unit or the watched hero, red when
/// a foe winds up a swing at it.
fn aggro(gizmos: &mut Gizmos, ctx: &Ctx<'_, '_, '_>) {
    let core = ctx.core;
    let Some(view) = &core.view else {
        return;
    };
    let Some(focus) = core.commanded().or(core.ears()) else {
        return;
    };
    for entry in &view.aggro {
        let Some(target) = entry.target else {
            continue;
        };
        if entry.unit != focus && target != focus {
            continue;
        }
        let (Some(from), Some(to)) = (ctx.at(entry.unit), ctx.at(target)) else {
            continue;
        };
        if from.kind == UnitKind::Tower {
            continue;
        }
        let swinging = entry.swing.is_some_and(|s| s.target == target);
        let against = target == focus && from.team != to.team;
        let color = match (against, swinging) {
            (true, true) => BAD,
            (true, false) => BAD.with_alpha(0.45),
            (false, _) => team_color(from.team).with_alpha(if swinging { 0.8 } else { 0.35 }),
        };
        gizmos.line(
            ctx.point(from.at.x, from.at.y),
            ctx.point(to.at.x, to.at.y),
            color,
        );
    }
}

/// The telegraphs of every cast point under way: the spot a raze is about to
/// burn filling up, the requiem's reach closing in.
fn casts(gizmos: &mut Gizmos, ctx: &Ctx<'_, '_, '_>) {
    let core = ctx.core;
    let Some(view) = &core.view else {
        return;
    };
    let rate = f32::from(core.tick_rate.max(1));
    for u in &view.units {
        let Some(began) = core.derived.cast_began(u.id) else {
            continue;
        };
        let Some(body) = ctx.at(u.id) else {
            continue;
        };
        let elapsed = (view.tick.saturating_sub(began) as f32 + core.smooth.alpha()) / rate;
        match cast_ability(core, u) {
            Some(id) if (RAZE_NEAR..RAZE_NEAR + 3).contains(&id) => {
                let k = (elapsed / RAZE_CAST_POINT).clamp(0.0, 1.0);
                let spot = raze_spot(
                    body.at.x,
                    body.at.y,
                    body.facing,
                    usize::from(id - RAZE_NEAR),
                );
                ring(gizmos, ctx, spot, RAZE_RADIUS, SOUL_RED, false);
                for f in [0.25, 0.5, 0.75, 1.0] {
                    if f <= k {
                        ring(
                            gizmos,
                            ctx,
                            spot,
                            RAZE_RADIUS * f * k,
                            SOUL_RED.with_alpha(0.5),
                            false,
                        );
                    }
                }
            }
            Some(REQUIEM) => {
                let k = (elapsed / REQUIEM_CAST_POINT).clamp(0.0, 1.0);
                let at = (body.at.x, body.at.y);
                ring(gizmos, ctx, at, REQUIEM_REACH, SOUL_VIOLET, true);
                ring(
                    gizmos,
                    ctx,
                    at,
                    REQUIEM_REACH * (1.0 - k).max(0.05),
                    SOUL_RED,
                    false,
                );
            }
            _ => {}
        }
    }
}

/// Which ability a unit in a cast point is casting, by the latest order its
/// seat is known to have given it.
pub fn cast_ability(core: &Core, u: &UnitView) -> Option<u16> {
    let owner = u.owner?;
    let shown = core
        .shown_orders
        .iter()
        .find(|s| s.slot == owner && s.unit.is_none_or(|id| id == u.id))?;
    match shown.order {
        Order::Cast { slot, .. } => u.abilities.get(usize::from(slot.0)).map(|a| a.id.0),
        _ => None,
    }
}

/// A shrinking ring where each known order was aimed.
fn orders(gizmos: &mut Gizmos, ctx: &Ctx<'_, '_, '_>) {
    let core = ctx.core;
    let Some(view) = &core.view else {
        return;
    };
    for shown in &core.shown_orders {
        let age = view.tick.saturating_sub(shown.tick);
        if age > ORDER_TICKS {
            continue;
        }
        let (target, color) = match shown.order {
            Order::Move { target } => (target, GOOD),
            Order::Attack { target } => (target, BAD),
            _ => continue,
        };
        let fade = 1.0 - (age as f32 + core.smooth.alpha()) / ORDER_TICKS as f32;
        let spot = match target {
            Target::Pos(p) => Some((p.x.to_f32(), p.y.to_f32(), 0.0)),
            Target::Unit(id) => ctx.at(id).map(|u| (u.at.x, u.at.y, 50.0)),
            Target::None => None,
        };
        if let Some((x, y, body)) = spot {
            ring(
                gizmos,
                ctx,
                (x, y),
                body + 18.0 + 30.0 * fade,
                color.with_alpha(fade.max(0.0)),
                false,
            );
        }
    }
}

/// The reach of a slot taken up for aiming and what the cursor is on.
fn aim(gizmos: &mut Gizmos, ctx: &Ctx<'_, '_, '_>, cursor: Option<Vec3>) {
    let core = ctx.core;
    let Some(slot) = core.aiming else {
        return;
    };
    let Some(caster) = core.commanded().and_then(|id| ctx.at(id)) else {
        return;
    };
    let reach = core.reach_of(slot) as f32;
    let aim = core.aim_in(slot);
    if reach > 0.0 && aim != Some(Aim::Building) {
        ring(
            gizmos,
            ctx,
            (caster.at.x, caster.at.y),
            reach,
            Color::srgb(0.45, 0.85, 1.0),
            false,
        );
    }
    let Some(cursor) = cursor else {
        return;
    };
    let (wx, wy) = (cursor.x, -cursor.z);
    let within = reach <= 0.0 || caster.at.distance(Vec2::new(wx, wy)) <= reach;
    let color = if within { GOOD } else { BAD };
    if aim == Some(Aim::Unit)
        && let Some(view) = &core.view
        && let Some(mark) = unit_under_cursor(view, (wx, wy), None, true).and_then(|id| ctx.at(id))
    {
        ring(gizmos, ctx, (mark.at.x, mark.at.y), 60.0, color, false);
        return;
    }
    ring(gizmos, ctx, (wx, wy), 24.0, color, false);
}

/// The colour of a side.
pub fn team_color(team: Team) -> Color {
    match team {
        Team::Radiant => Color::srgb(0.35, 0.85, 0.42),
        Team::Dire => Color::srgb(0.93, 0.32, 0.26),
        Team::Neutral => Color::srgb(0.8, 0.7, 0.46),
    }
}
