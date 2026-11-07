//! Shop and inventory items share Quickbuy's drop receiver; planning never moves an item.
use bevy::{prelude::*, window::PrimaryWindow};

use super::{Place, ShopCell, corner::QuickDrop};
use crate::{bag::TacticalBag, session::Session};

const DRAG_DISTANCE: f32 = 4.0;

#[derive(Resource, Default)]
pub(crate) struct ShopDrag {
    held: Option<Held>,
    pub clicked: Option<u16>,
}

struct Held {
    item: u16,
    place: Place,
    start: Vec2,
    dragging: bool,
}

impl ShopDrag {
    pub fn item(&self) -> Option<u16> {
        self.held
            .as_ref()
            .filter(|held| held.dragging)
            .map(|held| held.item)
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<ShopDrag>()
        .add_systems(
            First,
            crate::loading::reset::<ShopDrag>.in_set(crate::loading::Begin),
        )
        .add_systems(
            Update,
            track
                .before(super::clicks::cells)
                .before(crate::input::mouse)
                .before(crate::input::boxes)
                .run_if(super::match_or_tactical),
        );
}

fn track(
    mut session: NonSendMut<Session>,
    mut drag: ResMut<ShopDrag>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Single<&Window, With<PrimaryWindow>>,
    cells: Query<(&Interaction, &ShopCell, &Place, &InheritedVisibility)>,
    drop: Query<(&Interaction, &InheritedVisibility), With<QuickDrop>>,
    bag: Res<TacticalBag>,
    settling: Res<crate::drag::Settling>,
) {
    drag.clicked = None;
    let Some(cursor) = window.cursor_position() else {
        drag.held = None;
        return;
    };
    let hovered = cells
        .iter()
        .find(|(over, _, _, seen)| **over != Interaction::None && seen.get());
    if mouse.just_pressed(MouseButton::Left) {
        drag.held = hovered.map(|(_, item, place, _)| Held {
            item: item.0,
            place: *place,
            start: cursor,
            dragging: false,
        });
    }
    if let Some(held) = &mut drag.held {
        held.dragging |= cursor.distance_squared(held.start) > DRAG_DISTANCE * DRAG_DISTANCE;
    }
    if !mouse.just_released(MouseButton::Left) {
        return;
    }
    let held = drag.held.take();
    let core = &mut session.0;
    let inside = drop
        .iter()
        .any(|(over, seen)| *over != Interaction::None && seen.get());
    let inventory = core
        .held_item
        .and_then(|slot| super::displayed_item(core, &bag, &settling, slot).map(|item| item.id.0));
    let from_shop = held
        .as_ref()
        .filter(|held| held.dragging && held.place != Place::Quick)
        .map(|held| held.item);
    if inside && let Some(item) = from_shop.or(inventory) {
        core.held_item = None;
        core.set_quick_buy(
            item,
            keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]),
        );
    } else if let Some(held) = held.filter(|held| !held.dragging)
        && hovered.is_some_and(|(_, item, _, _)| item.0 == held.item)
    {
        drag.clicked = Some(held.item);
    }
}
