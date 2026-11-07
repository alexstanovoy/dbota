//! The main menu, as Dota's: sign in or register with the matchmaking
//! service, find a match per ruleset, answer MATCH FOUND, wait for a game
//! server, rejoin or watch one's running games, lobbies (searched, joined
//! from the list, a locked one with its password), a game by address, and
//! how a finished match went. The hero
//! selection and the game itself are the rest of the client.
//!
//! [`core::MenuCore`] holds the state and the logic and is tested without
//! drawing; [`view`] is what the screens draw; the rest is Bevy.

mod address;
mod backdrop;
mod core;
mod form;
mod keep;
mod kit;
mod launch;
#[cfg(not(target_arch = "wasm32"))]
mod leave;
mod lobbies;
mod meta_fetch;
mod overlay;
mod pages;
mod prefs;
mod preview;
mod replay_info;
mod screens;
mod shell;
mod store;
mod text;
mod top_bar;
mod view;
#[cfg(not(target_arch = "wasm32"))]
mod wire_probe;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(not(target_arch = "wasm32"))]
pub use core::Claim;
#[cfg(test)]
pub use core::Page;
pub use core::{Act, DEFAULT_NAME, GameKind, MenuCore};
pub use kit::{Disabled, Wash};
#[cfg(test)]
pub(crate) use kit::{MenuButton, Shade};
pub use launch::Trust;
pub use prefs::page_service;
pub use preview::{NAMES as PREVIEWS, view as preview};
pub use shell::{GameEpoch, Preview, Trusted, idle, plugin};
pub use store::SettingsStore;
pub(crate) use text::line;
#[cfg(test)]
pub use view::Connection;
