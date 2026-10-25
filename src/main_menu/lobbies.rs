//! LOBBIES: the open lobbies, and making or joining one by name and
//! password.

use bevy::prelude::*;

use super::core::{Act, Page};
use super::pages::field_box;
use super::screens::{GREEN, Kit, PLAIN};
use super::view::{FieldView, LobbyRow, ModeRow};
use crate::ui;

pub fn list(
    kit: &mut Kit,
    body: Entity,
    lobbies: &[LobbyRow],
    fields: &[FieldView],
    modes: &[ModeRow],
) {
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
    kit.bold(card, "CREATE OR JOIN", 22.0, ui::INK);
    kit.text(
        card,
        "A lobby with a password lets in whoever knows it.",
        13.0,
        ui::INK_DIM,
    );
    for (at, field) in fields.iter().enumerate() {
        field_box(kit, card, at, field);
    }
    if modes.len() > 1 {
        kit.text(card, "Game mode", 13.0, ui::INK_DIM);
        for mode in modes {
            let look = if mode.chosen { GREEN } else { PLAIN };
            kit.button(
                card,
                &mode.name,
                Act::LobbyMode(mode.id.clone()),
                look,
                34.0,
            );
        }
    }
    let row = kit.node(
        card,
        Node {
            column_gap: px(10),
            ..default()
        },
    );
    kit.button(row, "CREATE", Act::CreateLobby, GREEN, 40.0);
    kit.button(row, "JOIN BY NAME", Act::Submit, PLAIN, 40.0);

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
    kit.button(head, "REFRESH", Act::Go(Page::Lobbies), PLAIN, 28.0);
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
        kit.button(row, "JOIN", Act::AskLobby(lobby.id), GREEN, 34.0);
    }
}
