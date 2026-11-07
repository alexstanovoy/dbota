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
pub fn draw(kit: &mut Kit, root: Entity, view: &MenuView, fold: TabFold) {
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
    tabs(kit, bar, view, fold);
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
        None => {
            kit.quiet(bar, "SIGN IN", Act::Go(Page::Login), theme::PILL, quiet);
        }
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

/// The tabs the bar has no room for: the last `folded` of them stand in MORE's list, open or
/// not. The bar folds one more on each frame its tabs overflow it, and unfolds them all when the
/// window or the interface's scale changes.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TabFold {
    pub folded: usize,
    pub open: bool,
}

/// The row of the tabs, which overflows when the bar has no room for them.
#[derive(Component)]
pub struct TabRow;

/// MORE ▾, which opens and closes the list of the folded tabs.
#[derive(Component)]
pub struct MoreButton;

/// One tab per page there is now, the current one underlined; those the bar has no room for
/// under MORE.
fn tabs(kit: &mut Kit, bar: Entity, view: &MenuView, fold: TabFold) {
    let row = kit.node(
        bar,
        Node {
            align_self: AlignSelf::Stretch,
            // It shrinks below its tabs when the bar has no room: they overflow it then, for
            // the frame until [`fit_tabs`] folds one.
            min_width: px(0),
            ..default()
        },
    );
    kit.commands.entity(row).insert(TabRow);
    let lit = tab_of(view.beneath);
    let shown: Vec<(Page, &str)> = TABS
        .into_iter()
        .filter(|(page, _)| !page.needs_account() || view.account.is_some())
        .collect();
    let (kept, folded) = shown.split_at(shown.len().saturating_sub(fold.folded));
    for &(page, label) in kept {
        let tab = tab(kit, row, label, lit == page);
        kit.commands.entity(tab).insert(MenuButton(Act::Go(page)));
    }
    if folded.is_empty() {
        return;
    }
    let more_lit = folded.iter().any(|(page, _)| *page == lit);
    let more = tab(kit, row, "MORE ▾", more_lit);
    kit.commands
        .entity(more)
        .insert((MoreButton, Name::new("more")));
    if !fold.open {
        return;
    }
    // The list hangs from the bar's foot under MORE, over the page.
    let list = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(theme::BAR_HEIGHT),
                min_width: px(220),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(theme::TOOLTIP),
            BorderColor::all(theme::TOOLTIP_EDGE),
            ChildOf(more),
        ))
        .id();
    for &(page, label) in folded {
        let entry = tab(kit, list, label, lit == page);
        kit.commands.entity(entry).insert(MenuButton(Act::Go(page)));
        kit.commands
            .entity(entry)
            .entry::<Node>()
            .and_modify(|mut node| {
                node.height = px(theme::HIT);
                node.padding = UiRect::axes(px(16), px(0));
            });
    }
}

/// A tab of `label`, underlined when it is the current page's.
fn tab(kit: &mut Kit, row: Entity, label: &str, on: bool) -> Entity {
    let tab = kit
        .commands
        .spawn((
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
    tab
}

/// Folds one more tab under MORE while the tabs overflow the bar, and unfolds them all when the
/// window or the interface's scale changes: at most one fold a frame, so the bar settles within
/// as many frames as it has tabs.
pub fn fit_tabs(
    rows: Query<(&ComputedNode, &Children), With<TabRow>>,
    tabs: Query<&ComputedNode>,
    mut resized: MessageReader<bevy::window::WindowResized>,
    ui_scale: Res<UiScale>,
    mut fold: ResMut<TabFold>,
) {
    if !resized.is_empty() || ui_scale.is_changed() {
        resized.clear();
        if fold.folded != 0 {
            *fold = TabFold::default();
        }
        return;
    }
    let Ok((row, children)) = rows.single() else {
        return;
    };
    // The tabs' own widths: MORE's open list hangs from it, wider than it, and is no tab.
    let needed: f32 = tabs.iter_many(children).map(|tab| tab.size().x).sum();
    if needed > row.size().x + 0.5 && fold.folded < TABS.len() {
        fold.folded += 1;
    }
}

/// MORE opens and closes its list; any other button of the menu closes it.
pub fn more_clicks(
    more: Query<&Interaction, (Changed<Interaction>, With<MoreButton>)>,
    others: Query<&Interaction, (Changed<Interaction>, With<MenuButton>)>,
    mut fold: ResMut<TabFold>,
) {
    let pressed = |i: &Interaction| *i == Interaction::Pressed;
    if more.iter().any(pressed) {
        fold.open = !fold.open;
    } else if fold.open && others.iter().any(pressed) {
        fold.open = false;
    }
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
            kit.typed(pill, shortened(service), words, theme::INK_3);
        }
        Connection::Offline => {
            kit.typed(pill, "OFFLINE", theme::PILL, theme::INK_4);
        }
        Connection::Lost(_) => {
            kit.commands.entity(pill).insert((
                MenuButton(Act::RetryService),
                Button,
                Breathe,
                Name::new("reconnecting"),
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

/// The most characters of a service's name the pill shows: as wide as a common host name
/// (`play.example.org`), so the fullest bar fits the menu's frame whatever the name.
const PILL_NAME: usize = 20;

/// A service's name as the pill shows it: cut to [`PILL_NAME`] characters, an ellipsis last.
fn shortened(name: &str) -> String {
    if name.chars().count() <= PILL_NAME {
        return name.to_owned();
    }
    let kept: String = name.chars().take(PILL_NAME - 1).collect();
    format!("{kept}…")
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
