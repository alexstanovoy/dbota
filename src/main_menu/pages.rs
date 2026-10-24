//! The pages under the top bar.

use bevy::prelude::*;
use bota_core::HEROES;

use super::core::{Act, Page, SeatFill};
use super::form::FormKind;
use super::screens::{ERROR, GREEN, Kit, PLAIN, RED, tab_color, tab_of};
use super::view::{FieldView, LobbyView, MenuView, PageView, ReplayRow, ResultView, RulesetRow};
use crate::ui;

/// The tabs of the top bar: the service's once logged in, and what is played
/// without it always.
const TABS: [(Page, &str); 8] = [
    (Page::Home, "PLAY"),
    (Page::MyGames, "MY GAMES"),
    (Page::Lobbies, "LOBBIES"),
    (Page::Profile, "PROFILE"),
    (Page::LocalLobby, "LOCAL LOBBY"),
    (Page::DemoHero, "DEMO HERO"),
    (Page::Direct, "DIRECT CONNECT"),
    (Page::Replays, "REPLAYS"),
];

/// Whether a tab's page is the service's, shown only with an account.
fn online(page: Page) -> bool {
    matches!(
        page,
        Page::Home | Page::MyGames | Page::Lobbies | Page::Profile
    )
}

/// Dota's quit button (`#DOTAQuitButton`, a `TopBarSmallButton`): 62 by 61
/// with its 30-pixel icon washed orange, brighter under the cursor.
const QUIT_WASH: (Color, Color) = (
    Color::srgb(
        0xbd as f32 / 255.0,
        0x5e as f32 / 255.0,
        0x2d as f32 / 255.0,
    ),
    Color::srgb(1.0, 0x6e as f32 / 255.0, 0x41 as f32 / 255.0),
);
/// Dota's return-to-game button (`#DOTAReturnToGameButton`): 62 by 57, its
/// 30-pixel icon washed grey-blue.
const RETURN_WASH: (Color, Color) = (
    Color::srgb(
        0x44 as f32 / 255.0,
        0x4a as f32 / 255.0,
        0x55 as f32 / 255.0,
    ),
    Color::srgb(
        0x99 as f32 / 255.0,
        0x99 as f32 / 255.0,
        0xaa as f32 / 255.0,
    ),
);

/// The pages' column: the forms and the replay list, one width.
const PAGE_WIDTH: f32 = 600.0;
/// Where every page starts under the top bar.
const PAGE_TOP: f32 = 64.0;

/// The scrolled part of a page, under the top bar.
#[derive(Component)]
pub struct MenuBody;

/// The whole page layer.
pub fn draw(kit: &mut Kit, root: Entity, view: &MenuView) {
    top_bar(kit, root, view);
    play_area(kit, root, view);
    if let Some(status) = &view.status {
        let line = kit.node(
            root,
            Node {
                padding: UiRect::axes(px(28), px(8)),
                column_gap: px(14),
                align_items: AlignItems::Center,
                ..default()
            },
        );
        let color = if status.error { ERROR } else { ui::GOLDEN };
        kit.text(line, status.text.clone(), 15.0, color);
        kit.button(line, "×", Act::Dismiss, PLAIN, 22.0);
    }
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
                top: px(PAGE_TOP),
                bottom: px(24),
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
            fill,
            hosted_at,
        } => {
            let card = form_page(
                kit,
                body,
                *kind,
                fields,
                *busy,
                plain_host.as_deref(),
                *fill,
                hosted_at.as_deref(),
            );
            if *kind == FormKind::Host {
                cheats_toggle(kit, card, view.host_cheats);
            }
        }
        PageView::Home {
            rulesets,
            in_flight,
            cooldown,
        } => {
            home(kit, body, rulesets, *in_flight, cooldown.as_deref());
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
        PageView::Replays { list, fields } => replays_page(kit, body, list, fields),
        PageView::DemoHero { heroes } => demo_page(kit, body, heroes),
        PageView::Joining => {
            kit.bold(body, "JOINING THE GAME…", 30.0, ui::INK);
        }
        PageView::Settings | PageView::Hidden => {}
    }
}

fn top_bar(kit: &mut Kit, root: Entity, view: &MenuView) {
    let bar = kit
        .commands
        .spawn((
            Node {
                height: px(60),
                padding: UiRect::axes(px(28), px(0)),
                column_gap: px(30),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.55)),
            BorderColor::all(ui::EDGE),
            ChildOf(root),
        ))
        .id();
    // In a game, Dota's top bar starts with the way back to it.
    if view.game.is_some() {
        kit.icon_button(
            bar,
            "menu.return_to_game",
            "RETURN TO GAME",
            Act::ReturnToGame,
            (62.0, 57.0),
            30.0,
            RETURN_WASH,
        );
    }
    kit.bold(bar, "DBOTA", 28.0, Color::srgb(0.85, 0.2, 0.15));
    let tabs = kit.node(
        bar,
        Node {
            column_gap: px(6),
            flex_grow: 1.0,
            ..default()
        },
    );
    let lit = tab_of(view.page);
    for (page, label) in TABS {
        if online(page) && view.account.is_none() {
            continue;
        }
        let on = lit == page;
        let tab = kit
            .commands
            .spawn((
                super::screens::MenuButton(Act::Go(page)),
                Button,
                Node {
                    padding: UiRect::axes(px(14), px(18)),
                    border: UiRect::bottom(px(3)),
                    ..default()
                },
                BorderColor::all(if on { ui::GOLDEN } else { Color::NONE }),
                Name::new(label.to_lowercase()),
                ChildOf(tabs),
            ))
            .id();
        kit.bold(tab, label, 17.0, tab_color(on));
    }
    kit.button(bar, "SETTINGS", Act::Go(Page::Settings), PLAIN, 30.0);
    match &view.account {
        Some(account) => {
            kit.text(bar, account.clone(), 16.0, ui::INK);
            kit.button(bar, "LOG OUT", Act::Logout, PLAIN, 30.0);
        }
        None if view.page == Page::Direct => {
            kit.button(bar, "SIGN IN", Act::Go(Page::Login), PLAIN, 30.0);
        }
        None => {}
    }
    // Dota's power button ends the top bar.
    if view.can_quit {
        let quit = kit.icon_button(
            bar,
            "menu.quit",
            "QUIT",
            Act::Quit,
            (62.0, 61.0),
            30.0,
            QUIT_WASH,
        );
        kit.commands
            .entity(quit)
            .insert((crate::voice::QuitClick, Name::new("quit")));
    }
}

/// The corner where Dota's play button stands: over a game, RETURN TO GAME
/// and the way out of it (DISCONNECT); after leaving one's seat, RECONNECT.
fn play_area(kit: &mut Kit, root: Entity, view: &MenuView) {
    if view.game.is_none() && !view.reconnect {
        return;
    }
    let corner = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(24),
            bottom: px(24),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            row_gap: px(8),
            width: px(340),
            ..default()
        },
    );
    if view.game.is_some() {
        let back = kit.button(corner, "RETURN TO GAME", Act::ReturnToGame, GREEN, 64.0);
        kit.commands.entity(back).insert(Name::new("return"));
        // A replay is left as a game watched is (Dota's popup_confirm_close_replay).
        if view.can_leave_game {
            let leave_game = kit.button(corner, "LEAVE GAME", Act::LeaveGame, RED, 40.0);
            kit.commands
                .entity(leave_game)
                .insert(Name::new("leave game"));
        }
        let leave = kit.button(corner, "DISCONNECT", Act::Disconnect, RED, 40.0);
        kit.commands.entity(leave).insert(Name::new("disconnect"));
    } else {
        let again = kit.button(corner, "RECONNECT", Act::Reconnect, GREEN, 64.0);
        kit.commands.entity(again).insert(Name::new("reconnect"));
    }
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
    fill: Option<(SeatFill, bool)>,
    hosted_at: Option<&str>,
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
        FormKind::Direct => (
            "DIRECT CONNECT",
            "A game server's address: host:port, tcp://, ws:// or wss://. You play under your \
             profile name (SETTINGS, Options).",
        ),
        FormKind::Host => (
            "LOCAL LOBBY",
            "A game hosted on this machine, no account needed: the others join by its address \
             under DIRECT CONNECT. You play under your profile name (SETTINGS, Options).",
        ),
        FormKind::Lobby | FormKind::Replay | FormKind::None => ("", ""),
    };
    kit.bold(card, title, 26.0, ui::INK);
    if !hint.is_empty() {
        kit.text(card, hint, 14.0, ui::INK_DIM);
    }
    for (at, field) in fields.iter().enumerate() {
        field_box(kit, card, at, field);
    }
    if busy {
        kit.text(card, "Connecting to the service…", 15.0, ui::GOLDEN);
    }
    if let Some((chosen, bot_set)) = fill {
        seat_fill(kit, card, chosen, bot_set);
    }
    if let Some(at) = hosted_at {
        kit.text(
            card,
            format!("Your lobby is open: others join under DIRECT CONNECT at {at}"),
            15.0,
            ui::GOLDEN,
        );
    }
    let row = kit.node(
        card,
        Node {
            column_gap: px(10),
            margin: UiRect::top(px(6)),
            ..default()
        },
    );
    match kind {
        FormKind::Service => {
            kit.button(row, "CONTINUE", Act::Submit, GREEN, 44.0);
        }
        FormKind::Login => {
            kit.button(row, "SIGN IN", Act::Submit, GREEN, 44.0);
            kit.button(row, "CREATE ACCOUNT", Act::Go(Page::Register), PLAIN, 44.0);
        }
        FormKind::Register => {
            kit.button(row, "CREATE ACCOUNT", Act::Submit, GREEN, 44.0);
            kit.button(row, "BACK", Act::Go(Page::Login), PLAIN, 44.0);
        }
        FormKind::Direct => {
            kit.button(row, "PLAY", Act::Direct(false), GREEN, 44.0);
            kit.button(row, "WATCH", Act::Direct(true), PLAIN, 44.0);
        }
        // One lobby at a time: while it is up, the card says where it is.
        FormKind::Host if hosted_at.is_some() => {}
        FormKind::Host => {
            kit.button(row, "CREATE", Act::Host(false), GREEN, 44.0);
            kit.button(row, "CREATE FOR THE LAN", Act::Host(true), PLAIN, 44.0);
        }
        FormKind::Lobby | FormKind::Replay | FormKind::None => {}
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
            ui::GOLDEN,
        );
        kit.button(note, "CONNECT ANYWAY", Act::PlainAnyway, RED, 34.0);
    }
    if matches!(
        kind,
        FormKind::Service | FormKind::Login | FormKind::Register
    ) {
        let links = kit.node(
            card,
            Node {
                column_gap: px(10),
                ..default()
            },
        );
        if kind != FormKind::Service {
            kit.button(links, "CHANGE SERVICE", Act::Go(Page::Service), PLAIN, 28.0);
        }
        kit.button(links, "DIRECT CONNECT", Act::Go(Page::Direct), PLAIN, 28.0);
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
    kit.text(column, field.label, 13.0, ui::INK_DIM);
    let edge = if field.focused { ui::GOLDEN } else { ui::EDGE };
    let input = kit
        .commands
        .spawn((
            super::screens::MenuButton(Act::Focus(at)),
            Button,
            Node {
                height: px(38),
                padding: UiRect::axes(px(10), px(0)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
            BorderColor::all(edge),
            ChildOf(column),
        ))
        .id();
    let caret = if field.focused { "|" } else { "" };
    kit.text(input, format!("{}{caret}", field.shown), 17.0, ui::INK);
}

/// Play: every ruleset this client can play, with FIND MATCH.
fn home(
    kit: &mut Kit,
    body: Entity,
    rulesets: &[RulesetRow],
    in_flight: (usize, u8),
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
    kit.bold(column, "PLAY", 30.0, ui::INK);
    if rulesets.is_empty() {
        kit.text(
            column,
            "The service offers no game this version of dbota can play.",
            16.0,
            ui::INK_DIM,
        );
    }
    for ruleset in rulesets {
        let card = kit.panel(
            column,
            Node {
                width: px(620),
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
        kit.bold(about, ruleset.name.clone(), 22.0, ui::INK);
        let rated = if ruleset.rated { "ranked" } else { "unranked" };
        kit.text(
            about,
            format!("{} · {rated}", ruleset.teams),
            15.0,
            ui::INK_DIM,
        );
        let can =
            cooldown.is_none() && (in_flight.1 == 0 || in_flight.0 < usize::from(in_flight.1));
        let look = if can { GREEN } else { PLAIN };
        kit.button(
            card,
            "FIND MATCH",
            Act::Find(ruleset.id.clone()),
            look,
            52.0,
        );
    }
    if in_flight.1 > 0 {
        kit.text(
            column,
            format!("Games going: {} of {} at once", in_flight.0, in_flight.1),
            14.0,
            ui::INK_DIM,
        );
    }
    if let Some(cooldown) = cooldown {
        kit.text(column, cooldown, 15.0, ERROR);
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
    kit.bold(column, "PROFILE", 30.0, ui::INK);
    kit.bold(column, name.to_owned(), 22.0, ui::INK);
    if ratings.is_empty() {
        kit.text(column, "No rated games yet.", 16.0, ui::INK_DIM);
    }
    for line in ratings {
        kit.text(column, line.clone(), 16.0, ui::INK);
    }
    if !ladder.is_empty() {
        kit.bold(column, "LADDER", 20.0, ui::INK);
    }
    for line in ladder {
        kit.text(column, line.clone(), 15.0, ui::INK_DIM);
    }
}

fn lobby_page(kit: &mut Kit, body: Entity, lobby: &LobbyView) {
    let card = kit.panel(
        body,
        Node {
            width: px(560),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(24)),
            row_gap: px(10),
            ..default()
        },
    );
    let lock = if lobby.locked { " · password" } else { "" };
    kit.bold(card, format!("LOBBY: {}{lock}", lobby.name), 24.0, ui::INK);
    for (name, team, ready) in &lobby.members {
        let row = kit.node(
            card,
            Node {
                column_gap: px(16),
                ..default()
            },
        );
        let side = if *team == 0 { "Radiant" } else { "Dire" };
        kit.text(row, format!("{side:8}"), 15.0, ui::INK_DIM);
        kit.text(row, name.clone(), 16.0, ui::INK);
        kit.text(
            row,
            if *ready { "READY" } else { "not ready" },
            15.0,
            if *ready { ui::HP_MINE } else { ui::INK_DIM },
        );
    }
    if lobby.waiting_approval > 0 {
        kit.text(
            card,
            format!(
                "{} waiting for the owner's approval",
                lobby.waiting_approval
            ),
            14.0,
            ui::INK_DIM,
        );
    }
    if let Some(secs) = lobby.starting_in_secs {
        kit.bold(card, format!("Starting in {secs}"), 22.0, ui::GOLDEN);
    }
    let row = kit.node(
        card,
        Node {
            column_gap: px(10),
            ..default()
        },
    );
    if lobby.me_ready {
        kit.button(row, "NOT READY", Act::ReadyLobby(false), PLAIN, 42.0);
    } else if let Some(why) = lobby.waiting {
        kit.disabled_button(row, "READY", 42.0);
        kit.text(card, why, 14.0, ui::INK_DIM);
    } else {
        kit.button(row, "READY", Act::ReadyLobby(true), GREEN, 42.0);
    }
    kit.button(row, "LEAVE", Act::LeaveLobby, RED, 42.0);
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
        Some(true) => ui::HP_MINE,
        Some(false) => ui::HP_FOE,
        None => ui::INK,
    };
    kit.bold(column, result.headline.clone(), 56.0, color);
    kit.text(column, result.detail.clone(), 16.0, ui::INK_DIM);
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
            if seat.mine { ui::GOLDEN } else { ui::INK },
        );
        kit.text(name, seat.hero.clone(), 14.0, ui::INK_DIM);
        kit.text(row, seat.rating.clone(), 16.0, ui::INK);
    }
    kit.button(column, "CONTINUE", Act::Go(Page::Home), GREEN, 46.0);
}

/// REPLAYS: the replays on this machine, the newest first, each to watch;
/// and any replay file by its path.
fn replays_page(kit: &mut Kit, body: Entity, list: &[ReplayRow], fields: &[FieldView]) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            width: px(PAGE_WIDTH),
            ..default()
        },
    );
    kit.bold(column, "REPLAYS", 30.0, ui::INK);
    if list.is_empty() {
        kit.text(
            column,
            "No replays yet: a local lobby keeps its games' here.",
            16.0,
            ui::INK_DIM,
        );
    }
    for (at, replay) in list.iter().enumerate() {
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
        let (headline, line, colour) = match &replay.summary {
            Summary::Read { sides, outcome } => (
                sides.clone(),
                format!("{}  ·  {outcome}", replay.when),
                ui::INK_DIM,
            ),
            Summary::Reading => (replay.when.clone(), "reading…".to_owned(), ui::INK_DIM),
            Summary::Broken(why) => (
                replay.when.clone(),
                format!("cannot be watched: {why}"),
                ERROR,
            ),
        };
        kit.bold(label, headline, 16.0, ui::INK);
        kit.text(label, line, 13.0, colour);
        kit.text(
            label,
            format!("{}  ·  {}", replay.name, replay.size),
            12.0,
            ui::INK_DIM,
        );
        if !matches!(replay.summary, Summary::Broken(_)) {
            let watch = kit.button(row, "WATCH", Act::OpenReplay(at), PLAIN, 34.0);
            kit.commands
                .entity(watch)
                .insert(Name::new(format!("watch {at}")));
        }
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
    for (at, field) in fields.iter().enumerate() {
        field_box(kit, card, at, field);
    }
    kit.button(card, "OPEN", Act::Submit, GREEN, 40.0);
}

/// What a local lobby's empty seats take: left open, the server's idle
/// dummy, or the bot command of the settings, as Dota's lobby leaves a slot
/// open or adds a bot to it.
fn seat_fill(kit: &mut Kit, card: Entity, chosen: SeatFill, bot_set: bool) {
    kit.text(card, "Empty seats", 13.0, ui::INK_DIM);
    let row = kit.node(
        card,
        Node {
            column_gap: px(8),
            ..default()
        },
    );
    for (fill, label) in [
        (SeatFill::Open, "OPEN"),
        (SeatFill::Dummy, "IDLE DUMMY"),
        (SeatFill::Bot, "BOT"),
    ] {
        let look = if fill == chosen { GREEN } else { PLAIN };
        kit.button(row, label, Act::Fill(fill), look, 34.0);
    }
    let note = match chosen {
        SeatFill::Open => "Others join by this machine's address under DIRECT CONNECT.",
        SeatFill::Dummy => {
            "A hero that stands at its fountain and never acts: to practise or test alone."
        }
        SeatFill::Bot if bot_set => "Each empty seat runs the bot command of the settings.",
        SeatFill::Bot => "No bot command is set yet: SETTINGS, Options, LOCAL LOBBY.",
    };
    let colour = if chosen == SeatFill::Bot && !bot_set {
        ui::GOLDEN
    } else {
        ui::INK_DIM
    };
    kit.text(card, note, 13.0, colour);
}

/// Dota's lobby setting "Enable cheats", for the local lobby's host: a box
/// that turns on and off.
fn cheats_toggle(kit: &mut Kit, card: Entity, on: bool) {
    let row = kit
        .commands
        .spawn((
            super::screens::MenuButton(Act::ToggleCheats),
            Button,
            Name::new("enable cheats"),
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
        BorderColor::all(if on { ui::GOLDEN } else { ui::INK_DIM }),
        BackgroundColor(if on { ui::GOLDEN } else { Color::NONE }),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    kit.text(row, "Enable cheats", 16.0, ui::INK);
}

/// DEMO HERO: every hero, its portrait and its name; a click starts Dota's
/// Demo Hero with it, alone on our map with every cheat.
fn demo_page(kit: &mut Kit, body: Entity, heroes: &[(u16, String)]) {
    let column = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(14),
            ..default()
        },
    );
    kit.bold(column, "DEMO HERO", 30.0, ui::INK);
    kit.text(
        column,
        "Try a hero alone with every cheat: levels, gold, items, free spells, enemies to fight.",
        16.0,
        ui::INK_DIM,
    );
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
                super::screens::MenuButton(Act::Demo(*id)),
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
                BackgroundColor(ui::PANEL),
                BorderColor::all(ui::EDGE),
                ChildOf(grid),
            ))
            .id();
        kit.portrait(card, *id, 240.0);
        kit.bold(card, name.to_uppercase(), 18.0, ui::INK);
    }
}
