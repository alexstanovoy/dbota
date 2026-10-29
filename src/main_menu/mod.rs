//! The main menu, as Dota's: sign in or register with the matchmaking
//! service, find a match per ruleset, answer MATCH FOUND, wait for a game
//! server, rejoin or watch one's running games, lobbies by name and
//! password, a game by address, and how a finished match went. The hero
//! selection and the game itself are the rest of the client.
//!
//! [`core::MenuCore`] holds the state and the logic and is tested without
//! drawing; [`view`] is what the screens draw; the rest is Bevy.

mod address;
mod core;
mod form;
mod launch;
mod lobbies;
mod overlay;
mod pages;
mod prefs;
mod preview;
mod replay_info;
mod screens;
mod shell;
mod store;
mod text;
mod view;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(not(target_arch = "wasm32"))]
pub use core::Claim;
#[cfg(not(target_arch = "wasm32"))]
pub use core::Page;
pub use core::{Act, DEFAULT_NAME, GameKind, MenuCore};
pub use launch::Trust;
pub use prefs::page_service;
pub use preview::{NAMES as PREVIEWS, view as preview};
pub use screens::Disabled;
// What holds the menu: assets out of date, on the desktop (a browser page holds before the client).
#[cfg(not(target_arch = "wasm32"))]
pub use shell::Held;
pub use shell::{Preview, Trusted, idle, plugin};
pub use store::SettingsStore;
