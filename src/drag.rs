//! Dragging an item of the inventory, as Dota does: the item's icon follows
//! the cursor while the slot it came from shows it lifted (the bottom
//! panel's item face), and when it is let go over another slot the icon is
//! already there: the two slots show the swap at once, until the server's
//! view tells it, so nothing flies back and forth.

use bevy::{prelude::*, window::PrimaryWindow};
use bota_client_core::App as Core;
use bota_proto::{EntityId, ItemView};

use crate::bag::TacticalBag;
use crate::manifest::Manifest;
use crate::session::Session;

/// The dragged icon's size in interface pixels while its slot is not found: a slot box's icon.
const GHOST: (f32, f32) = (60.0, 44.0);
/// One tick's order budget also bounds a composed inventory preview.
const MAX_PENDING_SWAPS: usize = bota_proto::limits::MAX_ORDERS_PER_TICK;
/// How much of the icon shows while it is dragged.
const GHOST_ALPHA: f32 = 0.9;

/// The dragged icon.
#[derive(Component)]
struct Ghost;

/// A swap ordered and not yet seen in the view: the slots show it meanwhile.
#[derive(Resource, Default)]
pub struct Settling(Option<Settle>);

struct Settle {
    owner: EntityId,
    slots: [Option<Option<ItemView>>; bota_proto::limits::TOWN_PORTAL_SLOT + 1],
    sequences: Vec<u32>,
}

impl Settling {
    /// Compose swaps against the same inventory the user sees. In particular,
    /// a second drag must not read the snapshot preceding the first drag.
    pub fn swap(&mut self, core: &mut Core, from: u8, to: u8) {
        if usize::from(from) > bota_proto::limits::TOWN_PORTAL_SLOT
            || usize::from(to) > bota_proto::limits::TOWN_PORTAL_SLOT
        {
            return;
        }
        let Some(owner) = core.commanded() else {
            return;
        };
        let Some(moved) = self.item_in(core, from) else {
            return;
        };
        let displaced = self.item_in(core, to);
        if self.0.as_ref().is_some_and(|s| s.owner != owner) {
            self.0 = None;
        }
        if self
            .0
            .as_ref()
            .is_some_and(|s| s.sequences.len() == MAX_PENDING_SWAPS)
        {
            core.refuse(bota_proto::RejectReason::QueueFull);
            return;
        }
        let before = core.seq();
        core.act(bota_client_core::Intent::Swap { from, to });
        if core.seq() == before {
            return;
        }
        let settle = self.0.get_or_insert_with(|| Settle {
            owner,
            slots: [None; bota_proto::limits::TOWN_PORTAL_SLOT + 1],
            sequences: Vec::new(),
        });
        settle.slots[usize::from(from)] = Some(displaced);
        settle.slots[usize::from(to)] = Some(Some(moved));
        settle.sequences.push(core.seq());
    }

    pub fn item_in(&self, core: &Core, slot: u8) -> Option<ItemView> {
        self.over(core, slot).unwrap_or_else(|| core.item_in(slot))
    }

    /// The commanded unit also owns the panel when selected non-heroes or a
    /// dead hero are shown. Every reader uses that same resolution.
    pub fn over(&self, core: &Core, slot: u8) -> Option<Option<ItemView>> {
        let settle = self
            .0
            .as_ref()
            .filter(|s| Some(s.owner) == core.commanded())?;
        settle.slots.get(usize::from(slot)).copied().flatten()
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
    let core = &session.0;
    let Some(settle) = &settling.0 else { return };
    // Receipts precede application. Keep the preview until the view agrees,
    // or explicitly roll it back when one of its transactions is refused.
    let rejected = core
        .rejected_orders
        .iter()
        .any(|seq| settle.sequences.contains(seq));
    let matches = core.commanded() == Some(settle.owner)
        && core.source.pending_orders() == 0
        && settle.slots.iter().enumerate().all(|(slot, shown)| {
            let identity = |item: ItemView| (item.id, item.charges, item.mode);
            shown.is_none_or(|item| core.item_in(slot as u8).map(identity) == item.map(identity))
        });
    if core.lost || core.source.is_redialing() || core.over.is_some() || rejected || matches {
        settling.0 = None;
    }
}

/// Puts the icon of the item taken up at the cursor.
fn follow(
    session: NonSend<Session>,
    bag: Res<TacticalBag>,
    settling: Res<Settling>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    boxes: Query<(&crate::hud::ItemBox, &ComputedNode, &InheritedVisibility)>,
    mut ghost: Single<(&mut Node, &mut ImageNode, &mut Visibility), With<Ghost>>,
) {
    let core = &session.0;
    let (_, unit, kit) = crate::hud::subject(core);
    let held = core.held_item.and_then(|slot| {
        settling
            .over(core, slot)
            .unwrap_or_else(|| crate::hud::subject_item(core, &bag, unit, kit, slot))
    });
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
    let icon = crate::item_look::icon(&item, &manifest).map(|p| server.load(p));
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
