//! What shows over a game as well as over the menu: the searches under way
//! with their places in the queue, the matches waiting for a game server,
//! and Dota's MATCH FOUND prompt. Also MY GAMES, which lists the same.

use bevy::prelude::*;

use super::core::Act;
use super::kit::{Kit, Tone};
use super::view::{
    ConfirmView, FinishedRow, GameRow, MenuView, OfferBox, PasswordView, SearchRow, WaitRow,
};
use crate::theme::{self, Type};
use crate::ui;

/// The overlay layer.
pub fn draw(kit: &mut Kit, root: Entity, view: &MenuView) {
    // Match the page layer's visibility, including the dashboard over a live game.
    if view.covers()
        && let Some(status) = &view.status
    {
        status_line(kit, root, status);
    }
    let (searching, waiting) = view.finding();
    if !searching.is_empty() || !waiting.is_empty() {
        // Bottom right over the menu, as Dota's; top right over a game,
        // clear of its panels.
        let corner = if view.covers() {
            // Above the play area when it shows.
            let play_area = view.game.is_some() || view.reconnect;
            Node {
                right: px(48),
                bottom: px(if play_area { 18.0 + 168.0 } else { 18.0 }),
                ..default()
            }
        } else {
            Node {
                right: px(12),
                top: px(64),
                ..default()
            }
        };
        let panel = kit.panel(
            root,
            Node {
                position_type: PositionType::Absolute,
                width: px(300),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14)),
                row_gap: px(10),
                ..corner
            },
        );
        kit.commands.entity(panel).insert(crate::hud::Blocker);
        for (at, search) in searching.iter().enumerate() {
            let which = (searching.len() > 1).then(|| at + 1);
            search_row(kit, panel, search, which);
        }
        for wait in waiting {
            wait_row(kit, panel, wait);
        }
    }
    if let Some(offer) = &view.offer {
        match_found(kit, root, offer);
    }
    if let Some(confirm) = view.confirm.as_ref().filter(|_| view.covers()) {
        popup(kit, root, confirm);
    }
    if let Some(password) = view.password.as_ref().filter(|_| view.covers()) {
        password_popup(kit, root, password);
    }
}

/// Status belongs over the page: inserting a flex row under its tabs moves every hit target.
fn status_line(kit: &mut Kit, root: Entity, status: &super::core::Status) {
    let row = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            bottom: percent(8),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    let line = kit.panel(
        row,
        Node {
            max_width: percent(90),
            padding: UiRect::axes(px(20), px(8)),
            column_gap: px(14),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let color = if status.error { theme::ERROR_TEXT } else { theme::GOLD };
    kit.text(line, status.text.clone(), 15.0, color);
    let dismiss = kit.button(line, "×", Act::Dismiss, Tone::Secondary, 22.0);
    kit.commands
        .entity(dismiss)
        .entry::<Node>()
        .and_modify(|mut node| node.flex_shrink = 0.0);
}

/// Dota's generic popup (`PopupPanel` of popups_shared): the screen dimmed,
/// a dark card with a thin edge, its title, the question, OK and Cancel.
fn popup(kit: &mut Kit, root: Entity, confirm: &ConfirmView) {
    let card = popup_card(kit, root);
    let ink = theme::INK_2;
    let title = kit.typed(card, confirm.title.to_uppercase(), theme::TITLE, theme::INK_1);
    kit.commands.entity(title).insert(Node {
        margin: UiRect::bottom(px(15)),
        ..default()
    });
    let text = kit.text(card, confirm.text, 16.0, ink);
    kit.commands
        .entity(text)
        .insert(TextLayout::justify(Justify::Center));
    popup_answers(kit, card, Act::Answer(true), Act::Answer(false));
}

/// The password of a locked lobby asked in Dota's popup: the lobby's name, the field, what the
/// last try was told, OK and Cancel.
fn password_popup(kit: &mut Kit, root: Entity, password: &PasswordView) {
    let card = popup_card(kit, root);
    let ink = theme::INK_2;
    let title = kit.typed(card, "LOBBY PASSWORD", theme::TITLE, theme::INK_1);
    kit.commands.entity(title).insert(Node {
        margin: UiRect::bottom(px(15)),
        ..default()
    });
    kit.text(
        card,
        format!("{} has a password. Type it to join.", password.lobby),
        20.0,
        ink,
    );
    let column = kit.node(
        card,
        Node {
            width: px(320),
            flex_direction: FlexDirection::Column,
            margin: UiRect::top(px(15)),
            ..default()
        },
    );
    super::pages::field_box(kit, column, 0, &password.field);
    if let Some(error) = &password.error {
        let said = kit.text(card, error.clone(), 18.0, theme::ERROR_TEXT);
        kit.commands.entity(said).insert(Node {
            margin: UiRect::top(px(10)),
            ..default()
        });
    }
    popup_answers(
        kit,
        card,
        Act::LobbyPassword(true),
        Act::LobbyPassword(false),
    );
}

/// The screen dimmed and the popup's card over it.
fn popup_card(kit: &mut Kit, root: Entity) -> Entity {
    let dim = kit
        .commands
        .spawn((
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..ui::fill()
            },
            BackgroundColor(theme::scrim()),
            crate::hud::Blocker,
            Interaction::default(),
            GlobalZIndex(90),
            ChildOf(root),
        ))
        .id();
    kit.commands
        .spawn((
            Node {
                min_width: px(450),
                max_width: px(720),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(64), px(32)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::WINDOW),
            BorderColor::all(theme::WINDOW_EDGE),
            theme::window_shadow(),
            Name::new("popup"),
            ChildOf(dim),
        ))
        .id()
}

/// A popup's OK and Cancel.
fn popup_answers(kit: &mut Kit, card: Entity, ok: Act, cancel: Act) {
    let row = kit.node(
        card,
        Node {
            margin: UiRect::new(px(0), px(0), px(25), px(20)),
            ..default()
        },
    );
    let ok = kit.popup_button(row, "OK", ok);
    kit.commands.entity(ok).insert(Name::new("ok"));
    let cancel = kit.popup_button(row, "Cancel", cancel);
    kit.commands.entity(cancel).insert(Name::new("cancel"));
}

fn search_row(kit: &mut Kit, parent: Entity, search: &SearchRow, which: Option<usize>) {
    let row = kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            ..default()
        },
    );
    let head = kit.node(
        row,
        Node {
            column_gap: px(10),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let title = match which {
        Some(n) => format!("FINDING MATCH {n}"),
        None => "FINDING MATCH".to_owned(),
    };
    let left = kit.node(
        head,
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    kit.bold(left, title, 17.0, theme::INK_1);
    kit.text(left, search.ruleset.clone(), 13.0, theme::INK_4);
    let clock = Type {
        size: 22.0,
        ..theme::NUMBER
    };
    kit.typed(head, search.waited.clone(), clock, theme::INK_1);
    kit.button(head, "×", Act::Cancel(search.entry), Tone::Danger, 30.0);
    if search.position > 0 {
        kit.text(
            row,
            format!(
                "Place in the queue: {} of {}",
                search.position, search.waiting
            ),
            14.0,
            theme::INK_4,
        );
    }
    if let Some(hint) = search.hint {
        kit.text(row, hint, 14.0, theme::INK_4);
    }
    if search.overloaded {
        kit.text(
            row,
            format!(
                "Game servers are full: {} matches wait for a slot",
                search.games_waiting
            ),
            14.0,
            theme::ERROR_TEXT,
        );
    }
}

fn wait_row(kit: &mut Kit, parent: Entity, wait: &WaitRow) {
    let row = kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            ..default()
        },
    );
    kit.bold(row, "WAITING FOR A GAME SERVER", 16.0, theme::INK_1);
    let ahead = match wait.ahead {
        0 => "Starting it now".to_owned(),
        1 => "1 match ahead".to_owned(),
        n => format!("{n} matches ahead"),
    };
    kit.text(
        row,
        format!("Match {} · {ahead}", wait.match_id.0),
        14.0,
        theme::INK_4,
    );
    if wait.overloaded {
        kit.text(row, "Every game server is busy right now", 14.0, theme::ERROR_TEXT);
    }
}

/// Dota's prompt: YOUR GAME IS READY, the time to answer running down, how
/// many accepted, and Accept / Decline.
fn match_found(kit: &mut Kit, root: Entity, offer: &OfferBox) {
    let shade = kit
        .commands
        .spawn((
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..ui::fill()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            crate::hud::Blocker,
            Interaction::default(),
            GlobalZIndex(80),
            ChildOf(root),
        ))
        .id();
    let card = kit.panel(
        shade,
        Node {
            width: px(560),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::all(px(28)),
            row_gap: px(14),
            ..default()
        },
    );
    // Opaque: nothing of the page may read through the prompt.
    kit.commands
        .entity(card)
        .insert(BackgroundColor(Color::srgb(0.06, 0.07, 0.09)));
    kit.bold(card, "YOUR GAME IS READY", 34.0, theme::INK_1);
    let which = offer
        .which
        .as_deref()
        .map_or(String::new(), |w| format!(" · {w}"));
    kit.text(card, format!("{}{which}", offer.ruleset), 16.0, theme::INK_4);
    let track = kit
        .commands
        .spawn((
            Node {
                width: px(480),
                height: px(8),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(theme::FIELD),
            ChildOf(card),
        ))
        .id();
    kit.commands.spawn((
        Node {
            width: percent(offer.left_permille as f32 / 10.0),
            height: percent(100),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(if offer.left_secs > 5 { theme::GO.0 } else { theme::STOP.0 }),
        ChildOf(track),
    ));
    kit.bold(card, format!("{}", offer.left_secs), 30.0, theme::GOLD);
    let dots: String = (0..offer.of)
        .map(|i| if i < offer.accepted { '•' } else { '·' })
        .collect();
    kit.text(
        card,
        format!("{dots}   {} of {} accepted", offer.accepted, offer.of),
        16.0,
        theme::INK_1,
    );
    let row = kit.node(
        card,
        Node {
            column_gap: px(16),
            margin: UiRect::top(px(6)),
            ..default()
        },
    );
    if offer.answered {
        kit.text(
            row,
            "Accepted: waiting for the other players…",
            16.0,
            theme::RADIANT_TEXT,
        );
    } else {
        kit.button(row, "ACCEPT", Act::Accept(offer.proposal), Tone::Primary, 40.0);
        kit.button(row, "DECLINE", Act::Decline(offer.proposal), Tone::Danger, 40.0);
    }
    if !offer.answered {
        kit.text(
            card,
            "Declining keeps you out of the queue for a while.",
            13.0,
            theme::INK_4,
        );
    }
}

/// MY GAMES: every unfinished match of the account with Rejoin and Watch,
/// every search with Cancel, and the cooldown.
pub fn my_games(
    kit: &mut Kit,
    body: Entity,
    view: &MenuView,
    games: &[GameRow],
    finished: &[FinishedRow],
    cooldown: Option<&str>,
) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            width: px(640),
            ..default()
        },
    );
    kit.bold(column, "MY GAMES", 30.0, theme::INK_1);
    if games.is_empty() && view.searches.is_empty() && view.waits.is_empty() {
        kit.text(
            column,
            "Nothing going: find a match under PLAY.",
            16.0,
            theme::INK_4,
        );
    }
    for game in games {
        let row = kit.panel(
            column,
            Node {
                padding: UiRect::all(px(12)),
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
        kit.bold(label, game.label.clone(), 18.0, theme::INK_1);
        let state = if game.ready {
            "In progress"
        } else {
            "Starting"
        };
        kit.text(label, state, 14.0, theme::INK_4);
        if game.ready {
            kit.button(row, "REJOIN", Act::Rejoin(game.match_id), Tone::Primary, 38.0);
            kit.button(row, "WATCH", Act::Watch(game.match_id), Tone::Secondary, 38.0);
        }
    }
    for search in &view.searches {
        let row = kit.panel(
            column,
            Node {
                padding: UiRect::all(px(12)),
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
        kit.bold(
            label,
            format!("Searching: {}", search.ruleset),
            18.0,
            theme::INK_1,
        );
        kit.text(
            label,
            format!(
                "{} · place {} of {}",
                search.waited, search.position, search.waiting
            ),
            14.0,
            theme::INK_4,
        );
        kit.button(row, "CANCEL", Act::Cancel(search.entry), Tone::Danger, 38.0);
    }
    if !finished.is_empty() {
        kit.bold(column, "FINISHED", 20.0, theme::INK_1);
    }
    for game in finished {
        let row = kit.panel(
            column,
            Node {
                padding: UiRect::all(px(12)),
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
        let color = match game.won {
            Some(true) => theme::RADIANT_TEXT,
            Some(false) => theme::ERROR_TEXT,
            None => theme::INK_1,
        };
        let title = if game.rating.is_empty() {
            game.headline.clone()
        } else {
            format!("{}  {}", game.headline, game.rating)
        };
        kit.bold(label, title, 18.0, color);
        kit.text(label, game.detail.clone(), 14.0, theme::INK_4);
        if game.replay {
            kit.button(
                row,
                "WATCH REPLAY",
                Act::WatchReplay(game.match_id),
                Tone::Secondary,
                38.0,
            );
        }
    }
    if let Some(cooldown) = cooldown {
        kit.text(column, cooldown, 15.0, theme::ERROR_TEXT);
    }
}
