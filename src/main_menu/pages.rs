//! The pages: the top bar over the menu's background, the page's column under it, the player's
//! card at the bottom left and the game's corner at the bottom right; the way in (the service,
//! signing in, registering) on the background alone, a card in the middle.

use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bota_core::HEROES;

use super::backdrop::{self, Backdrop};
use super::core::{Act, LocalRules, Page};
use super::form::FormKind;
use super::kit::{Kit, MenuButton, Tone};
use super::view::{FieldView, MenuView, PageView, ReplayRow, ReplayShelf, ResultView};
use crate::theme;

/// The pages' column: the forms and the replay list, one width.
const PAGE_WIDTH: f32 = 760.0;
/// Where every page starts under the top bar.
const PAGE_TOP: f32 = 64.0;
/// Room left under a page for the player's card and the game's corner.
const PAGE_FOOT: f32 = 96.0;
/// The way in's card, and where its column starts.
const WAY_IN_WIDTH: f32 = 480.0;
const WAY_IN_TOP: f32 = 110.0;

/// The scrolled part of a page, under the top bar.
#[derive(Component)]
pub struct MenuBody;

/// The whole page layer.
pub fn draw(
    kit: &mut Kit,
    root: Entity,
    view: &MenuView,
    fold: super::top_bar::TabFold,
    texts: &crate::dota_text::Texts,
    clock: super::page_time::Clock,
) {
    if loading_or_login(kit, root, view) {
        return;
    }
    page_background(kit, root, view);
    super::top_bar::draw(kit, root, view, fold);
    if let PageView::MatchHistory(history) = &view.body {
        super::history_page::draw(kit, root, history);
        return;
    }
    if super::lobbies::draw(kit, root, view) {
        player_card(kit, root, view);
        super::top_bar::quit(kit, root, view);
        return;
    }
    let body = page_body(kit, root);
    if !draw_account_page(kit, root, body, view, texts, clock) {
        draw_body(kit, body, view);
    }
    page_corners(kit, root, view, clock);
}

fn loading_or_login(kit: &mut Kit, root: Entity, view: &MenuView) -> bool {
    if let PageView::Loading {
        details,
        progress_permille,
        elapsed_secs,
    } = &view.body
    {
        crate::loading::draw(
            kit,
            root,
            details,
            f32::from(*progress_permille) / 1000.0,
            *elapsed_secs,
        );
        return true;
    }
    let way_in = matches!(
        &view.body,
        PageView::Form {
            kind: FormKind::Service | FormKind::Login | FormKind::Register,
            ..
        }
    );
    if way_in {
        backdrop::draw(kit, root, Backdrop::of(view.side), 0.0);
        if let PageView::Form {
            kind,
            fields,
            busy,
            plain_host,
            ..
        } = &view.body
        {
            way_in_page(kit, root, *kind, fields, *busy, plain_host.as_deref());
        }
        super::top_bar::quit(kit, root, view);
        return true;
    }
    false
}

fn page_background(kit: &mut Kit, root: Entity, view: &MenuView) {
    let account_page = matches!(
        view.body,
        PageView::Profile { .. }
            | PageView::Heroes
            | PageView::Hero { .. }
            | PageView::Leaderboard { .. }
    );
    kit.commands
        .entity(root)
        .insert(BackgroundColor(if account_page {
            theme::ACCOUNT_PAGE
        } else {
            theme::GROUND
        }));
    if !account_page {
        backdrop::draw(kit, root, Backdrop::of(view.side), theme::BAR_HEIGHT);
    }
    hero_backdrop(kit, root, view);
}

fn hero_backdrop(kit: &mut Kit, root: Entity, view: &MenuView) {
    if matches!(view.body, PageView::Heroes | PageView::Hero { .. }) {
        let field = kit.node(
            root,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(54),
                bottom: px(0),
                ..default()
            },
        );
        kit.commands
            .entity(field)
            .insert(if matches!(view.body, PageView::Heroes) {
                theme::catalogue_ground()
            } else {
                theme::hero_field(false)
            });
        if matches!(view.body, PageView::Heroes) {
            let light = kit.node(field, crate::ui::fill());
            kit.commands.entity(light).insert(theme::catalogue_field());
        }
        if matches!(view.body, PageView::Hero { .. }) {
            let warm = kit.node(
                root,
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    right: px(0),
                    top: px(54),
                    bottom: px(0),
                    ..default()
                },
            );
            kit.commands.entity(warm).insert(theme::hero_field(true));
        }
    }
    if let PageView::Hero { hero, .. } = view.body {
        // Over the page's ground and under its words: the hero stands in the middle.
        super::hero_stage::draw(kit, root, hero, super::hero_stage::Anchor::Centre);
    }
}

fn page_body(kit: &mut Kit, root: Entity) -> Entity {
    let body = kit.node(
        root,
        Node {
            flex_grow: 1.0,
            flex_basis: px(0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::FlexStart,
            padding: UiRect {
                left: px(24),
                right: px(24),
                top: px(PAGE_TOP - theme::BAR_HEIGHT),
                bottom: px(PAGE_FOOT),
            },
            overflow: Overflow::scroll_y(),
            ..default()
        },
    );
    kit.commands
        .entity(body)
        .insert((MenuBody, ScrollPosition::default()));
    body
}

fn draw_account_page(
    kit: &mut Kit,
    root: Entity,
    body: Entity,
    view: &MenuView,
    texts: &crate::dota_text::Texts,
    clock: super::page_time::Clock,
) -> bool {
    match &view.body {
        PageView::Heroes => super::catalogue::grid(kit, root),
        PageView::Hero { hero, demo } => {
            super::catalogue::hero(kit, body, *hero, texts, *demo);
        }
        PageView::Leaderboard { board, field } => {
            kit.commands.entity(body).remove::<MenuBody>();
            super::ladder_page::draw(kit, root, board, field);
        }
        PageView::Profile {
            name,
            ratings,
            matches,
            mode_names,
            avatar,
            read_only,
            notice,
        } => {
            super::account_page::profile(
                kit,
                root,
                super::account_page::Profile {
                    name,
                    ratings,
                    matches,
                    mode_names,
                    texts,
                    avatar,
                    read_only: *read_only,
                    connected: matches!(view.connection, super::view::Connection::Online(_)),
                    notice: notice.as_deref(),
                },
                clock,
            );
            if !*read_only && view.offer.is_none() {
                super::avatar_draw::dialog(kit, root, avatar);
            }
        }
        _ => return false,
    }
    true
}

fn draw_body(kit: &mut Kit, body: Entity, view: &MenuView) {
    match &view.body {
        PageView::Form {
            kind,
            fields,
            busy,
            plain_host,
            rules,
            hosted_at,
        } => {
            form_page(
                kit,
                body,
                *kind,
                fields,
                *busy,
                plain_host.as_deref(),
                *rules,
                hosted_at.as_deref(),
                (*kind == FormKind::Host).then_some(view.host_cheats),
            );
        }
        PageView::Home { .. } => {}
        PageView::MyGames {
            games,
            finished,
            cooldown,
        } => super::overlay::my_games(kit, body, view, games, finished, cooldown.as_deref()),
        PageView::Lobbies { .. } | PageView::Lobby(_) => {}
        PageView::Result(result) => result_page(kit, body, result),
        PageView::Replays {
            list,
            fields,
            shelf,
        } => replays_page(kit, body, list, fields, shelf),
        #[cfg(not(target_arch = "wasm32"))]
        PageView::DemoHero { heroes, offered } => demo_page(kit, body, heroes, *offered),
        PageView::Joining => {
            kit.typed(body, "JOINING THE GAME…", theme::TITLE, theme::INK_1);
        }
        PageView::Hidden => {}
        PageView::Loading { .. } => {}
        _ => {}
    }
}

fn page_corners(kit: &mut Kit, root: Entity, view: &MenuView, clock: super::page_time::Clock) {
    if let PageView::Home { hero, matches, .. } = &view.body {
        super::home_cards::stage(kit, root, *hero);
        // Play's panel covers the right edge, where the cards would show only as clipped strips.
        if !view.play.open {
            super::home_cards::cards(kit, root, matches, clock);
        }
    }
    if !matches!(view.body, PageView::Profile { .. }) {
        player_card(kit, root, view);
    }
    if !matches!(
        view.body,
        PageView::Profile { .. }
            | PageView::Heroes
            | PageView::Hero { .. }
            | PageView::Leaderboard { .. }
    ) {
        game_corner(kit, root, view);
    }
    if let PageView::Home {
        rulesets,
        in_flight,
        cooldown,
        ..
    } = &view.body
    {
        super::play::draw(kit, root, view, rulesets, *in_flight, cooldown.as_deref());
    }
    super::top_bar::quit(kit, root, view);
}

/// The player at the bottom left: a square with their initial and their name; with an account,
/// a click opens the profile.
fn player_card(kit: &mut Kit, root: Entity, view: &MenuView) {
    let card = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(24),
                bottom: px(theme::CORNER_BOTTOM),
                column_gap: px(12),
                padding: UiRect::new(px(4), px(16), px(4), px(4)),
                align_items: AlignItems::Center,
                border: UiRect::left(px(2)),
                ..default()
            },
            theme::rightward((theme::SELF_ROW, theme::SELF_ROW.with_alpha(0.0))),
            BorderColor::all(theme::TAB_MARK),
            Name::new("player"),
            ChildOf(root),
        ))
        .id();
    kit.commands
        .entity(card)
        .insert((MenuButton(Act::Go(Page::Profile)), Button));
    let avatar = kit
        .commands
        .spawn((
            Node {
                width: px(52),
                height: px(52),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..default()
            },
            theme::downward(theme::AVATAR),
            BorderColor::all(theme::AVATAR_EDGE),
            Pickable::IGNORE,
            ChildOf(card),
        ))
        .id();
    let initial: String = view
        .player
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    kit.typed(avatar, initial, theme::INITIAL, theme::INK_1);
    kit.typed(card, view.player.clone(), theme::PLAYER_NAME, theme::INK_1);
}

/// The corner where Dota's play button stands: over a game, LEAVE GAME and DISCONNECT above
/// RETURN TO GAME; after leaving one's seat, LEAVE above RECONNECT.
fn game_corner(kit: &mut Kit, root: Entity, view: &MenuView) {
    if view.game.is_none() && !view.reconnect {
        if !view.play.open || view.offer.is_some() {
            super::play::corner(kit, root, matches!(view.body, PageView::Home { .. }));
        }
        return;
    }
    let corner = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(theme::CORNER_RIGHT),
            bottom: px(theme::CORNER_BOTTOM),
            width: px(theme::CORNER_WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(theme::CORNER_GAP),
            ..default()
        },
    );
    let wide = |kit: &mut Kit, label: &str, act: Act, tone: Tone, height: f32, name: &str| {
        let button = kit.button(corner, label, act, tone, height);
        kit.commands
            .entity(button)
            .insert(Name::new(name.to_owned()));
        kit.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(|mut node| node.width = percent(100));
    };
    if view.game.is_some() {
        // A replay is left as a game watched is (Dota's popup_confirm_close_replay).
        if view.can_leave_game {
            wide(
                kit,
                "LEAVE GAME",
                Act::LeaveGame,
                Tone::Danger,
                theme::HIT,
                "leave game",
            );
        }
        wide(
            kit,
            "DISCONNECT",
            Act::Disconnect,
            Tone::Danger,
            theme::HIT,
            "disconnect",
        );
        wide(
            kit,
            "RETURN TO GAME",
            Act::ReturnToGame,
            Tone::Primary,
            theme::ACTION_HEIGHT,
            "return",
        );
    } else {
        // Dota's LEAVE: a way to be done with a game that cannot be taken again (its server died).
        wide(
            kit,
            "LEAVE",
            Act::LeaveSeat,
            Tone::Danger,
            theme::HIT,
            "leave match",
        );
        wide(
            kit,
            "RECONNECT",
            Act::Reconnect,
            Tone::Primary,
            theme::ACTION_HEIGHT,
            "reconnect",
        );
    }
}

/// The way in (the service's address, signing in, registering): Dota's logo and DBOTA over a
/// card of the form's fields and buttons, in the middle of the background.
fn way_in_page(
    kit: &mut Kit,
    root: Entity,
    kind: FormKind,
    fields: &[FieldView],
    busy: bool,
    plain: Option<&str>,
) {
    let column = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(WAY_IN_TOP),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(28),
            ..default()
        },
    );
    let mark = kit.node(
        column,
        Node {
            column_gap: px(16),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.art(
        mark,
        "menu.logo",
        Node {
            width: px(60),
            height: px(60),
            ..default()
        },
    );
    kit.typed(mark, "DBOTA", theme::WAY_IN_WORDMARK, theme::INK_1);
    let card = form_page(kit, column, kind, fields, busy, plain, None, None, None);
    kit.commands
        .entity(card)
        .entry::<Node>()
        .and_modify(|mut node| node.width = px(WAY_IN_WIDTH));
    kit.quiet(
        column,
        "Play without an account",
        Act::Go(WITHOUT_ACCOUNT),
        theme::LINK_TEXT,
        theme::LINK,
    );
}

/// Where a player without an account plays: the local lobby; in a browser, which hosts no game,
/// the replays.
#[cfg(not(target_arch = "wasm32"))]
const WITHOUT_ACCOUNT: Page = Page::LocalLobby;
#[cfg(target_arch = "wasm32")]
const WITHOUT_ACCOUNT: Page = Page::Replays;

/// A form: a card of fields in the middle of the page.
#[allow(clippy::too_many_arguments)]
fn form_page(
    kit: &mut Kit,
    body: Entity,
    kind: FormKind,
    fields: &[FieldView],
    busy: bool,
    plain: Option<&str>,
    rules: Option<LocalRules>,
    hosted_at: Option<&str>,
    cheats: Option<bool>,
) -> Entity {
    let card = kit.panel(
        body,
        Node {
            width: px(PAGE_WIDTH),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(28)),
            row_gap: px(14),
            ..default()
        },
    );
    form_heading(kit, card, kind);
    let hosting = kind == FormKind::Host;
    // The local lobby's address is below the lobby it hosts.
    let shown = if hosting { 1 } else { fields.len() };
    for (at, field) in fields.iter().enumerate().take(shown) {
        field_box(kit, card, at, field);
    }
    if busy {
        kit.text(card, "Connecting to the service…", 15.0, theme::GOLD);
    }
    if let Some(chosen) = rules.filter(|_| hosting) {
        local_rules(kit, card, chosen);
    }
    if let Some(at) = hosted_at.filter(|_| hosting) {
        kit.text(
            card,
            format!("Your lobby is open: others join by address at {at}"),
            15.0,
            theme::GOLD,
        );
    }
    if let Some(on) = cheats.filter(|_| hosting) {
        checkbox(kit, card, "Enable cheats", Act::ToggleCheats, on);
    }
    form_actions(kit, card, kind, hosted_at);
    if kind == FormKind::Host
        && let Some(address) = fields.get(1)
    {
        direct_fields(kit, card, address);
    }
    if let Some(host) = plain {
        plain_notice(kit, card, host);
    }
    // The way in's other ways: to another service (the way out without an account is under
    // the card).
    if matches!(kind, FormKind::Login | FormKind::Register) {
        kit.button(
            card,
            "CHANGE SERVICE",
            Act::Go(Page::Service),
            Tone::Secondary,
            28.0,
        );
    }
    card
}

fn form_heading(kit: &mut Kit, card: Entity, kind: FormKind) {
    let (title, hint) = match kind {
        FormKind::Service => (
            "MATCHMAKING SERVICE",
            "The address of the service to play on: wss://host:port",
        ),
        FormKind::Login => ("SIGN IN", ""),
        FormKind::Register => (
            "CREATE ACCOUNT",
            "Logins are 3 to 20 characters of a-z, 0-9 and _. There is no password recovery: keep it safe.",
        ),
        FormKind::Host => (
            "LOCAL LOBBY",
            "A game hosted on this machine, no account needed, or a game server joined by its \
             address. You play under your profile name (SETTINGS, Options).",
        ),
        FormKind::Lobby
        | FormKind::LobbyPassword
        | FormKind::LobbySettings
        | FormKind::LobbyChat
        | FormKind::Replay
        | FormKind::None
        | FormKind::Leaderboard => ("", ""),
    };
    kit.typed(card, title, theme::CARD_TITLE, theme::INK_1);
    if !hint.is_empty() {
        kit.text(card, hint, 14.0, theme::INK_4);
    }
}

fn form_actions(kit: &mut Kit, card: Entity, kind: FormKind, hosted_at: Option<&str>) {
    // The way in's card is narrow: its buttons stand one under the other, as wide as the card.
    let way_in = matches!(
        kind,
        FormKind::Service | FormKind::Login | FormKind::Register
    );
    let row = kit.node(
        card,
        Node {
            flex_direction: if way_in {
                FlexDirection::Column
            } else {
                FlexDirection::Row
            },
            column_gap: px(10),
            row_gap: px(10),
            margin: UiRect::top(px(6)),
            ..default()
        },
    );
    let answer = |kit: &mut Kit, label: &str, act: Act, tone: Tone| {
        let button = kit.button(row, label, act, tone, 44.0);
        if way_in {
            kit.commands
                .entity(button)
                .entry::<Node>()
                .and_modify(|mut node| node.width = percent(100));
        }
    };
    for (label, action, tone) in form_answers(kind, hosted_at.is_some()) {
        answer(kit, label, action, tone);
    }
}

fn form_answers(kind: FormKind, hosted: bool) -> Vec<(&'static str, Act, Tone)> {
    match kind {
        FormKind::Service => vec![("CONTINUE", Act::Submit, Tone::Primary)],
        FormKind::Login => vec![
            ("SIGN IN", Act::Submit, Tone::Primary),
            ("CREATE ACCOUNT", Act::Go(Page::Register), Tone::Secondary),
        ],
        FormKind::Register => vec![
            ("CREATE ACCOUNT", Act::Submit, Tone::Primary),
            ("BACK", Act::Go(Page::Login), Tone::Secondary),
        ],
        FormKind::Host if !hosted => vec![
            ("CREATE", Act::Host(false), Tone::Primary),
            ("CREATE FOR THE LAN", Act::Host(true), Tone::Secondary),
        ],
        _ => Vec::new(),
    }
}

fn direct_fields(kit: &mut Kit, card: Entity, address: &FieldView) {
    kit.typed(card, "JOIN BY ADDRESS", theme::HEADING, theme::INK_1);
    kit.text(
        card,
        "A game server's address: host:port, tcp://, ws:// or wss://.",
        14.0,
        theme::INK_4,
    );
    field_box(kit, card, 1, address);
    let join = kit.node(
        card,
        Node {
            column_gap: px(10),
            ..default()
        },
    );
    kit.button(join, "PLAY", Act::Direct(false), Tone::Primary, 44.0);
    kit.button(join, "WATCH", Act::Direct(true), Tone::Secondary, 44.0);
}

fn plain_notice(kit: &mut Kit, card: Entity, host: &str) {
    let note = kit.node(
        card,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        },
    );
    kit.text(
            note,
            format!(
                "Plain TCP to {host} carries everything unprotected, the seat key included: \
                 fine on your own network, not across the internet. Use wss:// where the host has a certificate."
            ),
            14.0,
            theme::GOLD,
        );
    kit.button(note, "CONNECT ANYWAY", Act::PlainAnyway, Tone::Danger, 34.0);
}

/// A labelled text field; a click puts the keyboard in it.
pub fn field_box(kit: &mut Kit, parent: Entity, at: usize, field: &FieldView) {
    field_inner(kit, parent, at, field, None);
}
pub fn search_field(kit: &mut Kit, parent: Entity, at: usize, field: &FieldView) {
    field_inner(kit, parent, at, field, Some(("menu.search", "Nickname")));
}
fn field_inner(
    kit: &mut Kit,
    parent: Entity,
    at: usize,
    field: &FieldView,
    decoration: Option<(&str, &str)>,
) {
    let column = kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    kit.text(column, field.label, 13.0, theme::INK_5);
    let edge = if field.focused {
        theme::FOCUS
    } else {
        theme::FIELD_EDGE
    };
    let input = kit
        .commands
        .spawn((
            super::kit::MenuButton(Act::Focus(at)),
            Button,
            Node {
                height: px(theme::HIT),
                padding: UiRect::axes(px(10), px(0)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::FIELD_SUNKEN),
            BorderColor::all(edge),
            // A script clicks a field by its label, as it does a button.
            Name::new(field.label.to_lowercase()),
            ChildOf(column),
        ))
        .id();
    if let Some((icon, _)) = decoration {
        kit.art(
            input,
            icon,
            Node {
                width: px(16),
                height: px(16),
                margin: UiRect::right(px(8)),
                ..default()
            },
        );
    }
    let caret = if field.focused { "|" } else { "" };
    if !field.focused
        && field.shown.is_empty()
        && let Some((_, placeholder)) = decoration
    {
        kit.text(input, placeholder, 15.0, theme::INK_5);
    } else {
        kit.text(input, format!("{}{caret}", field.shown), 15.0, theme::INK_1);
    }
}

fn result_page(kit: &mut Kit, body: Entity, result: &ResultView) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(14),
            ..default()
        },
    );
    let color = match result.won {
        Some(true) => theme::RADIANT_TEXT,
        Some(false) => theme::DIRE_TEXT,
        None => theme::INK_1,
    };
    kit.bold(column, result.headline.clone(), 56.0, color);
    kit.text(column, result.detail.clone(), 16.0, theme::INK_4);
    let table = kit.panel(
        column,
        Node {
            width: px(640),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            row_gap: px(8),
            ..default()
        },
    );
    for seat in &result.seats {
        let row = kit.node(
            table,
            Node {
                column_gap: px(18),
                align_items: AlignItems::Center,
                ..default()
            },
        );
        let side = if seat.team == 0 {
            ("RADIANT", Color::srgb(0.4, 0.8, 0.35))
        } else {
            ("DIRE", Color::srgb(0.85, 0.3, 0.25))
        };
        kit.bold(row, side.0, 14.0, side.1);
        let hero = HEROES.iter().position(|h| h.key == seat.hero_key);
        if let Some(hero) = hero {
            kit.portrait(row, hero as u16, 72.0);
        }
        let name = kit.node(
            row,
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        kit.bold(
            name,
            seat.name.clone(),
            17.0,
            if seat.mine { theme::GOLD } else { theme::INK_1 },
        );
        kit.text(
            name,
            kit.texts.name(&seat.hero_key).to_owned(),
            14.0,
            theme::INK_4,
        );
        kit.text(row, seat.rating.clone(), 16.0, theme::INK_1);
    }
    kit.button(column, "CONTINUE", Act::Go(Page::Home), Tone::Primary, 46.0);
}

/// Recordings kept by this client, each available to watch or delete.
fn replays_page(
    kit: &mut Kit,
    body: Entity,
    list: &[ReplayRow],
    _fields: &[FieldView],
    shelf: &ReplayShelf,
) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            width: px(PAGE_WIDTH),
            ..default()
        },
    );
    kit.bold(column, "REPLAYS", 30.0, theme::INK_1);
    let (note, colour) = if !shelf.store {
        (
            "This browser keeps no replays for the page.".to_owned(),
            theme::GOLD,
        )
    } else if !shelf.keeping {
        (
            "Saving replays is off (SETTINGS, Options, REPLAYS): the ones below were kept before."
                .to_owned(),
            theme::GOLD,
        )
    } else if shelf.folder.is_empty() {
        (
            "The replays of your matches are kept in this browser, within the limits of SETTINGS, Options, REPLAYS.".to_owned(),
            theme::INK_4,
        )
    } else {
        (
            format!(
                "The replays of your matches are kept in {}, within the limits of SETTINGS, Options, REPLAYS.",
                shelf.folder
            ),
            theme::INK_4,
        )
    };
    kit.text(column, note, 13.0, colour);
    if shelf.service_keeps_none {
        kit.text(
            column,
            "The matchmaking service keeps no replays of its matches, so none of them is saved here; local games still are.",
            13.0,
            theme::GOLD,
        );
    }
    if list.is_empty() {
        kit.text(column, "No replays yet", 16.0, theme::INK_4);
    }
    for replay in list {
        replay_row(kit, column, replay);
    }
}

/// What a replay's row says of where it was played.
fn mode_label(mode: &str) -> String {
    match mode {
        "local" => "Local lobby".to_owned(),
        "demo" => "Demo Hero".to_owned(),
        "file" => "Replay file".to_owned(),
        ruleset => format!("Online match · {ruleset}"),
    }
}

fn replay_row(kit: &mut Kit, column: Entity, replay: &ReplayRow) {
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
    use super::replay_info::Summary;
    let known = |sides: &str| {
        if sides.is_empty() {
            mode_label(&replay.mode)
        } else {
            sides.to_owned()
        }
    };
    let (headline, line, colour) = match &replay.summary {
        Summary::Read { sides, outcome } if outcome.is_empty() => (
            known(&sides.text(kit.texts)),
            replay.when.clone(),
            theme::INK_4,
        ),
        Summary::Read { sides, outcome } => (
            known(&sides.text(kit.texts)),
            format!("{}  ·  {outcome}", replay.when),
            theme::INK_4,
        ),
        Summary::Reading => (
            mode_label(&replay.mode),
            format!("{}  ·  reading…", replay.when),
            theme::INK_4,
        ),
        Summary::Broken(why) => (
            mode_label(&replay.mode),
            format!("{}  ·  cannot be watched: {why}", replay.when),
            theme::ERROR_TEXT,
        ),
    };
    kit.bold(label, headline, 16.0, theme::INK_1);
    kit.text(label, line, 13.0, colour);
    kit.text(
        label,
        format!("{}  ·  {}", replay.name, replay.size),
        12.0,
        theme::INK_4,
    );
    let name = &replay.name;
    if !matches!(replay.summary, Summary::Broken(_)) {
        let watch = kit.button(
            row,
            "WATCH",
            Act::OpenReplay(name.clone()),
            Tone::Secondary,
            34.0,
        );
        kit.commands
            .entity(watch)
            .insert(Name::new(format!("watch {name}")));
    }
    let delete = kit.button(
        row,
        "DELETE",
        Act::DeleteReplay(name.clone()),
        Tone::Danger,
        34.0,
    );
    kit.commands
        .entity(delete)
        .insert(Name::new(format!("delete {name}")));
}

/// The rules of the lobby about to be hosted. The ten seats are the same under either: it
/// starts with the players seated, and nothing takes the empty ones.
fn local_rules(kit: &mut Kit, card: Entity, chosen: LocalRules) {
    kit.text(card, "Rules", 13.0, theme::INK_4);
    let row = kit.node(
        card,
        Node {
            column_gap: px(8),
            ..default()
        },
    );
    for rules in LocalRules::ALL {
        let look = if rules == chosen {
            Tone::Primary
        } else {
            Tone::Secondary
        };
        kit.button(row, rules.label(), Act::Rules(rules), look, 34.0);
    }
    kit.text(
        card,
        "Ten seats, five a side. Others join by this machine's address, typed under JOIN BY \
         ADDRESS; the match starts with the players ready and nobody fills the empty seats.",
        13.0,
        theme::INK_4,
    );
}

fn checkbox(kit: &mut Kit, card: Entity, label: &'static str, action: Act, on: bool) {
    let row = kit
        .commands
        .spawn((
            super::kit::MenuButton(action),
            Button,
            Name::new(label.to_ascii_lowercase()),
            Node {
                column_gap: px(10),
                align_items: AlignItems::Center,
                margin: UiRect::top(px(4)),
                ..default()
            },
            ChildOf(card),
        ))
        .id();
    crate::ui::checkbox::draw(
        &mut kit.visual(),
        row,
        18.0,
        3.0,
        crate::ui::checkbox::State {
            checked: on,
            enabled: true,
        },
    );
    kit.text(row, label, 16.0, theme::INK_1);
}

/// DEMO HERO: every hero, its portrait and its name; a click starts Dota's
/// Demo Hero with it, a game of the service alone on the map with every cheat.
#[cfg(not(target_arch = "wasm32"))]
fn demo_page(kit: &mut Kit, body: Entity, heroes: &[(u16, String)], offered: bool) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(14),
            ..default()
        },
    );
    kit.bold(column, "DEMO HERO", 30.0, theme::INK_1);
    kit.text(
        column,
        "Try a hero alone with every cheat: levels, gold, items, free spells, enemies to fight.",
        16.0,
        theme::INK_4,
    );
    if !offered {
        kit.text(
            column,
            "This service offers no Demo Hero.",
            16.0,
            theme::ERROR_TEXT,
        );
        return;
    }
    let grid = kit.node(
        column,
        Node {
            column_gap: px(16),
            ..default()
        },
    );
    for (id, key) in heroes {
        let name = kit.texts.name(key).to_owned();
        let card = kit
            .commands
            .spawn((
                super::kit::MenuButton(Act::Demo(*id)),
                Button,
                Name::new(format!("demo {}", name.to_lowercase())),
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(6),
                    padding: UiRect::all(px(8)),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                theme::downward(theme::PANEL),
                BorderColor::all(theme::PANEL_EDGE),
                ChildOf(grid),
            ))
            .id();
        kit.portrait(card, *id, 240.0);
        kit.bold(card, name.to_uppercase(), 18.0, theme::INK_1);
    }
}

pub(super) fn corner_height(view: &MenuView) -> f32 {
    let secondary = if view.game.is_some() && view.can_leave_game {
        2
    } else if view.game.is_some() || view.reconnect {
        1
    } else {
        0
    };
    theme::ACTION_HEIGHT + secondary as f32 * (theme::HIT + theme::CORNER_GAP)
}
