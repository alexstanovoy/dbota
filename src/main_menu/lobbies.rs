//! LOBBIES: making a lobby, and the open ones, searched by name or owner and
//! joined from the list (a locked one through the password dialog).

use bevy::prelude::*;

use super::core::Act;
use super::pages::field_box;
use super::screens::{GREEN, Kit, PLAIN};
use super::view::{FieldView, LobbyRow};
use crate::ui;

pub fn list(kit: &mut Kit, body: Entity, lobbies: &[LobbyRow], fields: &[FieldView]) {
    let page = kit.node(
        body,
        Node {
            column_gap: px(24),
            align_items: AlignItems::FlexStart,
            ..default()
        },
    );
    let card = kit.panel(
        page,
        Node {
            width: px(360),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(20)),
            row_gap: px(12),
            ..default()
        },
    );
    kit.bold(card, "CREATE A LOBBY", 22.0, ui::INK);
    kit.text(
        card,
        "It begins with the standard rules: you choose the game mode in the lobby. A password \
         keeps out whoever does not know it.",
        13.0,
        ui::INK_DIM,
    );
    let (create, search) = fields.split_at(fields.len().min(2));
    for (at, field) in create.iter().enumerate() {
        field_box(kit, card, at, field);
    }
    kit.button(card, "CREATE", Act::CreateLobby, GREEN, 40.0);

    let column = kit.node(
        page,
        Node {
            width: px(520),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        },
    );
    let head = kit.node(
        column,
        Node {
            column_gap: px(12),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.bold(head, "OPEN LOBBIES", 22.0, ui::INK);
    // The list again, as the search typed asks it: a refresh keeps the search.
    kit.button(head, "REFRESH", Act::SearchLobbies, PLAIN, 28.0);
    for (at, field) in search.iter().enumerate() {
        field_box(kit, column, create.len() + at, field);
    }
    let row = kit.node(column, Node::default());
    kit.button(row, "SEARCH", Act::SearchLobbies, PLAIN, 34.0);
    if lobbies.is_empty() {
        kit.text(column, "No open lobby.", 15.0, ui::INK_DIM);
    }
    for lobby in lobbies {
        let row = kit.panel(
            column,
            Node {
                padding: UiRect::all(px(10)),
                column_gap: px(12),
                align_items: AlignItems::Center,
                ..default()
            },
        );
        let label = kit.node(
            row,
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        let lock = if lobby.locked { "  [password]" } else { "" };
        kit.bold(label, format!("{}{lock}", lobby.name), 17.0, ui::INK);
        kit.text(
            label,
            format!(
                "{} · {} of {} seats",
                lobby.owner, lobby.members, lobby.seats
            ),
            13.0,
            ui::INK_DIM,
        );
        let join = kit.button(row, "JOIN", Act::JoinLobby(lobby.id), GREEN, 34.0);
        // A script clicks it by the lobby's name.
        kit.commands
            .entity(join)
            .insert(Name::new(format!("join {}", lobby.name.to_lowercase())));
    }
}
