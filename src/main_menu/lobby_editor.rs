//! The pre-create and host settings popup; edits stay local until OK.
use super::{
    core::Act,
    kit::Kit,
    lobby_controls::{Action, Choice, ControlsView, EditorView, audience_name},
    lobby_widgets::{self as w},
    view::PasswordView,
};
use crate::theme;
use bevy::prelude::*;

pub fn draw(kit: &mut Kit, root: Entity, controls: &ControlsView) {
    let Some(editor) = &controls.editor else {
        return;
    };
    let card = modal(kit, root, theme::lobby::EDITOR_WIDTH, 12.0);
    centered_text(
        kit,
        card,
        if editor.creating {
            "CREATE LOBBY"
        } else {
            "LOBBY SETTINGS"
        },
        theme::lobby::TITLE,
        theme::INK_1,
    );
    let general = w::column(kit, card, 2.0);
    kit.typed(
        general,
        "GENERAL",
        theme::lobby::SECTION_LABEL,
        theme::INK_2,
    );
    w::rule(kit, general);
    fields(kit, card, controls, editor);
    toggles(kit, card, editor);
    answers(
        kit,
        card,
        w::action(Action::Save),
        w::action(Action::Cancel),
        editor.saving,
    );
    if let Some(error) = &editor.error {
        let notice = kit.node(root, crate::ui::fill());
        kit.commands
            .entity(notice)
            .insert((GlobalZIndex(100), Pickable::IGNORE));
        super::overlay::status_line(
            kit,
            notice,
            &super::core::Status {
                text: error.clone(),
                error: true,
            },
        );
    }
}

fn centered_text(
    kit: &mut Kit,
    parent: Entity,
    value: impl Into<String>,
    role: theme::Type,
    ink: Color,
) {
    let text = kit.typed(parent, value, role, ink);
    kit.commands.entity(text).insert((
        Node {
            width: percent(100),
            ..default()
        },
        TextLayout::justify(Justify::Center),
    ));
}

fn modal(kit: &mut Kit, root: Entity, width: f32, gap: f32) -> Entity {
    let dim = kit.node(
        root,
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..crate::ui::fill()
        },
    );
    kit.commands.entity(dim).insert((
        BackgroundColor(theme::scrim()),
        crate::hud::Blocker,
        bevy::picking::hover::Hovered::default(),
        Pickable::default(),
        GlobalZIndex(90),
    ));
    let card = kit.panel(
        dim,
        Node {
            width: px(width),
            padding: UiRect::all(px(28)),
            row_gap: px(gap),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            ..default()
        },
    );
    kit.commands.entity(card).insert((
        Name::new("lobby settings popup"),
        theme::downward(theme::lobby::POPUP),
    ));
    card
}

fn row(kit: &mut Kit, parent: Entity) -> [Entity; 2] {
    let row = kit.node(
        parent,
        Node {
            column_gap: px(16),
            ..default()
        },
    );
    std::array::from_fn(|_| {
        kit.node(
            row,
            Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                flex_basis: px(0),
                min_width: px(0),
                ..default()
            },
        )
    })
}

fn fields(kit: &mut Kit, parent: Entity, controls: &ControlsView, editor: &EditorView) {
    let grid = w::column(kit, parent, 8.0);
    let [name, mode] = row(kit, grid);
    if let Some(field) = editor.fields.first() {
        w::field(kit, name, 0, field, !editor.creating, false);
    }
    let modes =
        (controls.open == Some(Choice::EditMode)).then(|| controls.choice_list(Choice::EditMode));
    w::select(
        kit,
        mode,
        "Game mode",
        controls.mode_name(Some(&editor.mode)),
        Choice::EditMode,
        modes,
        false,
        false,
    );
    let [password, server] = row(kit, grid);
    if let Some(field) = editor.fields.get(1) {
        w::field(kit, password, 1, field, false, false);
    }
    w::select(
        kit,
        server,
        "Server",
        &controls.service,
        Choice::Server,
        None,
        false,
        true,
    );
    let [audience, _] = row(kit, grid);
    let options =
        (controls.open == Some(Choice::Audience)).then(|| controls.choice_list(Choice::Audience));
    w::select(
        kit,
        audience,
        "Open to",
        audience_name(editor.audience),
        Choice::Audience,
        options,
        false,
        false,
    );
}

fn toggles(kit: &mut Kit, parent: Entity, editor: &EditorView) {
    let panel = kit.node(
        parent,
        Node {
            padding: UiRect::all(px(12)),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
    );
    kit.commands
        .entity(panel)
        .insert(BackgroundColor(theme::lobby::ROW_TINT));
    let check = w::check(
        kit,
        panel,
        "Enable cheats",
        editor.cheats == Some(true),
        w::action(Action::Cheats),
    );
    if editor.cheats.is_none() {
        kit.commands
            .entity(check)
            .remove::<super::kit::MenuButton>();
    }
    w::check(
        kit,
        panel,
        "Hide password",
        editor.hidden,
        w::action(Action::HidePassword),
    );
    if editor.password_unknown {
        w::check(
            kit,
            panel,
            "Remove password",
            editor.remove_password,
            w::action(Action::RemovePassword),
        );
    }
}

fn answers(kit: &mut Kit, parent: Entity, ok: Act, cancel: Act, saving: bool) {
    let row = kit.node(
        parent,
        Node {
            justify_content: JustifyContent::Center,
            column_gap: px(16),
            ..default()
        },
    );
    if saving {
        for label in ["SAVING…", "CANCEL"] {
            let button = kit.disabled_button(row, label, 44.0);
            kit.commands
                .entity(button)
                .entry::<Node>()
                .and_modify(|mut node| node.width = px(super::kit::POPUP_ANSWER_WIDTH));
        }
    } else {
        kit.popup_button(row, "OK", ok);
        kit.popup_button(row, "CANCEL", cancel);
    }
}

pub fn password(kit: &mut Kit, root: Entity, password: &PasswordView, hidden: bool) {
    let card = modal(kit, root, theme::lobby::PASSWORD_WIDTH, 14.0);
    kit.commands
        .entity(card)
        .insert(Name::new("lobby password popup"));
    centered_text(
        kit,
        card,
        "LOBBY PASSWORD",
        theme::lobby::TITLE,
        theme::INK_1,
    );
    centered_text(
        kit,
        card,
        format!("{} needs a password.", password.lobby),
        theme::LABEL,
        theme::INK_2,
    );
    let field = super::view::FieldView {
        label: "Password",
        ..password.field.clone()
    };
    w::field(kit, card, 0, &field, false, password.error.is_some());
    let error_row = kit.node(
        card,
        Node {
            height: px(18),
            flex_shrink: 0.0,
            ..default()
        },
    );
    kit.typed(
        error_row,
        password.error.as_deref().unwrap_or(""),
        theme::BODY,
        theme::ERROR_TEXT,
    );
    w::check(
        kit,
        card,
        "Hide password",
        hidden,
        w::action(Action::HideJoinPassword),
    );
    answers(
        kit,
        card,
        Act::LobbyPassword(true),
        Act::LobbyPassword(false),
        false,
    );
}
