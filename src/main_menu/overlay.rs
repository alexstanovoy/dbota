//! What shows over a game as well as over the menu: the searches under way
//! with their elapsed time, the matches waiting for a game server,
//! and Dota's MATCH FOUND prompt. Also MY GAMES, which lists the same.

use bevy::picking::hover::{DirectlyHovered, Hovered};
use bevy::prelude::*;

use super::core::Act;
use super::kit::{Kit, Tone};
use super::view::{
    ConfirmView, FinishedRow, GameRow, MenuView, OfferBox, PasswordView, SearchRow, WaitRow,
};
use crate::theme;
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
            Node {
                right: px(theme::CORNER_RIGHT),
                bottom: px(theme::CORNER_BOTTOM
                    + super::pages::corner_height(view)
                    + theme::CORNER_GAP),
                ..default()
            }
        } else {
            Node {
                right: px(12),
                top: px(64),
                ..default()
            }
        };
        let panel = kit.node(
            root,
            Node {
                position_type: PositionType::Absolute,
                width: px(theme::CORNER_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                ..corner
            },
        );
        kit.commands.entity(panel).insert((
            crate::hud::Blocker,
            Hovered::default(),
            Pickable::default(),
            Name::new("search overlay"),
        ));
        for search in searching {
            search_row(kit, panel, search);
        }
        for wait in waiting {
            wait_row(kit, panel, wait);
        }
    }
    if let Some(offer) = &view.offer {
        match_found(
            kit,
            root,
            offer,
            view.play.info == Some(super::PlayInfo::Decline),
        );
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
    let color = if status.error {
        theme::ERROR_TEXT
    } else {
        theme::GOLD
    };
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
    let title = kit.typed(
        card,
        confirm.title.to_uppercase(),
        theme::TITLE,
        theme::INK_1,
    );
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
            Hovered::default(),
            DirectlyHovered::default(),
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
            column_gap: px(12),
            margin: UiRect::new(px(0), px(0), px(25), px(20)),
            ..default()
        },
    );
    let ok = kit.popup_button(row, "OK", ok);
    kit.commands.entity(ok).insert(Name::new("ok"));
    let cancel = kit.popup_button(row, "Cancel", cancel);
    kit.commands.entity(cancel).insert(Name::new("cancel"));
}

fn search_row(kit: &mut Kit, parent: Entity, search: &SearchRow) {
    let row = kit.node(
        parent,
        Node {
            width: percent(100),
            min_height: px(theme::FINDING_HEIGHT),
            align_items: AlignItems::Center,
            padding: UiRect::left(px(14)),
            column_gap: px(10),
            border: UiRect::axes(px(0), px(1)),
            ..default()
        },
    );
    kit.commands.entity(row).insert((
        Name::new("finding match"),
        BackgroundColor(theme::FINDING),
        BorderColor::all(theme::SECONDARY_EDGE),
    ));
    search_title(kit, row, search);
    kit.typed(row, &search.waited, theme::FINDING_CLOCK, theme::INK_1);
    let cancel = kit.quiet(
        row,
        "×",
        Act::Cancel(search.entry),
        theme::CLOSE_GLYPH,
        (theme::INK_1, theme::INK_3),
    );
    kit.commands
        .entity(cancel)
        .insert(Name::new("cancel search"));
    kit.commands
        .entity(cancel)
        .entry::<Node>()
        .and_modify(|mut n| {
            n.width = px(44);
            n.justify_content = JustifyContent::Center;
        });
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
    kit.text(row, "Starting your game", 14.0, theme::INK_4);
    if wait.overloaded {
        kit.text(
            row,
            "Every game server is busy right now",
            14.0,
            theme::ERROR_TEXT,
        );
    }
}

/// Dota's prompt: YOUR GAME IS READY, the time to answer running down, how
/// many accepted, and Accept / Decline.
fn match_found(kit: &mut Kit, root: Entity, offer: &OfferBox, tip: bool) {
    let card = offer_card(kit, root);
    offer_time(kit, card, offer);
    offer_title(kit, card, offer);
    let row = kit.node(
        card,
        Node {
            height: px(64),
            margin: UiRect::top(px(8)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            column_gap: px(8),
            ..default()
        },
    );
    if offer.answered {
        accept_squares(kit, row, offer);
    } else {
        let button = kit.button(
            row,
            "ACCEPT",
            Act::Accept(offer.proposal),
            Tone::Primary,
            64.0,
        );
        kit.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(|mut n| n.width = px(320));
        decline(kit, card, offer, tip);
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
            kit.button(
                row,
                "REJOIN",
                Act::Rejoin(game.match_id),
                Tone::Primary,
                38.0,
            );
            kit.button(
                row,
                "WATCH",
                Act::Watch(game.match_id),
                Tone::Secondary,
                38.0,
            );
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
        kit.text(label, search.waited.clone(), 14.0, theme::INK_4);
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

fn offer_card(kit: &mut Kit, root: Entity) -> Entity {
    let shade = kit.node(root, crate::ui::fill());
    kit.commands.entity(shade).insert((
        BackgroundColor(Color::BLACK.with_alpha(theme::dim(0.45))),
        crate::hud::Blocker,
        Pickable::default(),
        Hovered::default(),
        DirectlyHovered::default(),
        GlobalZIndex(80),
    ));
    let card = kit.node(
        shade,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(100.0 / 3.0),
            margin: UiRect::left(px(-280)),
            width: px(560),
            height: px(248),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            padding: UiRect::new(px(32), px(32), px(30), px(12)),
            row_gap: px(18),
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(card).insert((
        Name::new("match found"),
        theme::downward(theme::ACCEPT_PANEL),
        BorderColor::all(theme::SECONDARY_EDGE),
        theme::window_shadow(),
    ));
    card
}

fn offer_time(kit: &mut Kit, card: Entity, offer: &OfferBox) {
    let Some(left) = offer.left_permille else {
        return;
    };
    let track = kit.node(
        card,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(-1),
            height: px(3),
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    let line = kit.node(
        track,
        Node {
            width: percent(left.min(1000) as f32 / 10.0),
            height: percent(100),
            ..default()
        },
    );
    let ratio = left.min(1000) as f32 / 1000.0;
    let color = theme::ACCEPT_TIME
        .1
        .to_srgba()
        .mix(&theme::ACCEPT_TIME.0.to_srgba(), ratio);
    kit.commands
        .entity(line)
        .insert((Name::new("accept time"), BackgroundColor(color.into())));
}

fn offer_title(kit: &mut Kit, card: Entity, offer: &OfferBox) {
    kit.typed(card, "YOUR GAME IS READY", theme::OFFER_TITLE, theme::INK_1);
    let mode = kit.typed(card, &offer.ruleset, theme::LARGE_BODY, theme::INK_3);
    kit.commands.entity(mode).insert(Node {
        margin: UiRect::top(px(-10)),
        ..default()
    });
}

fn decline(kit: &mut Kit, card: Entity, offer: &OfferBox, tip: bool) {
    let decline = kit.node(
        card,
        Node {
            position_type: PositionType::Absolute,
            right: px(8),
            bottom: px(4),
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        },
    );
    let link = kit.quiet(
        decline,
        "Decline",
        Act::Decline(offer.proposal),
        theme::SMALL_BODY,
        (theme::DECLINE, theme::DIRE_TEXT),
    );
    kit.commands
        .entity(link)
        .entry::<Node>()
        .and_modify(|mut node| node.min_height = px(32));
    super::play::info(kit, decline, super::PlayInfo::Decline, "About declining");
    if let Some(cooldown) = offer
        .decline_cooldown_ms
        .filter(|cooldown| tip && *cooldown > 0)
    {
        let duration = if cooldown >= 60_000 {
            format!("{} min", cooldown.div_ceil(60_000))
        } else {
            format!("{} sec", cooldown.div_ceil(1000))
        };
        super::play::tooltip(
            kit,
            decline,
            &format!("Declining bans you from matchmaking for {duration}."),
            true,
        );
    }
}

fn accept_squares(kit: &mut Kit, row: Entity, offer: &OfferBox) {
    for i in 0..offer.of.min(10) {
        let square = kit.node(
            row,
            Node {
                width: px(34),
                height: px(34),
                border: UiRect::all(px(1)),
                ..default()
            },
        );
        kit.commands.entity(square).insert((
            Name::new(format!("accept-player-{i}")),
            BackgroundColor(if i < offer.accepted {
                theme::GO.0
            } else {
                theme::ACCEPT_EMPTY
            }),
            BorderColor::all(if i < offer.accepted {
                theme::RADIANT
            } else {
                theme::TOOLTIP_EDGE
            }),
        ));
        if i < offer.accepted {
            kit.commands
                .entity(square)
                .insert(theme::downward(theme::GO));
        }
    }
}

fn search_title(kit: &mut Kit, row: Entity, search: &SearchRow) {
    let words = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    let title = kit.typed(
        words,
        if search.overloaded {
            "SERVERS FULL"
        } else {
            "FINDING MATCH"
        },
        theme::FINDING_TITLE,
        theme::INK_1,
    );
    kit.commands
        .entity(title)
        .insert(TextColor(if search.overloaded {
            theme::GOLD
        } else {
            theme::INK_1
        }));
    kit.commands.entity(title).insert(TextLayout {
        linebreak: LineBreak::NoWrap,
        ..default()
    });
    let caption = kit.typed(words, &search.ruleset, theme::SMALL_BODY, theme::INK_2);
    kit.commands.entity(caption).insert((
        Node {
            overflow: Overflow::clip(),
            ..default()
        },
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
    ));
    if let Some(hint) = search.hint {
        kit.typed(words, hint, theme::BODY, theme::INK_3);
    }
}
