//! The main menu, as Dota's: sign in or register with the matchmaking
//! service, find a match per ruleset, answer MATCH FOUND, wait for a game
//! server, rejoin or watch one's running games, lobbies (searched, joined
//! from the list, a locked one with its password), a game by address, and
//! how a finished match went. The hero
//! selection and the game itself are the rest of the client.
//!
//! [`core::MenuCore`] holds the state and the logic and is tested without
//! drawing; [`view`] is what the screens draw; the rest is Bevy.

mod accepted;
mod account_page;
mod address;
mod archive_storage;
pub(crate) mod availability;
mod avatar;
mod avatar_draw;
mod backdrop;
pub(crate) mod catalogue;
mod core;
mod form;
pub(crate) mod hero_stage;
mod highlight;
mod history;
mod history_page;
mod home_cards;
mod keep;
pub(crate) mod kit;
mod ladder_page;
mod launch;
mod leaderboard;
#[cfg(not(target_arch = "wasm32"))]
mod leave;
mod list_cache;
mod lobbies;
mod lobby_chat;
mod lobby_controls;
mod lobby_editor;
mod lobby_room;
mod lobby_scroll;
mod lobby_widgets;
mod local_matches;
mod local_meta;
mod meta_fetch;
mod overlay;
mod page_previews;
mod page_time;
mod pages;
mod play;
mod player_matches;
pub(crate) mod player_profile;
mod prefs;
mod preview;
mod profile_metadata;
mod rank_confidence;
mod replay_info;
pub(crate) mod results;
mod screens;
pub(crate) mod shell;
mod store;
pub(crate) mod text;
mod top_bar;
mod view;
#[cfg(not(target_arch = "wasm32"))]
mod wire_probe;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(not(target_arch = "wasm32"))]
pub use core::Claim;
pub(crate) use core::Page;
pub use core::{Act, DEFAULT_NAME, GameKind, MenuCore};
pub use kit::{Disabled, Wash};
#[cfg(test)]
pub(crate) use kit::{MenuButton, Shade};
pub use launch::Trust;
pub use lobby_chat::Incoming as LobbyChatMessage;
#[cfg(test)]
pub(crate) use lobby_chat::Port as LobbyChatPort;
pub use lobby_controls::Action as LobbyAction;
pub use play::PlayAction;
pub use prefs::page_service;
pub(crate) use preview::loading_chat;
pub use preview::{NAMES as PREVIEWS, view as preview};
pub use shell::{GameEpoch, Preview, Trusted, idle, plugin};
pub use store::SettingsStore;
pub(crate) use text::line;
#[cfg(test)]
pub use view::{Connection, PageView};

#[cfg_attr(
    not(test),
    expect(
        unused_imports,
        reason = "Avatar transport is capability-gated until its SDK endpoint exists"
    )
)]
pub use avatar::Request as AvatarRequest;

#[cfg(test)]
pub(crate) use leaderboard::Players as LeaderboardPlayers;

#[cfg(test)]
pub(crate) use avatar::{Action as AvatarAction, tests::TEST_PNG as AVATAR_TEST_PNG};

#[cfg(test)]
pub(crate) use keep::deliver_replay_for_menu;

#[cfg(test)]
pub(crate) use view::LobbyView;

#[cfg(test)]
pub(crate) use lobby_controls::Choice as LobbyChoice;

#[cfg(test)]
pub(crate) use lobby_scroll::scroll as test_lobby_scroll;
