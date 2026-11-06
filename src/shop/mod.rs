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
pub use corner::STICKY;
#[cfg(test)]
pub(crate) use corner::{GrabAll, drawn_gold};
pub(crate) use drag::ShopDrag;
pub use plugin::plugin;

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
