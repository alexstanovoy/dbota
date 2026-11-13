//! The shop, as Dota 7.41 has it: the search box and the BASICS, UPGRADES
//! and NEUTRALS tabs with Dota's categories, the build tree of the item
//! chosen, and in the corner the stash with GRAB ALL, the quick-buy row and
//! the gold that opens the shop.

mod catalog;
mod cell;
mod clicks;
mod corner;
mod drag;
mod panel;
mod plugin;
mod tree;

use crate::select::match_or_tactical;
pub use catalog::Catalog;
pub(crate) use cell::Chosen;
#[cfg(test)]
pub(crate) use cell::ShopAccessShade;
pub use cell::{Place, ShopCell, spawn as spawn_cell};
#[cfg(test)]
pub(crate) use corner::Grab;
pub use corner::STICKY;
#[cfg(test)]
pub(crate) use corner::{GrabAll, ShopButton, drawn_gold, drawn_gold_style};
pub(crate) use drag::ShopDrag;
#[cfg(test)]
pub(crate) use panel::ShopPanel;
pub use plugin::plugin;
#[cfg(test)]
pub(crate) use tree::{HEIGHT as TREE_HEIGHT, Tree};

/// The item an inventory slot shows, including ordered moves not in the snapshot yet.
pub(crate) fn displayed_item(
    core: &bota_client_core::App,
    bag: &crate::bag::TacticalBag,
    settling: &crate::drag::Settling,
    slot: u8,
) -> Option<bota_proto::ItemView> {
    let (_, unit, kit) = crate::hud::subject(core);
    settling
        .over(core, slot)
        .unwrap_or_else(|| crate::hud::subject_item(core, bag, unit, kit, slot))
}
