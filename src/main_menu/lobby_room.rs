//! A lobby's teams, spectators, settings and private conversation.
use super::{
    core::Act,
    kit::{Kit, Tone},
    lobby_chat::{ChatView, Line},
    lobby_controls::{Action, ControlsView},
    lobby_widgets::{self as w},
    view::{LobbyView, MemberRow, TeamRow},
};
use crate::theme;
use bevy::prelude::*;
use bota_radovid::proto::LOBBY_NO_SLOT;

pub fn draw(kit: &mut Kit, root: Entity, lobby: &LobbyView, controls: &ControlsView) {
    background(kit, root);
    conversation(kit, root, &controls.chat);
    let panel = w::panel(kit, root, "lobby room");
    heading(kit, panel, lobby, &controls.chat);
    let teams = kit.node(
        panel,
        Node {
            column_gap: px(14),
            ..default()
        },
    );
    for (index, team) in lobby.teams.iter().take(2).enumerate() {
        team_column(kit, teams, team, lobby, index);
    }
    spectators(kit, panel, lobby);
    settings(kit, panel, lobby, &controls.service);
    kit.node(
        panel,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    footer(kit, panel, lobby);
}

fn background(kit: &mut Kit, root: Entity) {
    let area = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            top: px(theme::BAR_HEIGHT),
            bottom: px(0),
            left: px(0),
            right: px(0),
            ..default()
        },
    );
    kit.commands
        .entity(area)
        .insert(theme::rightward(theme::lobby::SCENE));
    let shade = kit.node(area, crate::ui::fill());
    kit.commands.entity(shade).insert(theme::downward((
        theme::GROUND.with_alpha(0.0),
        theme::GROUND,
    )));
    kit.art(
        root,
        "portrait.hero.pudge.tall",
        Node {
            position_type: PositionType::Absolute,
            left: px(260),
            top: px(110),
            width: px(260),
            height: px(345),
            ..default()
        },
    );
}

fn heading(kit: &mut Kit, parent: Entity, lobby: &LobbyView, chat: &ChatView) {
    let row = kit.node(
        parent,
        Node {
            height: px(40),
            align_items: AlignItems::Baseline,
            column_gap: px(10),
            border: UiRect::bottom(px(1)),
            padding: UiRect::bottom(px(8)),
            ..default()
        },
    );
    kit.commands
        .entity(row)
        .insert(BorderColor::all(theme::lobby::RULE));
    if lobby.locked {
        w::lock(kit, row);
    }
    kit.typed(
        row,
        &lobby.name,
        theme::Type {
            size: 26.0,
            ..theme::HEADING
        },
        theme::INK_1,
    );
    if let Some(host) = chat
        .people
        .iter()
        .find(|person| Some(person.account) == chat.owner)
    {
        kit.text(row, format!("hosted by {}", host.name), 15.0, theme::INK_4);
    }
}

fn team_column(kit: &mut Kit, parent: Entity, team: &TeamRow, lobby: &LobbyView, index: usize) {
    let color = if index == 0 {
        theme::RADIANT_TEXT
    } else {
        theme::DIRE_TEXT
    };
    let card = kit.node(
        parent,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            height: px(296),
            overflow: Overflow::scroll_y(),
            padding: UiRect::all(px(8)),
            border: UiRect::all(px(1)),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    kit.commands.entity(card).insert((
        Name::new(format!("lobby team {}", team.name)),
        ScrollPosition::default(),
        BorderColor::all(color),
        BackgroundColor(Color::BLACK.with_alpha(0.12)),
    ));
    let heading = kit.node(
        card,
        Node {
            height: px(34),
            margin: UiRect::bottom(px(4)),
            border: UiRect::bottom(px(1)),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.commands.entity(heading).insert(BorderColor::all(color));
    kit.typed(
        heading,
        &team.name,
        theme::Type {
            size: 24.0,
            tracking: 3.0,
            ..theme::HEADING
        },
        color,
    );
    for slot in &team.slots {
        let row = kit.node(
            card,
            Node {
                height: px(44),
                flex_shrink: 0.0,
                padding: UiRect::axes(px(6), px(0)),
                column_gap: px(8),
                align_items: AlignItems::Center,
                ..default()
            },
        );
        kit.commands
            .entity(row)
            .insert(BackgroundColor(Color::BLACK.with_alpha(0.35)));
        if let Some(person) = &slot.who {
            member(kit, row, person, lobby);
        } else {
            let button = w::input(
                kit,
                row,
                &format!("join slot {}", slot.slot),
                Some(Act::LobbySlot(slot.slot)),
                color.with_alpha(0.5),
            );
            kit.commands
                .entity(button)
                .insert(BackgroundColor(Color::NONE))
                .entry::<Node>()
                .and_modify(|mut node| {
                    node.height = px(38);
                    node.justify_content = JustifyContent::Center;
                });
            kit.typed(
                button,
                "JOIN SLOT",
                theme::Type {
                    size: 13.0,
                    tracking: 1.5,
                    ..theme::LABEL
                },
                theme::INK_2,
            );
        }
    }
}

fn member(kit: &mut Kit, parent: Entity, member: &MemberRow, lobby: &LobbyView) {
    let name = if member.me {
        format!("{} (you)", member.name)
    } else {
        member.name.clone()
    };
    let text = kit.text(
        parent,
        name,
        13.0,
        if member.me {
            theme::SELECT
        } else {
            theme::INK_1
        },
    );
    kit.commands.entity(text).insert(Node {
        flex_grow: 1.0,
        min_width: px(0),
        ..default()
    });
    if member.ready {
        let chip = kit.typed(
            parent,
            "READY",
            theme::Type {
                size: 12.0,
                tracking: 1.0,
                ..theme::LABEL
            },
            theme::RADIANT_TEXT,
        );
        kit.commands
            .entity(chip)
            .insert(Name::new(format!("lobby ready chip {}", member.account.0)));
    }
    if lobby.owner && !member.me {
        let move_button = w::quiet(
            kit,
            parent,
            "⇄",
            Act::MoveMember(member.account),
            theme::INK_2,
        );
        kit.commands
            .entity(move_button)
            .insert(Name::new(format!("move {}", member.name.to_lowercase())));
        let kick = w::quiet(
            kit,
            parent,
            "×",
            Act::KickMember(member.account),
            theme::ERROR_TEXT,
        );
        kit.commands
            .entity(kick)
            .insert(Name::new(format!("kick {}", member.name.to_lowercase())));
    }
}

fn spectators(kit: &mut Kit, parent: Entity, lobby: &LobbyView) {
    let row = kit.node(
        parent,
        Node {
            min_height: px(56),
            padding: UiRect::axes(px(10), px(8)),
            align_items: AlignItems::Center,
            column_gap: px(12),
            border: UiRect::all(px(2)),
            ..default()
        },
    );
    kit.commands.entity(row).insert((
        theme::downward(theme::lobby::SPECTATORS),
        BorderColor::all(Color::BLACK),
    ));
    kit.typed(row, "SPECTATING", theme::LABEL, theme::INK_2);
    for member in lobby.spectators.iter().take(8) {
        kit.text(row, &member.name, 13.0, theme::INK_1);
    }
    kit.node(
        row,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    kit.button(
        row,
        "SPECTATE",
        Act::LobbySlot(LOBBY_NO_SLOT),
        Tone::Secondary,
        38.0,
    );
}

fn settings(kit: &mut Kit, parent: Entity, lobby: &LobbyView, service: &str) {
    let panel = w::column(kit, parent, 8.0);
    kit.commands.entity(panel).insert(Node {
        margin: UiRect::top(px(12)),
        flex_direction: FlexDirection::Column,
        row_gap: px(8),
        ..default()
    });
    let head = kit.node(
        panel,
        Node {
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let title = kit.typed(head, "LOBBY SETTINGS", theme::LABEL, theme::INK_1);
    kit.commands.entity(title).insert(Node {
        flex_grow: 1.0,
        ..default()
    });
    if lobby.settings.editable {
        w::quiet(kit, head, "EDIT  ✎", w::action(Action::Edit), theme::SELECT);
    }
    w::rule(kit, panel);
    let rows = kit.node(
        panel,
        Node {
            column_gap: px(16),
            ..default()
        },
    );
    let left = w::column(kit, rows, 6.0);
    kit.commands.entity(left).insert(Node {
        flex_direction: FlexDirection::Column,
        row_gap: px(6),
        width: percent(50),
        ..default()
    });
    setting(kit, left, "GAME MODE:", &lobby.settings.mode);
    setting(
        kit,
        left,
        "PASSWORD:",
        if lobby.locked { "SET" } else { "NONE" },
    );
    setting(kit, left, "OPEN TO:", lobby.settings.audience);
    let right = w::column(kit, rows, 6.0);
    setting(kit, right, "SERVER:", service);
    if let Some(cheats) = lobby.settings.cheats {
        setting(kit, right, "CHEATS:", if cheats { "ON" } else { "OFF" });
    }
}

fn setting(kit: &mut Kit, parent: Entity, label: &str, value: &str) {
    let row = kit.node(
        parent,
        Node {
            column_gap: px(4),
            ..default()
        },
    );
    kit.typed(
        row,
        label,
        theme::Type {
            size: 13.0,
            tracking: 1.0,
            ..theme::CAPTION
        },
        theme::INK_5,
    );
    kit.typed(
        row,
        value.to_uppercase(),
        theme::Type {
            size: 13.0,
            tracking: 1.0,
            ..theme::LABEL
        },
        theme::INK_1,
    );
}

fn footer(kit: &mut Kit, parent: Entity, lobby: &LobbyView) {
    let seated: Vec<_> = lobby
        .teams
        .iter()
        .flat_map(|team| &team.slots)
        .filter_map(|slot| slot.who.as_ref())
        .collect();
    let ready = seated.iter().filter(|member| member.ready).count();
    let status = if let Some(secs) = lobby.starting_in_secs {
        format!("Starting in {secs}")
    } else if ready == seated.len() && ready > 0 {
        format!("All {ready} players are ready.")
    } else {
        format!("{ready} of {} seated players are ready.", seated.len())
    };
    kit.text(parent, status, 13.0, theme::INK_2);
    if let Some(name) = &lobby.moving {
        kit.text(
            parent,
            format!("Choose a free slot for {name}, or SPECTATE"),
            13.0,
            theme::GOLD,
        );
    }
    let row = kit.node(
        parent,
        Node {
            column_gap: px(10),
            ..default()
        },
    );
    let leave = kit.node(
        row,
        Node {
            width: percent(50),
            min_width: px(0),
            ..default()
        },
    );
    w::full_button(
        kit,
        leave,
        "LEAVE LOBBY  ×",
        Act::LeaveLobby,
        Tone::Danger,
        52.0,
    );
    let go = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            ..default()
        },
    );
    let reason = if lobby.owner {
        lobby.waiting.as_ref().map(|reason| reason.message())
    } else if !seated.iter().any(|member| member.me) {
        Some("Join a slot to ready".into())
    } else {
        None
    };
    let label = if lobby.owner {
        "START GAME"
    } else if lobby.me_ready {
        "NOT READY"
    } else {
        "READY"
    };
    if reason.is_some() || lobby.starting_in_secs.is_some() {
        w::full_disabled(kit, go, label, 52.0);
    } else {
        w::full_button(
            kit,
            go,
            label,
            Act::ReadyLobby(if lobby.owner { true } else { !lobby.me_ready }),
            Tone::Primary,
            52.0,
        );
    }
    if let Some(reason) = reason {
        let text = kit.text(parent, reason, 13.0, theme::INK_4);
        kit.commands.entity(text).insert(Node {
            position_type: PositionType::Absolute,
            right: px(24),
            bottom: px(94),
            ..default()
        });
    }
}

fn conversation(kit: &mut Kit, root: Entity, chat: &ChatView) {
    let panel = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: px(24),
            bottom: px(88),
            width: px(theme::lobby::CHAT_WIDTH),
            height: px(300),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(panel).insert((
        Name::new("lobby chat"),
        BackgroundColor(theme::lobby::CHAT.with_alpha(theme::dim(0.88))),
        BorderColor::all(theme::lobby::CHAT_EDGE),
    ));
    let content = kit.node(
        panel,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(0),
            ..default()
        },
    );
    let log = kit.node(
        content,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            padding: UiRect::axes(px(12), px(8)),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::FlexEnd,
            row_gap: px(4),
            overflow: Overflow::clip(),
            ..default()
        },
    );
    for line in chat.lines.iter().rev().take(8).rev() {
        chat_line(kit, log, line, chat);
    }
    people(kit, content, chat);
    chat_input(kit, panel, chat);
}

fn people(kit: &mut Kit, parent: Entity, chat: &ChatView) {
    let people = kit.node(
        parent,
        Node {
            width: px(170),
            flex_shrink: 0.0,
            padding: UiRect::axes(px(10), px(8)),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            border: UiRect::left(px(1)),
            overflow: Overflow::clip(),
            ..default()
        },
    );
    kit.commands
        .entity(people)
        .insert(BorderColor::all(theme::lobby::CHAT_EDGE));
    for person in &chat.people {
        let host = Some(person.account) == chat.owner;
        let name = if host {
            format!("{} ★", person.name)
        } else {
            person.name.clone()
        };
        let text = kit.text(
            people,
            name,
            13.0,
            if host { theme::GOLD } else { theme::INK_1 },
        );
        kit.commands
            .entity(text)
            .insert(Name::new(format!("lobby chat member {}", person.account.0)));
    }
}

fn chat_line(kit: &mut Kit, parent: Entity, line: &Line, chat: &ChatView) {
    let row = kit.node(
        parent,
        Node {
            column_gap: px(8),
            align_items: AlignItems::FlexStart,
            ..default()
        },
    );
    kit.typed(
        row,
        format!("{:02}:{:02}", line.minute / 60, line.minute % 60),
        theme::Type {
            size: 12.0,
            ..theme::NUMBER
        },
        theme::lobby::CLOCK,
    );
    let words = kit.node(
        row,
        Node {
            min_width: px(0),
            flex_grow: 1.0,
            flex_basis: px(0),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        },
    );
    if let Some(account) = line.account {
        let name = kit.bold(
            words,
            format!("{}: ", line.name),
            14.0,
            if Some(account) == chat.owner {
                theme::GOLD
            } else {
                theme::INK_1
            },
        );
        kit.commands
            .entity(name)
            .insert(Name::new(format!("lobby chat speaker {}", account.0)));
        kit.text(words, &line.message, 14.0, theme::INK_1);
    } else {
        let text = kit.text(words, &line.message, 14.0, theme::INK_4);
        kit.commands
            .entity(text)
            .entry::<TextFont>()
            .and_modify(|mut font| font.style = bevy::text::FontStyle::Italic);
    }
}

fn chat_input(kit: &mut Kit, parent: Entity, chat: &ChatView) {
    let row = kit.node(
        parent,
        Node {
            height: px(50),
            flex_shrink: 0.0,
            padding: UiRect::axes(px(8), px(6)),
            column_gap: px(8),
            align_items: AlignItems::Center,
            border: UiRect::top(px(1)),
            ..default()
        },
    );
    kit.commands
        .entity(row)
        .insert(BorderColor::all(theme::lobby::CHAT_EDGE));
    kit.text(row, "To lobby:", 13.0, theme::SELECT);
    let space = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            min_width: px(0),
            ..default()
        },
    );
    let input = w::input(
        kit,
        space,
        "lobby message",
        Some(w::action(Action::FocusChat)),
        if chat.focused {
            theme::FOCUS
        } else {
            theme::SECONDARY_EDGE
        },
    );
    kit.commands
        .entity(input)
        .entry::<Node>()
        .and_modify(|mut node| node.height = px(38));
    kit.text(
        input,
        if chat.input.is_empty() {
            if chat.available {
                "Message"
            } else {
                "Lobby chat is unavailable"
            }
        } else {
            &chat.input
        },
        14.0,
        if chat.input.is_empty() {
            theme::INK_4
        } else {
            theme::INK_1
        },
    );
    if chat.available {
        kit.button(row, "➤", w::action(Action::SendChat), Tone::Secondary, 38.0);
    } else {
        let button = kit.disabled_button(row, "➤", 38.0);
        let mut accessible = bevy::a11y::AccessibilityNode::default();
        accessible.set_description("Lobby chat is unavailable");
        kit.commands.entity(button).insert(accessible);
    }
    if let Some(error) = &chat.error {
        kit.text(parent, error, 13.0, theme::ERROR_TEXT);
    }
}
