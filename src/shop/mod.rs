//! The shop, as Dota 7.41 has it: the search box and the BASICS, UPGRADES
//! and NEUTRALS tabs with Dota's categories, the build tree of the item
//! chosen, and in the corner the stash with GRAB ALL, the quick-buy row and
//! the gold that opens the shop.

mod catalog;
mod cell;
mod clicks;
mod corner;
mod panel;
mod plugin;
mod tree;

use crate::select::match_or_tactical;
pub use catalog::Catalog;
pub use cell::ShopCell;
pub use corner::STICKY;
pub use plugin::plugin;
