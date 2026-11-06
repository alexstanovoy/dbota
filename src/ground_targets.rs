//! The rune or the item lying on the ground that a right click would take, lit as a unit under the
//! cursor is: Dota's yellow ring on it, so that the click is no guess. What a click takes, and
//! whether it gives an order at all, is decided in `bota_client_core` (`lying_at` and
//! `gives_orders`, which the smart click reads too); this asks the same two of the same view.

use bevy::prelude::*;
use bota_client_core::Lying;

use crate::camera::CursorGround;
use crate::input::OverHud;
use crate::map::Ground;
use crate::session::Session;

/// Extra radius of the ring beyond the reach a click has, in world units.
const RING_MARGIN: f32 = 6.0;

/// What a right click on the ground under the cursor would take, with the ring to draw: its
/// handle, where it lies (bota's units) and how near a click had to be.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq)]
pub struct GroundTarget(pub Option<Lying>);

pub fn plugin(app: &mut App) {
    app.init_resource::<GroundTarget>()
        .add_systems(Update, find.after(crate::pick::update))
        .add_systems(PostUpdate, draw.before(bevy::ui::UiSystems::Prepare));
}

/// Finds the target of this frame: only where a right click takes the ground, so not over the
/// interface or the settings, not on a unit, and where the seat's click gives orders at all (the
/// core's own gate: not in a replay, not with an enemy picked, not once the match is over).
/// Aiming or holding an item does not hide it: the right click cancels the aim and takes it.
fn find(
    session: NonSend<Session>,
    cursor: Res<CursorGround>,
    hover: Res<crate::pick::Hover>,
    over: Res<OverHud>,
    screen: Res<crate::settings::Screen>,
    mut target: ResMut<GroundTarget>,
) {
    let core = &session.0;
    let now = match cursor.0 {
        Some(ground)
            if !over.0 && !screen.open && hover.attack.is_none() && core.gives_orders() =>
        {
            core.lying_at((ground.x, -ground.z))
        }
        _ => None,
    };
    if target.0 != now {
        target.0 = now;
    }
}

fn draw(target: Res<GroundTarget>, ground: Res<Ground>, mut gizmos: Gizmos) {
    let Some(lying) = target.0 else {
        return;
    };
    let (x, y) = (lying.pos.x.to_f32(), lying.pos.y.to_f32());
    let centre = Vec3::new(x, ground.at(x, y) + 2.0, -y);
    gizmos.circle(
        Isometry3d::new(centre, Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)),
        lying.reach + RING_MARGIN,
        crate::pick::HOVER_RING,
    );
}
