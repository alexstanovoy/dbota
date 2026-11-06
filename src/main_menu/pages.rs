//! The pages: the top bar over the menu's background, the page's column under it, the player's
//! card at the bottom left and the game's corner at the bottom right; the way in (the service,
//! signing in, registering) on the background alone, a card in the middle.

use bevy::prelude::*;
use bota_core::HEROES;

use super::backdrop::{self, Backdrop};
use super::core::{Act, LocalRules, Page};
use super::form::FormKind;
use super::kit::{Kit, MenuButton, Tone};
use super::view::{
    FieldView, LobbyView, MemberRow, MenuView, PageView, ReplayRow, ReplayShelf, ResultView,
    RulesetRow, SlotRow,
};
use crate::theme::{self, Face, Type};
use bota_radovid::proto::{AccountGameLimit, LOBBY_NO_SLOT};

/// The pages' column: the forms and the replay list, one width.
const PAGE_WIDTH: f32 = 760.0;
/// Where every page starts under the top bar.
const PAGE_TOP: f32 = 64.0;
/// Room left under a page for the player's card and the game's corner.
const PAGE_FOOT: f32 = 96.0;
/// The way in's card, and where its column starts.
const WAY_IN_WIDTH: f32 = 480.0;
const WAY_IN_TOP: f32 = 110.0;
/// The column of the game's buttons at the bottom right.
const CORNER_WIDTH: f32 = 300.0;

/// The scrolled part of a page, under the top bar.
#[derive(Component)]
pub struct MenuBody;

/// The whole page layer.
pub fn draw(kit: &mut Kit, root: Entity, view: &MenuView) {
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
        return;
    }
    backdrop::draw(kit, root, Backdrop::of(view.side), theme::BAR_HEIGHT);
    super::top_bar::draw(kit, root, view);
    // Every page starts at the same height under the top bar and centres
    // on one column, as Dota's dashboard pages do; a long page scrolls.
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
        PageView::Home {
            rulesets,
            allow_matchmaking_bots,
            in_flight,
            cooldown,
        } => {
            home(
                kit,
                body,
                rulesets,
                *in_flight,
                *allow_matchmaking_bots,
                cooldown.as_deref(),
            );
        }
        PageView::MyGames {
            games,
            finished,
            cooldown,
        } => super::overlay::my_games(kit, body, view, games, finished, cooldown.as_deref()),
        PageView::Profile {
            name,
            ratings,
            ladder,
        } => profile_page(kit, body, name, ratings, ladder),
        PageView::Lobbies { list, fields } => super::lobbies::list(kit, body, list, fields),
        PageView::Lobby(lobby) => lobby_page(kit, body, lobby),
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
        PageView::Settings | PageView::Hidden => {}
    }
    player_card(kit, root, view);
    game_corner(kit, root, view);
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
                bottom: px(18),
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
    if view.account.is_some() {
        kit.commands
            .entity(card)
            .insert((MenuButton(Act::Go(Page::Profile)), Button));
    }
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
    let large = Type {
        face: Face::Bold,
        size: 20.0,
        tracking: 0.0,
        figures: false,
    };
    kit.typed(avatar, initial, large, theme::INK_1);
    let name = Type {
        face: Face::Semibold,
        size: 17.0,
        tracking: 0.0,
        figures: false,
    };
    kit.typed(card, view.player.clone(), name, theme::INK_1);
}

/// The corner where Dota's play button stands: over a game, LEAVE GAME and DISCONNECT above
/// RETURN TO GAME; after leaving one's seat, LEAVE above RECONNECT.
fn game_corner(kit: &mut Kit, root: Entity, view: &MenuView) {
    if view.game.is_none() && !view.reconnect {
        return;
    }
    let corner = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(48),
            bottom: px(18),
            width: px(CORNER_WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        },
    );
    let wide = |kit: &mut Kit, label: &str, act: Act, tone: Tone, height: f32, name: &str| {
        let button = kit.button(corner, label, act, tone, height);
        kit.commands.entity(button).insert(Name::new(name.to_owned()));
        kit.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(|mut node| node.width = percent(100));
    };
    if view.game.is_some() {
        // A replay is left as a game watched is (Dota's popup_confirm_close_replay).
        if view.can_leave_game {
            wide(kit, "LEAVE GAME", Act::LeaveGame, Tone::Danger, 44.0, "leave game");
        }
        wide(kit, "DISCONNECT", Act::Disconnect, Tone::Danger, 44.0, "disconnect");
        wide(kit, "RETURN TO GAME", Act::ReturnToGame, Tone::Primary, 52.0, "return");
    } else {
        // Dota's LEAVE: a way to be done with a game that cannot be taken again (its server died).
        wide(kit, "LEAVE", Act::LeaveSeat, Tone::Danger, 44.0, "leave match");
        wide(kit, "RECONNECT", Act::Reconnect, Tone::Primary, 52.0, "reconnect");
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
    let wordmark = Type {
        size: 56.0,
        tracking: 12.0,
        ..theme::WORDMARK
    };
    kit.typed(mark, "DBOTA", wordmark, theme::INK_1);
    let card = form_page(kit, column, kind, fields, busy, plain, None, None, None);
    kit.commands
        .entity(card)
        .entry::<Node>()
        .and_modify(|mut node| node.width = px(WAY_IN_WIDTH));
}

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
        | FormKind::Room
        | FormKind::Replay
        | FormKind::None => ("", ""),
    };
    let title_role = Type {
        size: 24.0,
        tracking: 2.0,
        ..theme::TITLE
    };
    kit.typed(card, title, title_role, theme::INK_1);
    if !hint.is_empty() {
        kit.text(card, hint, 14.0, theme::INK_4);
    }
    let hosting = kind == FormKind::Host;
    // The local lobby's address is below the lobby it hosts.
    let shown = match kind {
        FormKind::Host if hosting => 1,
        FormKind::Host => 0,
        _ => fields.len(),
    };
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
    match kind {
        FormKind::Service => answer(kit, "CONTINUE", Act::Submit, Tone::Primary),
        FormKind::Login => {
            answer(kit, "SIGN IN", Act::Submit, Tone::Primary);
            answer(kit, "CREATE ACCOUNT", Act::Go(Page::Register), Tone::Secondary);
        }
        FormKind::Register => {
            answer(kit, "CREATE ACCOUNT", Act::Submit, Tone::Primary);
            answer(kit, "BACK", Act::Go(Page::Login), Tone::Secondary);
        }
        // One lobby at a time: while it is up, the card says where it is.
        FormKind::Host if hosted_at.is_some() || !hosting => {}
        FormKind::Host => {
            answer(kit, "CREATE", Act::Host(false), Tone::Primary);
            answer(kit, "CREATE FOR THE LAN", Act::Host(true), Tone::Secondary);
        }
        FormKind::Lobby
        | FormKind::LobbyPassword
        | FormKind::Room
        | FormKind::Replay
        | FormKind::None => {}
    }
    if let Some(address) = fields.get(1).filter(|_| kind == FormKind::Host) {
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
    if let Some(host) = plain {
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
    if way_in {
        let links = kit.node(
            card,
            Node {
                column_gap: px(10),
                ..default()
            },
        );
        if kind != FormKind::Service {
            kit.button(links, "CHANGE SERVICE", Act::Go(Page::Service), Tone::Secondary, 28.0);
        }
        #[cfg(not(target_arch = "wasm32"))]
        kit.button(links, "LOCAL LOBBY", Act::Go(Page::LocalLobby), Tone::Secondary, 28.0);
    }
    card
}

/// A labelled text field; a click puts the keyboard in it.
pub fn field_box(kit: &mut Kit, parent: Entity, at: usize, field: &FieldView) {
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
    let caret = if field.focused { "|" } else { "" };
    kit.text(input, format!("{}{caret}", field.shown), 15.0, theme::INK_1);
}

/// Play: every ruleset this client can play, with FIND MATCH.
fn home(
    kit: &mut Kit,
    body: Entity,
    rulesets: &[RulesetRow],
    in_flight: (usize, Option<AccountGameLimit>),
    allow_matchmaking_bots: bool,
    cooldown: Option<&str>,
) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(16),
            ..default()
        },
    );
    kit.bold(column, "PLAY", 30.0, theme::INK_1);
    checkbox(
        kit,
        column,
        "Allow matchmaking with bots",
        Act::ToggleMatchmakingBots,
        allow_matchmaking_bots,
    );
    if rulesets.is_empty() {
        kit.text(
            column,
            "The service offers no game this version of dbota can play.",
            16.0,
            theme::INK_4,
        );
    }
    for ruleset in rulesets {
        let card = kit.panel(
            column,
            Node {
                width: px(900),
                padding: UiRect::all(px(18)),
                column_gap: px(18),
                align_items: AlignItems::Center,
                ..default()
            },
        );
        let heroes = kit.node(
            card,
            Node {
                column_gap: px(6),
                ..default()
            },
        );
        for hero in 0..HEROES.len() as u16 {
            kit.portrait(heroes, hero, 96.0);
        }
        let about = kit.node(
            card,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                flex_grow: 1.0,
                ..default()
            },
        );
        kit.bold(about, ruleset.name.clone(), 22.0, theme::INK_1);
        let rated = if ruleset.rated { "ranked" } else { "unranked" };
        kit.text(
            about,
            format!("{} · {rated} · {}", ruleset.teams, ruleset.audience),
            15.0,
            theme::INK_4,
        );
        let can = cooldown.is_none()
            && match in_flight.1 {
                Some(AccountGameLimit::Unlimited) => true,
                Some(AccountGameLimit::Limited(maximum)) => in_flight.0 < usize::from(maximum),
                None => false,
            };
        let look = if can { Tone::Primary } else { Tone::Secondary };
        kit.button(
            card,
            "FIND MATCH",
            Act::Find(ruleset.id.clone()),
            look,
            52.0,
        );
    }
    let count = match in_flight.1 {
        Some(AccountGameLimit::Limited(maximum)) => {
            Some(format!("Games going: {} of {maximum} at once", in_flight.0))
        }
        Some(AccountGameLimit::Unlimited) => {
            Some(format!("Games going: {} · no account limit", in_flight.0))
        }
        None => None,
    };
    if let Some(count) = count {
        kit.text(column, count, 14.0, theme::INK_4);
    }
    if let Some(cooldown) = cooldown {
        kit.text(column, cooldown, 15.0, theme::ERROR_TEXT);
    }
}

fn profile_page(kit: &mut Kit, body: Entity, name: &str, ratings: &[String], ladder: &[String]) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            width: px(700),
            ..default()
        },
    );
    kit.bold(column, "PROFILE", 30.0, theme::INK_1);
    kit.bold(column, name.to_owned(), 22.0, theme::INK_1);
    if ratings.is_empty() {
        kit.text(column, "No rated games yet.", 16.0, theme::INK_4);
    }
    for line in ratings {
        kit.text(column, line.clone(), 16.0, theme::INK_1);
    }
    if !ladder.is_empty() {
        kit.bold(column, "LADDER", 20.0, theme::INK_1);
    }
    for line in ladder {
        kit.text(column, line.clone(), 15.0, theme::INK_4);
    }
}

/// The lobby one sits in: the slot grid by team (a free slot is a click away, the owner moves
/// and removes the others), those looking on, and the settings at the side, the owner's to change.
fn lobby_page(kit: &mut Kit, body: Entity, lobby: &LobbyView) {
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
            width: px(700),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24)),
            row_gap: px(10),
            ..default()
        },
    );
    let lock = if lobby.locked { " · password" } else { "" };
    kit.bold(card, format!("LOBBY: {}{lock}", lobby.name), 24.0, theme::INK_1);
    if lobby.settings.cheats == Some(true) {
        // Everyone is told before readying: the match will be played with cheats.
        kit.bold(
            card,
            "CHEATS ON: this match is played with the game's cheats",
            16.0,
            theme::GOLD,
        );
    }
    if let Some(name) = &lobby.moving {
        kit.text(
            card,
            format!("Moving {name}: click a free slot, or LOOK ON"),
            14.0,
            theme::GOLD,
        );
    }
    for team in &lobby.teams {
        kit.bold(card, team.name.clone(), 16.0, theme::INK_4);
        for slot in &team.slots {
            slot_row(kit, card, slot, lobby);
        }
    }
    kit.bold(card, "LOOKING ON", 16.0, theme::INK_4);
    for member in &lobby.spectators {
        member_row(kit, card, member, lobby);
    }
    kit.button(card, "LOOK ON", Act::LobbySlot(LOBBY_NO_SLOT), Tone::Secondary, 28.0);
    if let Some(secs) = lobby.starting_in_secs {
        kit.bold(card, format!("Starting in {secs}"), 22.0, theme::GOLD);
    }
    let row = kit.node(
        card,
        Node {
            column_gap: px(10),
            ..default()
        },
    );
    if lobby.starts_short {
        // Below its seats: the empty ones stay empty.
        kit.button(row, "START NOW", Act::ReadyLobby(true), Tone::Primary, 42.0);
        kit.text(
            card,
            "The empty seats stay empty: nobody plays against you.",
            14.0,
            theme::INK_4,
        );
    } else if lobby.me_ready {
        kit.button(row, "NOT READY", Act::ReadyLobby(false), Tone::Secondary, 42.0);
    } else if let Some(why) = lobby.waiting {
        kit.disabled_button(row, "READY", 42.0);
        kit.text(card, why, 14.0, theme::INK_4);
    } else {
        kit.button(row, "READY", Act::ReadyLobby(true), Tone::Primary, 42.0);
    }
    kit.button(row, "LEAVE", Act::LeaveLobby, Tone::Danger, 42.0);
    settings_card(kit, page, lobby);
}

/// One slot of the grid: its holder, or a click to sit there.
fn slot_row(kit: &mut Kit, card: Entity, slot: &SlotRow, lobby: &LobbyView) {
    let row = kit.node(
        card,
        Node {
            column_gap: px(10),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    match &slot.who {
        None => {
            kit.button(row, "OPEN", Act::LobbySlot(slot.slot), Tone::Secondary, 28.0);
        }
        Some(member) => member_row(kit, row, member, lobby),
    }
}

/// A member's name and state, with the owner's MOVE and KICK for the others.
fn member_row(kit: &mut Kit, parent: Entity, member: &MemberRow, lobby: &LobbyView) {
    let row = kit.node(
        parent,
        Node {
            column_gap: px(10),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let name = if member.me {
        format!("{} (you)", member.name)
    } else {
        member.name.clone()
    };
    kit.text(row, format!("{name:24}"), 16.0, theme::INK_1);
    if member.ready {
        kit.text(row, "READY", 15.0, theme::RADIANT_TEXT);
    }
    if lobby.owner && !member.me {
        kit.button(row, "MOVE", Act::MoveMember(member.account), Tone::Secondary, 24.0);
        kit.button(row, "KICK", Act::KickMember(member.account), Tone::Danger, 24.0);
    }
}

/// The lobby's settings: read by everyone, changed by the owner.
fn settings_card(kit: &mut Kit, page: Entity, lobby: &LobbyView) {
    let card = kit.panel(
        page,
        Node {
            width: px(360),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(20)),
            row_gap: px(10),
            ..default()
        },
    );
    let settings = &lobby.settings;
    kit.bold(card, "SETTINGS", 20.0, theme::INK_1);
    kit.text(card, format!("Game mode: {}", settings.mode), 15.0, theme::INK_1);
    kit.text(
        card,
        format!("Open to: {}", settings.audience),
        15.0,
        theme::INK_1,
    );
    kit.text(
        card,
        if settings.locked {
            "Password: set"
        } else {
            "Password: none"
        },
        15.0,
        theme::INK_1,
    );
    if let Some(on) = settings.cheats {
        let (said, ink) = if on {
            ("Cheats: on", theme::GOLD)
        } else {
            ("Cheats: off", theme::INK_1)
        };
        kit.text(card, said, 15.0, ink);
    }
    if !settings.editable {
        kit.text(card, "Only the owner changes these.", 13.0, theme::INK_4);
        return;
    }
    if settings.modes.len() > 1 {
        kit.text(card, "Game mode", 13.0, theme::INK_4);
        for mode in &settings.modes {
            let look = if mode.chosen { Tone::Primary } else { Tone::Secondary };
            kit.button(
                card,
                &mode.name,
                Act::ConfigureMode(mode.id.clone()),
                look,
                30.0,
            );
        }
    }
    match settings.cheats {
        Some(false) => {
            kit.button(
                card,
                "ENABLE CHEATS",
                Act::ConfigureCheats(true),
                Tone::Secondary,
                30.0,
            );
        }
        Some(true) => {
            kit.button(
                card,
                "DISABLE CHEATS",
                Act::ConfigureCheats(false),
                Tone::Secondary,
                30.0,
            );
        }
        None => {}
    }
    kit.text(card, "Open to", 13.0, theme::INK_4);
    for audience in &settings.audiences {
        let look = if audience.chosen { Tone::Primary } else { Tone::Secondary };
        kit.button(
            card,
            audience.name,
            Act::ConfigureAudience(audience.audience),
            look,
            30.0,
        );
    }
    for (at, field) in lobby.fields.iter().enumerate() {
        field_box(kit, card, at, field);
    }
    kit.button(card, "SET PASSWORD", Act::ConfigurePassword, Tone::Secondary, 30.0);
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
        let hero = HEROES.iter().position(|h| h.name == seat.hero);
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
        kit.text(name, seat.hero.clone(), 14.0, theme::INK_4);
        kit.text(row, seat.rating.clone(), 16.0, theme::INK_1);
    }
    kit.button(column, "CONTINUE", Act::Go(Page::Home), Tone::Primary, 46.0);
}

/// REPLAYS: the replays this client keeps, the newest first, each to watch or delete (and in a
/// browser to download); and any replay file, by its path on a desktop, from the file chooser in
/// a browser.
fn replays_page(
    kit: &mut Kit,
    body: Entity,
    list: &[ReplayRow],
    fields: &[FieldView],
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
            "This browser keeps no replays for the page. A replay file can still be opened below."
                .to_owned(),
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
    let card = kit.panel(
        column,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            row_gap: px(10),
            margin: UiRect::top(px(10)),
            ..default()
        },
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        for (at, field) in fields.iter().enumerate() {
            field_box(kit, card, at, field);
        }
        kit.button(card, "OPEN", Act::Submit, Tone::Primary, 40.0);
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = fields;
        let open = kit.button(card, "OPEN A .BRP FILE", Act::OpenReplayFile, Tone::Primary, 40.0);
        kit.commands
            .entity(open)
            .insert(Name::new("open replay file"));
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
        Summary::Read { sides, outcome } if outcome.is_empty() => {
            (known(sides), replay.when.clone(), theme::INK_4)
        }
        Summary::Read { sides, outcome } => (
            known(sides),
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
        let watch = kit.button(row, "WATCH", Act::OpenReplay(name.clone()), Tone::Secondary, 34.0);
        kit.commands
            .entity(watch)
            .insert(Name::new(format!("watch {name}")));
    }
    #[cfg(target_arch = "wasm32")]
    {
        let download = kit.button(
            row,
            "DOWNLOAD .BRP",
            Act::DownloadReplay(name.clone()),
            Tone::Secondary,
            34.0,
        );
        kit.commands
            .entity(download)
            .insert(Name::new(format!("download {name}")));
    }
    let delete = kit.button(row, "DELETE", Act::DeleteReplay(name.clone()), Tone::Danger, 34.0);
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
        let look = if rules == chosen { Tone::Primary } else { Tone::Secondary };
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
    kit.commands.spawn((
        Node {
            width: px(18),
            height: px(18),
            border: UiRect::all(px(2)),
            border_radius: BorderRadius::all(px(3)),
            ..default()
        },
        BorderColor::all(if on { theme::GOLD } else { theme::INK_4 }),
        BackgroundColor(if on { theme::GOLD } else { Color::NONE }),
        Pickable::IGNORE,
        ChildOf(row),
    ));
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
        kit.text(column, "This service offers no Demo Hero.", 16.0, theme::ERROR_TEXT);
        return;
    }
    let grid = kit.node(
        column,
        Node {
            column_gap: px(16),
            ..default()
        },
    );
    for (id, name) in heroes {
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
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(theme::SURFACE),
                BorderColor::all(theme::PANEL_EDGE),
                ChildOf(grid),
            ))
            .id();
        kit.portrait(card, *id, 240.0);
        kit.bold(card, name.to_uppercase(), 18.0, theme::INK_1);
    }
}
