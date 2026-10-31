//! An item as Dota draws it by its state: a Bottle by the rune it keeps or the charges it has
//! left, Power Treads by the attribute they are set to, a Ward Dispenser by the ward it plants.
//! The manifest's `state` lines say it, item by item (grammar in its header, read and checked by
//! `dbota_import::catalog`): the first line of an item whose state holds names the icon the item
//! is drawn with and the install's words for that state, which its tooltip shows. An item with a
//! new state adds lines there, not code.

use bota_proto::ItemView;
use dbota_import::catalog::{ItemState, StateWhen};

use crate::manifest::Manifest;

/// Whether an item in a slot is in a state.
fn holds(when: StateWhen, item: &ItemView) -> bool {
    match when {
        StateWhen::Rune(kind) => item.rune.is_some_and(|r| r.kind == kind),
        StateWhen::Mode(attribute) => item.mode == Some(attribute),
        StateWhen::Ward { sentry } => item.dispenser.is_some_and(|d| d.sentry_active == sentry),
        StateWhen::Charges(n) => item.charges == Some(n),
    }
}

/// The state line an item in a slot is drawn by, if one of its holds.
pub fn state<'m>(item: &ItemView, manifest: &'m Manifest) -> Option<&'m ItemState> {
    let name = manifest.dota_name("item", item.id.0)?;
    manifest
        .states
        .iter()
        .find(|s| s.item == name && holds(s.when, item))
}

/// The asset path of the icon an item in a slot is drawn with: its state's, else its own.
pub fn icon(item: &ItemView, manifest: &Manifest) -> Option<String> {
    state(item, manifest)
        .and_then(|s| manifest.file(&s.icon))
        .or_else(|| manifest.item_icon(item.id.0))
}

/// The label key of the install's words for the state an item is in, if it has some.
pub fn state_text<'m>(item: &ItemView, manifest: &'m Manifest) -> Option<&'m str> {
    state(item, manifest)?.text.as_deref()
}
