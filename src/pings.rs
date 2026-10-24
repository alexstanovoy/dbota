//! Pings, as Dota has them: the ping binding (Alt+click) marks the spot or
//! the unit under the cursor for one's own side, Ctrl+Alt+click warns of
//! danger there. A heard ping marks the ground and the minimap for a few
//! seconds in its speaker's colour (a warning in red), a unit's following the
//! unit, and sounds once.

use bevy::prelude::*;
use bota_client_core::{App as Core, ChatLine, world_vec};
use bota_proto::{Body, EntityId, Heard, Ping, PingAt, PingKind, Speaker};

use crate::camera::CursorGround;
use crate::keys::{Action, Fired, Input as Bound};
use crate::map::Ground;
use crate::session::Session;
use crate::settings::Screen;
use crate::units::{Unit, UnitIndex};

/// Pings marked at most, on the map and on the minimap.
pub const MARKS: usize = 8;
/// Seconds a ping stays, the last [`FADE_SECS`] of them fading.
const PING_SECS: f32 = 3.0;
const FADE_SECS: f32 = 0.5;
/// Ticks in a second, for the minimap's swell.
pub const TICKS_PER_SECOND: f32 = 30.0;
/// World units the ground ring grows to, and the arrow's height.
const RING_REACH: f32 = 170.0;
const ARROW_HEIGHT: f32 = 300.0;
/// World units the marks float over the ground.
const LIFT: f32 = 10.0;
/// The warning's colour.
const DANGER: Color = Color::srgb(1.0, 0.18, 0.12);

/// A heard ping that still shows.
pub struct Mark {
    /// Where it points, in bota world units: the spot, or where the unit it
    /// names stands.
    pub at: Vec2,
    /// Seconds since it was heard.
    pub age: f32,
    pub danger: bool,
    speaker: Color,
}

impl Mark {
    /// The colour it is drawn in now: its speaker's, red for a warning,
    /// fading out at the end.
    pub fn color(&self) -> Color {
        let base = if self.danger { DANGER } else { self.speaker };
        base.with_alpha(((PING_SECS - self.age) / FADE_SECS).clamp(0.0, 1.0))
    }
}

/// The pings this client reads that still show, newest first: those of a
/// replay watched through a seat are that side's own. `drawn` says where a
/// unit is drawn this frame, which a ping at it follows; the view's place
/// stands in.
pub fn marks(core: &Core, drawn: impl Fn(EntityId) -> Option<Vec2>) -> Vec<Mark> {
    let Some(view) = core.view() else {
        return Vec::new();
    };
    core.chat_lines()
        .rev()
        .filter(|line| line.age < PING_SECS)
        .filter_map(|line| {
            let Body::Ping(ping) = &line.heard.line.body else {
                return None;
            };
            let Speaker::Seat(slot) = line.heard.from else {
                return None;
            };
            let at = match ping.at {
                PingAt::Spot(at) => Vec2::new(at.x.to_f32(), at.y.to_f32()),
                PingAt::Unit(id) => {
                    let place = view.unit(id)?.pos;
                    drawn(id).unwrap_or(Vec2::new(place.x.to_f32(), place.y.to_f32()))
                }
            };
            Some(Mark {
                at,
                age: line.age,
                danger: ping.kind == PingKind::Danger,
                speaker: crate::teleport::player_color(view, slot),
            })
        })
        .take(MARKS)
        .collect()
}

/// What each ping line the client has heard since the last call was, oldest
/// first: for the sound, which is made once per ping. `last` is the newest
/// line already looked at.
pub fn fresh(core: &Core, last: &mut Option<Heard>) -> Vec<PingKind> {
    let lines: Vec<&ChatLine> = core.chat_lines().collect();
    let newest = lines.last().map(|line| line.heard.clone());
    let from = last
        .as_ref()
        .and_then(|seen| lines.iter().rposition(|line| &line.heard == seen))
        .map_or(0, |at| at + 1);
    *last = newest.or(last.take());
    lines[from..]
        .iter()
        .filter(|line| line.age < PING_SECS)
        .filter_map(|line| match &line.heard.line.body {
            Body::Ping(Ping { kind, .. }) => Some(*kind),
            Body::Text(_) => None,
        })
        .collect()
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, draw);
}

/// The ping binding pressed this frame, a warning before a plain ping.
pub fn pressed(fired: &Fired) -> Option<PingKind> {
    if fired.pressed(Action::PingDanger) {
        Some(PingKind::Danger)
    } else if fired.pressed(Action::Ping) {
        Some(PingKind::Regular)
    } else {
        None
    }
}

/// Whether this client may ping: a seat of a live match.
pub fn may(core: &Core) -> bool {
    core.can_say() && core.my_slot.is_some()
}

/// Pings the spot or the unit under the cursor when a ping binding is
/// pressed over the world, and keeps that click from being a selection. A
/// click on the minimap is the minimap's.
pub fn send(
    mut session: NonSendMut<Session>,
    fired: Res<Fired>,
    settings: Res<crate::config::Settings>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    cursor: Res<CursorGround>,
    hover: Res<crate::pick::Hover>,
    over: Res<crate::input::OverHud>,
    screen: Res<Screen>,
) {
    let Some(kind) = pressed(&fired) else {
        return;
    };
    let action = match kind {
        PingKind::Danger => Action::PingDanger,
        PingKind::Regular => Action::Ping,
    };
    if over.0 || screen.open {
        return;
    }
    if let Some(Bound::Mouse(button)) = settings.key(action).map(|chord| chord.input) {
        mouse.clear_just_pressed(button);
    }
    let core = &mut session.0;
    if !may(core) {
        return;
    }
    let at = match (hover.select, cursor.0) {
        (Some((id, _)), _) => PingAt::Unit(id),
        (None, Some(ground)) => PingAt::Spot(world_vec((ground.x, -ground.z))),
        (None, None) => return,
    };
    core.ping(Ping { kind, at });
}

/// Draws every ping that still shows on the ground: rings swelling out from
/// its spot and an arrow over it, following the unit a ping names.
fn draw(
    mut gizmos: Gizmos,
    session: NonSend<Session>,
    ground: Res<Ground>,
    index: Res<UnitIndex>,
    units: Query<&Unit>,
) {
    let drawn = |id: EntityId| {
        let unit = units.get(*index.0.get(&id)?).ok()?;
        Some(unit.at)
    };
    for mark in marks(&session.0, drawn) {
        let color = mark.color();
        let base = Vec3::new(
            mark.at.x,
            ground.at(mark.at.x, mark.at.y) + LIFT,
            -mark.at.y,
        );
        let flat = Isometry3d::new(base, Quat::from_rotation_arc(Vec3::Z, Vec3::Y));
        for phase in [0.0, 0.5] {
            let swell = (mark.age * 1.5 + phase).fract();
            let ring = color.with_alpha(color.alpha() * (1.0 - swell));
            gizmos
                .circle(flat, 30.0 + (RING_REACH - 30.0) * swell, ring)
                .resolution(48);
        }
        // Dota's arrow: it points down at the spot and bobs.
        let bob = (mark.age * 6.0).sin() * 20.0;
        let tip = base + Vec3::Y * (60.0 + bob);
        let tail = tip + Vec3::Y * (ARROW_HEIGHT - 60.0);
        gizmos.line(tip, tail, color);
        for side in [Vec3::X, Vec3::Z] {
            gizmos.line(tip, tip + Vec3::Y * 45.0 + side * 35.0, color);
            gizmos.line(tip, tip + Vec3::Y * 45.0 - side * 35.0, color);
        }
    }
}
