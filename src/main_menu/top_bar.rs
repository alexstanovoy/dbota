//! Dota's dashboard top bar: the logo and DBOTA (the way home), RETURN TO GAME over a game, the
//! pages' tabs, then LOG OUT, the connection pill and the settings cog; and the quit button, at
//! one point of the screen whatever is drawn, the sign-in screen without a bar included.

use std::time::Duration;

use bevy::prelude::*;

use super::core::{Act, Page};
use super::kit::{Kit, MenuButton, Tone, Wash};
use super::screens::tab_of;
use super::view::{Connection, MenuView};
use crate::theme::{self, Face, Type};

/// The tabs of the pages there are for now, until their screens are redesigned: the service's
/// once signed in, and what is played without it always.
///
/// A browser cannot start a game server, so the local lobby is not in its bar, nor is Demo Hero
/// (the native client opens it; the browser may one day).
#[cfg(not(target_arch = "wasm32"))]
const TABS: [(Page, &str); 7] = [
    (Page::Home, "PLAY"),
    (Page::MyGames, "MY GAMES"),
    (Page::Lobbies, "LOBBIES"),
    (Page::Profile, "PROFILE"),
    (Page::LocalLobby, "LOCAL LOBBY"),
    (Page::DemoHero, "DEMO HERO"),
    (Page::Replays, "REPLAYS"),
];
#[cfg(target_arch = "wasm32")]
const TABS: [(Page, &str); 5] = [
    (Page::Home, "PLAY"),
    (Page::MyGames, "MY GAMES"),
    (Page::Lobbies, "LOBBIES"),
    (Page::Profile, "PROFILE"),
    (Page::Replays, "REPLAYS"),
];

/// Today's pages are more than the bar's two tabs of the design: their tabs are smaller.
const INTERIM_TAB: Type = Type {
    face: Face::Semibold,
    size: 15.0,
    tracking: 1.5,
    figures: false,
};

/// The quit button's icon, across.
const QUIT_ICON: f32 = 24.0;
/// The settings cog's icon, across.
const GEAR_ICON: f32 = 26.0;

/// The bar over the page: the first child of the page layer.
pub fn draw(kit: &mut Kit, root: Entity, view: &MenuView) {
    let bar = kit
        .commands
        .spawn((
            Node {
                height: px(theme::BAR_HEIGHT),
                flex_shrink: 0.0,
                padding: UiRect::new(px(14), px(theme::EDGE_GAP), px(0), px(0)),
                column_gap: px(6),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(1)),
                ..default()
            },
            theme::downward(theme::BAR),
            BorderColor::all(theme::BAR_EDGE),
            ChildOf(root),
        ))
        .id();
    // The line of light under the edge.
    kit.commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            bottom: px(-2),
            height: px(1),
            ..default()
        },
        BackgroundColor(theme::BAR_GLINT),
        Pickable::IGNORE,
        ChildOf(bar),
    ));
    logo(kit, bar);
    if view.game.is_some() {
        let back = kit.button(
            bar,
            "RETURN TO GAME",
            Act::ReturnToGame,
            Tone::Primary,
            38.0,
        );
        kit.commands
            .entity(back)
            .entry::<Node>()
            .and_modify(|mut node| node.margin.right = px(8));
    }
    tabs(kit, bar, view);
    kit.node(
        bar,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    let quiet = (theme::INK_3, theme::INK_1);
    match &view.account {
        Some(_) => {
            kit.quiet(bar, "LOG OUT", Act::Logout, theme::PILL, quiet);
        }
        None if view.page == Page::LocalLobby => {
            kit.quiet(bar, "SIGN IN", Act::Go(Page::Login), theme::PILL, quiet);
        }
        None => {}
    }
    pill(kit, bar, &view.connection);
    let gear = kit.icon_button(
        bar,
        "menu.gear",
        "SETTINGS",
        Act::Go(Page::Settings),
        GEAR_ICON,
    );
    kit.commands
        .entity(gear)
        .entry::<Node>()
        .and_modify(|mut node| node.margin.left = px(8));
    // The quit button's place, which it takes over the bar (see [`quit`]).
    if view.can_quit {
        kit.node(
            bar,
            Node {
                width: px(theme::HIT),
                height: px(theme::HIT),
                flex_shrink: 0.0,
                ..default()
            },
        );
    }
}

/// The logo and DBOTA, which go home.
fn logo(kit: &mut Kit, bar: Entity) {
    let home = kit
        .commands
        .spawn((
            MenuButton(Act::Go(Page::Home)),
            Button,
            Node {
                height: px(theme::BAR_HEIGHT),
                column_gap: px(10),
                padding: UiRect::right(px(18)),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            Name::new("dbota"),
            ChildOf(bar),
        ))
        .id();
    kit.art(
        home,
        "menu.logo",
        Node {
            width: px(34),
            height: px(34),
            ..default()
        },
    );
    kit.typed(home, "DBOTA", theme::WORDMARK, theme::INK_1);
}

/// One tab per page there is now, the current one underlined.
fn tabs(kit: &mut Kit, bar: Entity, view: &MenuView) {
    let row = kit.node(
        bar,
        Node {
            align_self: AlignSelf::Stretch,
            ..default()
        },
    );
    let lit = tab_of(view.page);
    for (page, label) in TABS {
        if page.needs_account() && view.account.is_none() {
            continue;
        }
        tab(kit, row, label, page, lit == page);
    }
}

/// The tab of `page`, underlined when it is the current one.
fn tab(kit: &mut Kit, row: Entity, label: &str, page: Page, on: bool) {
    let tab = kit
        .commands
        .spawn((
            MenuButton(Act::Go(page)),
            Button,
            Node {
                padding: UiRect::axes(px(12), px(0)),
                align_items: AlignItems::Center,
                border: UiRect::bottom(px(2)),
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(if on { theme::TAB_MARK } else { Color::NONE }),
            Name::new(label.to_lowercase()),
            ChildOf(row),
        ))
        .id();
    let rest = if on { theme::INK_1 } else { theme::INK_4 };
    let words = kit.typed(tab, label, INTERIM_TAB, rest);
    kit.commands.entity(words).insert(TextLayout {
        linebreak: LineBreak::NoWrap,
        ..default()
    });
    kit.commands.entity(tab).insert(Wash {
        target: words,
        rest,
        hover: theme::INK_1,
    });
}

/// The pill that says how the menu stands with the service: its name while connected, OFFLINE
/// without an account, RECONNECTING (a click retries at once) while the link is lost.
fn pill(kit: &mut Kit, bar: Entity, connection: &Connection) {
    let (ground, edge) = match connection {
        Connection::Online(_) => theme::PILL_ONLINE,
        Connection::Offline => theme::PILL_OFFLINE,
        Connection::Lost(_) => theme::PILL_LOST,
    };
    let pill = kit
        .commands
        .spawn((
            Node {
                height: px(32),
                padding: UiRect::axes(px(12), px(0)),
                column_gap: px(8),
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(theme::PILL_RADIUS)),
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(ground),
            BorderColor::all(edge),
            Name::new("connection"),
            ChildOf(bar),
        ))
        .id();
    match connection {
        Connection::Online(service) => {
            kit.commands.spawn((
                Node {
                    width: px(8),
                    height: px(8),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(theme::RADIANT),
                Pickable::IGNORE,
                ChildOf(pill),
            ));
            let words = Type {
                face: Face::Regular,
                tracking: 0.0,
                ..theme::PILL
            };
            kit.typed(pill, service.clone(), words, theme::INK_3);
        }
        Connection::Offline => {
            kit.typed(pill, "OFFLINE", theme::PILL, theme::INK_4);
        }
        Connection::Lost(service) => {
            kit.commands.entity(pill).insert((
                MenuButton(Act::RetryService),
                Button,
                Breathe,
                Name::new(format!("reconnecting to {service}")),
            ));
            kit.commands.spawn((
                Node {
                    width: px(13),
                    height: px(13),
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor {
                    top: Color::NONE,
                    ..BorderColor::all(theme::DIRE)
                },
                Spin,
                Pickable::IGNORE,
                ChildOf(pill),
            ));
            let words = Type {
                face: Face::Bold,
                tracking: 0.0,
                ..theme::PILL
            };
            kit.typed(pill, "RECONNECTING", words, theme::INK_1);
        }
    }
}

/// The quit button, at the same point of the screen on every screen: [`theme::EDGE_GAP`] from
/// the right and centred on the bar's height, whether a bar is drawn or not.
pub fn quit(kit: &mut Kit, root: Entity, view: &MenuView) {
    if !view.can_quit {
        return;
    }
    let quit = kit.icon_button(root, "menu.quit", "QUIT", Act::Quit, QUIT_ICON);
    kit.commands.entity(quit).insert((
        crate::voice::QuitClick,
        Name::new("quit"),
        Node {
            position_type: PositionType::Absolute,
            right: px(theme::EDGE_GAP),
            top: px((theme::BAR_HEIGHT - theme::HIT) / 2.0),
            width: px(theme::HIT),
            height: px(theme::HIT),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
    ));
}

/// The ring of RECONNECTING, a turn a second.
#[derive(Component)]
pub struct Spin;

/// The red halo of RECONNECTING, breathing in and out every 2.6 seconds: the one offline signal.
#[derive(Component)]
pub struct Breathe;

const SPIN_TURN: Duration = Duration::from_secs(1);
const BREATH: Duration = Duration::from_millis(2600);

/// Turns the rings and breathes the halos.
pub fn animate(
    time: Res<Time>,
    mut rings: Query<&mut UiTransform, With<Spin>>,
    mut halos: Query<(Entity, Option<&mut BoxShadow>), With<Breathe>>,
    mut commands: Commands,
) {
    let now = time.elapsed();
    let turn = (now.as_secs_f32() / SPIN_TURN.as_secs_f32()).fract();
    for mut ring in &mut rings {
        ring.rotation = Rot2::radians(turn * std::f32::consts::TAU);
    }
    // 0 at rest, 1 at the full breath: a cosine, smooth at both ends.
    let phase = (now.as_secs_f32() / BREATH.as_secs_f32()).fract();
    let breath = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
    let halo = BoxShadow::new(
        theme::DIRE.with_alpha(0.35 + 0.2 * breath),
        px(0),
        px(0),
        px(1.0 + 2.0 * breath),
        px(6.0 + 6.0 * breath),
    );
    for (pill, shadow) in &mut halos {
        match shadow {
            Some(mut shadow) => *shadow = halo.clone(),
            None => {
                commands.entity(pill).insert(halo.clone());
            }
        }
    }
}
