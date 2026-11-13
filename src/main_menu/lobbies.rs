//! The service's lobby browser and the entry to local hosting.
use super::{
    core::{Act, Page},
    kit::{Kit, MenuButton, Tone},
    lobby_controls::{Action, Choice, ControlsView},
    lobby_widgets::{self as w},
    play::PlayAction,
    view::{FieldView, LobbyRow, MenuView, PageView},
};
use crate::theme;
use bevy::prelude::*;
use bevy::ui_widgets::Button;

pub fn draw(kit: &mut Kit, root: Entity, view: &MenuView) -> bool {
    match &view.body {
        PageView::Lobbies { list, fields } => browser(kit, root, list, fields, &view.lobby_ui),
        PageView::Lobby(lobby) => super::lobby_room::draw(kit, root, lobby, &view.lobby_ui),
        _ => return false,
    }
    true
}

fn browser(
    kit: &mut Kit,
    root: Entity,
    rows: &[LobbyRow],
    fields: &[FieldView],
    controls: &ControlsView,
) {
    w::backdrop(
        kit,
        root,
        "lobby browser backdrop",
        Act::Play(PlayAction::Open(true)),
    );
    let panel = w::panel(kit, root, "lobby browser");
    let visible: Vec<_> = rows
        .iter()
        .filter(|row| {
            super::lobby_controls::includes_lobby(
                controls.filter.as_ref(),
                controls.no_password,
                &row.ruleset,
                row.locked,
            )
        })
        .collect();
    heading(kit, panel, visible.len(), rows.len());
    table(kit, panel, &visible, controls);
    kit.node(
        panel,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    filters(kit, panel, fields.first(), controls);
    creation(kit, panel, controls);
}

fn heading(kit: &mut Kit, parent: Entity, shown: usize, total: usize) {
    let row = kit.node(
        parent,
        Node {
            height: px(42),
            align_items: AlignItems::Center,
            column_gap: px(16),
            ..default()
        },
    );
    kit.typed(row, "LOBBIES", theme::lobby::TITLE, theme::INK_1);
    kit.typed(
        row,
        format!("{shown} of {total} open"),
        theme::LABEL,
        theme::INK_4,
    );
    kit.node(
        row,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    w::quiet(kit, row, "REFRESH  ⟳", Act::SearchLobbies, theme::INK_2);
}

fn table(kit: &mut Kit, parent: Entity, rows: &[&LobbyRow], controls: &ControlsView) {
    let table = w::column(kit, parent, 2.0);
    kit.commands.entity(table).insert(Node {
        max_height: px(450),
        overflow: Overflow::scroll_y(),
        flex_direction: FlexDirection::Column,
        row_gap: px(2),
        ..default()
    });
    kit.commands.entity(table).insert((
        super::lobby_scroll::Area::Browser,
        ScrollPosition::default(),
    ));
    let header = table_row(kit, table, 30.0);
    kit.commands
        .entity(header)
        .entry::<Node>()
        .and_modify(|mut node| node.margin.bottom = px(5));
    for (index, label) in ["", "NAME", "HOST", "SLOTS", "MODE"]
        .into_iter()
        .enumerate()
    {
        let cell = cell(kit, header, index);
        kit.typed(cell, label, theme::CAPTION, theme::INK_4);
    }
    w::rule(kit, table);
    if rows.is_empty() {
        kit.typed(table, "No open lobbies", theme::LABEL, theme::INK_4);
    }
    for lobby in rows.iter().take(bota_radovid::proto::MAX_LOBBY_LIST) {
        lobby_row(kit, table, lobby, controls.selected == Some(lobby.id));
    }
}

fn table_row(kit: &mut Kit, parent: Entity, height: f32) -> Entity {
    kit.node(
        parent,
        Node {
            height: px(height),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(8), px(0)),
            column_gap: px(8),
            ..default()
        },
    )
}

fn cell(kit: &mut Kit, parent: Entity, index: usize) -> Entity {
    let (width, grow) = match index {
        0 => (28.0, 0.0),
        1 => (190.0, 1.0),
        2 => (140.0, 0.0),
        3 => (70.0, 0.0),
        _ => (100.0, 0.0),
    };
    kit.node(
        parent,
        Node {
            width: px(width),
            min_width: px(0),
            flex_shrink: 1.0,
            flex_grow: grow,
            overflow: Overflow::clip(),
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

fn lobby_row(kit: &mut Kit, parent: Entity, lobby: &LobbyRow, selected: bool) {
    let row = table_row(kit, parent, 48.0);
    kit.commands.entity(row).insert((
        Button,
        Pickable::default(),
        MenuButton(Act::JoinLobby(lobby.id)),
        w::Control,
        Name::new(format!("join {}", lobby.name.to_lowercase())),
    ));
    if selected {
        kit.commands
            .entity(row)
            .insert(theme::rightward(theme::lobby::SELECTION));
        let marker = kit.node(
            row,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                bottom: px(0),
                width: px(3),
                ..default()
            },
        );
        kit.commands.entity(marker).insert((
            Name::new("selected lobby marker"),
            BackgroundColor(theme::SELECT),
        ));
    } else {
        kit.commands
            .entity(row)
            .insert(BackgroundColor(theme::lobby::ROW_TINT));
    }
    let lock = cell(kit, row, 0);
    if lobby.locked {
        w::lock(kit, lock);
    }
    for (index, text) in [
        &lobby.name,
        &lobby.owner,
        &format!("{} / {}", lobby.members, lobby.seats),
        &lobby.mode,
    ]
    .into_iter()
    .enumerate()
    {
        let cell = cell(kit, row, index + 1);
        kit.typed(
            cell,
            text,
            theme::BODY,
            if index == 0 {
                theme::INK_1
            } else {
                theme::INK_2
            },
        );
    }
}

fn filters(kit: &mut Kit, parent: Entity, field: Option<&FieldView>, controls: &ControlsView) {
    let parent = w::column(kit, parent, 8.0);
    w::rule(kit, parent);
    kit.typed(parent, "FILTER LOBBIES", theme::LABEL, theme::INK_2);
    let row = kit.node(
        parent,
        Node {
            column_gap: px(8),
            align_items: AlignItems::FlexEnd,
            ..default()
        },
    );
    filter_mode(kit, row, controls);
    let search = kit.node(
        row,
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            ..default()
        },
    );
    if let Some(field) = field {
        let field = FieldView {
            label: "Lobby or host name",
            ..field.clone()
        };
        w::field(kit, search, 0, &field, false, false);
    }
    let check = kit.node(
        row,
        Node {
            width: px(136),
            height: px(44),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
    );
    w::check(
        kit,
        check,
        "No password",
        controls.no_password,
        w::action(Action::NoPassword),
    );
}

fn filter_mode(kit: &mut Kit, row: Entity, controls: &ControlsView) {
    let mode = kit.node(
        row,
        Node {
            flex_direction: FlexDirection::Column,
            width: px(150),
            flex_shrink: 0.0,
            ..default()
        },
    );
    let options =
        (controls.open == Some(Choice::Filter)).then(|| controls.choice_list(Choice::Filter));
    w::select(
        kit,
        mode,
        "Game mode",
        controls.mode_name(controls.filter.as_ref()),
        Choice::Filter,
        options,
        true,
        false,
    );
}

fn creation(kit: &mut Kit, parent: Entity, controls: &ControlsView) {
    let parent = w::column(kit, parent, 8.0);
    w::rule(kit, parent);
    kit.typed(parent, "CREATE A LOBBY", theme::LABEL, theme::INK_2);
    let row = kit.node(
        parent,
        Node {
            column_gap: px(8),
            ..default()
        },
    );
    let server = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            flex_direction: FlexDirection::Column,
            min_width: px(0),
            ..default()
        },
    );
    let options =
        (controls.open == Some(Choice::Server)).then(|| controls.choice_list(Choice::Server));
    w::select(
        kit,
        server,
        "Server",
        controls.server_name(),
        Choice::Server,
        options,
        true,
        false,
    );
    let mode = kit.node(
        row,
        Node {
            flex_direction: FlexDirection::Column,
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            ..default()
        },
    );
    let options = (controls.open == Some(Choice::Mode)).then(|| controls.choice_list(Choice::Mode));
    w::select(
        kit,
        mode,
        "Game mode",
        controls.mode_name(controls.mode.as_ref()),
        Choice::Mode,
        options,
        true,
        false,
    );
    creation_actions(kit, parent);
}

fn creation_actions(kit: &mut Kit, parent: Entity) {
    let actions = kit.node(
        parent,
        Node {
            column_gap: px(8),
            ..default()
        },
    );
    let create = kit.node(
        actions,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            ..default()
        },
    );
    w::full_button(
        kit,
        create,
        "CREATE LOBBY",
        w::action(Action::Create),
        Tone::Primary,
        52.0,
        theme::button(52.0),
    );
    let direct = kit.node(
        actions,
        Node {
            width: px(180),
            flex_shrink: 0.0,
            ..default()
        },
    );
    w::full_button(
        kit,
        direct,
        "JOIN BY ADDRESS",
        Act::Go(Page::LocalLobby),
        Tone::Secondary,
        52.0,
        theme::lobby::DIRECT_ACTION,
    );
}
