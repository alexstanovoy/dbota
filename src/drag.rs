//! Dragging an item of the inventory, as Dota does: the item's icon follows
//! the cursor while the slot it came from shows it lifted (the bottom
//! panel's item face), and when it is let go over another slot the icon is
//! already there: the two slots show the swap at once, until the server's
//! view tells it, so nothing flies back and forth.

use bevy::{prelude::*, window::PrimaryWindow};
use bota_proto::ItemView;

use crate::bag::TacticalBag;
use crate::manifest::Manifest;
use crate::session::Session;

/// The dragged icon's size in interface pixels while its slot is not found: a slot box's icon.
const GHOST: (f32, f32) = (60.0, 44.0);
/// Seconds a swap is shown before the view tells it, at most.
const SETTLE_SECS: f32 = 0.6;
/// How much of the icon shows while it is dragged.
const GHOST_ALPHA: f32 = 0.9;

/// The dragged icon.
#[derive(Component)]
struct Ghost;

/// A swap ordered and not yet seen in the view: the slots show it meanwhile.
#[derive(Resource, Default)]
pub struct Settling(Option<Settle>);

struct Settle {
    from: u8,
    to: u8,
    moved: ItemView,
    displaced: Option<ItemView>,
    began: f32,
}

impl Settling {
    /// Starts showing the item of `from` in `to`, and what was in `to` in `from`.
    pub fn start(
        &mut self,
        (from, to): (u8, u8),
        moved: ItemView,
        displaced: Option<ItemView>,
        now: f32,
    ) {
        self.0 = Some(Settle {
            from,
            to,
            moved,
            displaced,
            began: now,
        });
    }

    /// What a slot shows in place of what the view has: the item that is
    /// there (or none) while a swap is under way; nothing to say otherwise.
    pub fn over(&self, slot: u8, now: f32) -> Option<Option<ItemView>> {
        let settle = self.0.as_ref().filter(|s| now - s.began < SETTLE_SECS)?;
        if slot == settle.to {
            Some(Some(settle.moved))
        } else if slot == settle.from {
            Some(settle.displaced)
        } else {
            None
        }
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Settling>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (settled, follow));
}

fn spawn(mut commands: Commands) {
    commands.spawn((
        Ghost,
        Node {
            position_type: PositionType::Absolute,
            width: px(GHOST.0),
            height: px(GHOST.1),
            ..default()
        },
        ImageNode::default(),
        GlobalZIndex(40),
        Pickable::IGNORE,
        Visibility::Hidden,
    ));
}

/// Forgets a swap once the view shows the item in its new slot.
fn settled(session: NonSend<Session>, mut settling: ResMut<Settling>) {
    let Some(settle) = &settling.0 else { return };
    if session.0.item_in(settle.to).map(|i| i.id) == Some(settle.moved.id)
        && session.0.item_in(settle.from).map(|i| i.id) == settle.displaced.map(|i| i.id)
    {
        settling.0 = None;
    }
}

/// Puts the icon of the item taken up at the cursor.
fn follow(
    session: NonSend<Session>,
    bag: Res<TacticalBag>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    boxes: Query<(&crate::hud::ItemBox, &ComputedNode, &InheritedVisibility)>,
    mut ghost: Single<(&mut Node, &mut ImageNode, &mut Visibility), With<Ghost>>,
) {
    let core = &session.0;
    let (_, unit, kit) = crate::hud::subject(core);
    let held = core
        .held_item
        .and_then(|slot| crate::hud::subject_item(core, &bag, unit, kit, slot));
    let (Some(item), Some(cursor)) = (held, window.cursor_position()) else {
        ghost.2.set_if_neq(Visibility::Hidden);
        return;
    };
    let scale = ui_scale.0.max(0.1);
    // As big as the slot it was taken from.
    let size = boxes
        .iter()
        .find(|(b, _, shown)| Some(b.0) == core.held_item && shown.get())
        .map_or(Vec2::new(GHOST.0, GHOST.1), |(_, node, _)| {
            node.size() * node.inverse_scale_factor
        });
    ghost.0.width = px(size.x);
    ghost.0.height = px(size.y);
    ghost.0.left = px(cursor.x / scale - size.x / 2.0);
    ghost.0.top = px(cursor.y / scale - size.y / 2.0);
    let icon = manifest.item_icon(item.id.0).map(|p| server.load(p));
    let handle = icon.clone().unwrap_or_default();
    if ghost.1.image != handle {
        ghost.1.image = handle;
    }
    let tint = if icon.is_some() {
        Color::srgba(1.0, 1.0, 1.0, GHOST_ALPHA)
    } else {
        Color::NONE
    };
    if ghost.1.color != tint {
        ghost.1.color = tint;
    }
    ghost.2.set_if_neq(Visibility::Inherited);
}
