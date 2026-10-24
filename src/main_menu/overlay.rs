//! What shows over a game as well as over the menu: the searches under way
//! with their places in the queue, the matches waiting for a game server,
//! and Dota's MATCH FOUND prompt. Also MY GAMES, which lists the same.

use bevy::prelude::*;

use super::core::Act;
use super::screens::{ERROR, GREEN, Kit, PLAIN, RED};
use super::view::{ConfirmView, GameRow, MenuView, OfferBox, SearchRow, WaitRow};
use crate::ui;

/// The overlay layer.
pub fn draw(kit: &mut Kit, root: Entity, view: &MenuView) {
    if !view.searches.is_empty() || !view.waits.is_empty() {
        // Bottom right over the menu, as Dota's; top right over a game,
        // clear of its panels.
        let corner = if view.covers() {
            // Above the play area when it shows.
            let play_area = view.game.is_some() || view.reconnect;
            Node {
                right: px(24),
                bottom: px(if play_area { 24.0 + 128.0 } else { 24.0 }),
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
                width: px(340),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14)),
                row_gap: px(10),
                ..corner
            },
        );
        kit.commands.entity(panel).insert(crate::hud::Blocker);
        for (at, search) in view.searches.iter().enumerate() {
            let which = (view.searches.len() > 1).then(|| at + 1);
            search_row(kit, panel, search, which);
        }
        for wait in &view.waits {
            wait_row(kit, panel, wait);
        }
    }
    if let Some(offer) = &view.offer {
        match_found(kit, root, offer);
    }
    if let Some(confirm) = view.confirm.as_ref().filter(|_| view.covers()) {
        popup(kit, root, confirm);
    }
}

/// Dota's generic popup (`PopupPanel` of popups_shared): the screen dimmed,
/// a dark card with a thin edge, its title, the question, OK and Cancel.
fn popup(kit: &mut Kit, root: Entity, confirm: &ConfirmView) {
    let dim = kit
        .commands
        .spawn((
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..ui::fill()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            crate::hud::Blocker,
            Interaction::default(),
            GlobalZIndex(90),
            ChildOf(root),
        ))
        .id();
    let card = kit
        .commands
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
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                Color::srgba_u8(0x27, 0x2d, 0x2e, 0xee).into(),
                Color::srgba_u8(0x13, 0x16, 0x17, 0xee).into(),
            ])),
            BorderColor::all(Color::srgb_u8(0x5e, 0x68, 0x69)),
            BoxShadow::new(Color::BLACK, px(0), px(0), px(0), px(8)),
            Name::new("popup"),
            ChildOf(dim),
        ))
        .id();
    let ink = Color::srgb_u8(0xaf, 0xb4, 0xb4);
    let title = kit.text(card, confirm.title, 30.0, ink);
    kit.commands.entity(title).insert(Node {
        margin: UiRect::bottom(px(15)),
        ..default()
    });
    let text = kit.text(card, confirm.text, 20.0, ink);
    kit.commands
        .entity(text)
        .insert(TextLayout::justify(Justify::Center));
    let row = kit.node(
        card,
        Node {
            margin: UiRect::new(px(0), px(0), px(25), px(20)),
            ..default()
        },
    );
    let ok = kit.popup_button(row, "OK", Act::Answer(true));
    kit.commands.entity(ok).insert(Name::new("ok"));
    let cancel = kit.popup_button(row, "Cancel", Act::Answer(false));
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
    kit.bold(left, title, 17.0, ui::INK);
    kit.text(left, search.ruleset.clone(), 13.0, ui::INK_DIM);
    kit.bold(head, search.waited.clone(), 22.0, ui::GOLDEN);
    kit.button(head, "×", Act::Cancel(search.entry), RED, 30.0);
    if search.position > 0 {
        kit.text(
            row,
            format!(
                "Place in the queue: {} of {}",
                search.position, search.waiting
            ),
            14.0,
            ui::INK_DIM,
        );
    }
    if search.overloaded {
        kit.text(
            row,
            format!(
                "Game servers are full: {} matches wait for a slot",
                search.games_waiting
            ),
            14.0,
            ERROR,
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
    kit.bold(row, "WAITING FOR A GAME SERVER", 16.0, ui::INK);
    let ahead = match wait.ahead {
        0 => "Starting it now".to_owned(),
        1 => "1 match ahead".to_owned(),
        n => format!("{n} matches ahead"),
    };
    kit.text(
        row,
        format!("Match {} · {ahead}", wait.match_id.0),
        14.0,
        ui::INK_DIM,
    );
    if wait.overloaded {
        kit.text(row, "Every game server is busy right now", 14.0, ERROR);
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
    kit.bold(card, "YOUR GAME IS READY", 34.0, ui::INK);
    let which = offer
        .which
        .as_deref()
        .map_or(String::new(), |w| format!(" · {w}"));
    kit.text(card, format!("{}{which}", offer.ruleset), 16.0, ui::INK_DIM);
    let track = kit
        .commands
        .spawn((
            Node {
                width: px(480),
                height: px(8),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
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
        BackgroundColor(if offer.left_secs > 5 { GREEN.1 } else { RED.1 }),
        ChildOf(track),
    ));
    kit.bold(card, format!("{}", offer.left_secs), 30.0, ui::GOLDEN);
    let dots: String = (0..offer.of)
        .map(|i| if i < offer.accepted { '•' } else { '·' })
        .collect();
    kit.text(
        card,
        format!("{dots}   {} of {} accepted", offer.accepted, offer.of),
        16.0,
        ui::INK,
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
            ui::HP_MINE,
        );
    } else {
        kit.button(row, "ACCEPT", Act::Accept(offer.proposal), GREEN, 56.0);
        kit.button(row, "DECLINE", Act::Decline(offer.proposal), RED, 56.0);
    }
    if !offer.answered {
        kit.text(
            card,
            "Declining keeps you out of the queue for a while.",
            13.0,
            ui::INK_DIM,
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
    kit.bold(column, "MY GAMES", 30.0, ui::INK);
    if games.is_empty() && view.searches.is_empty() && view.waits.is_empty() {
        kit.text(
            column,
            "Nothing going: find a match under PLAY.",
            16.0,
            ui::INK_DIM,
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
        kit.bold(label, game.label.clone(), 18.0, ui::INK);
        let state = if game.ready {
            "In progress"
        } else {
            "Starting"
        };
        kit.text(label, state, 14.0, ui::INK_DIM);
        if game.ready {
            kit.button(row, "REJOIN", Act::Rejoin(game.match_id), GREEN, 38.0);
            kit.button(row, "WATCH", Act::Watch(game.match_id), PLAIN, 38.0);
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
            ui::INK,
        );
        kit.text(
            label,
            format!(
                "{} · place {} of {}",
                search.waited, search.position, search.waiting
            ),
            14.0,
            ui::INK_DIM,
        );
        kit.button(row, "CANCEL", Act::Cancel(search.entry), RED, 38.0);
    }
    if let Some(cooldown) = cooldown {
        kit.text(column, cooldown, 15.0, ERROR);
    }
}
