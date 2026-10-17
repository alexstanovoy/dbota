//! Hero selection, as Dota opens a match: every hero's portrait in a grid,
//! picked with a click, a Ready button, and each seat's pick and readiness.
//! Mouse only. A spectator sees the seats and waits.

use bevy::prelude::*;
use bota_client_core::{Intent, Phase};
use bota_core::HEROES;

use crate::hud::Blocker;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The whole screen.
#[derive(Component)]
struct Screen;

/// A hero's card, by hero id.
#[derive(Component)]
struct Card(u16);

/// The Ready button.
#[derive(Component)]
struct ReadyButton;

/// The text on the Ready button.
#[derive(Component)]
struct ReadyText;

/// The line over the grid.
#[derive(Component)]
struct Heading;

/// The list of seats.
#[derive(Component)]
struct Seats;

const CARD: (f32, f32) = (150.0, 214.0);
const PICKED: Color = Color::srgb(0.95, 0.75, 0.3);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_systems(Update, (show, clicks, looks));
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    let screen = commands
        .spawn((
            Screen,
            Blocker,
            Interaction::default(),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(18),
                ..ui::fill()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.035, 0.96)),
            GlobalZIndex(40),
            Visibility::Hidden,
        ))
        .id();
    commands.spawn((
        Heading,
        ui::text("CHOOSE YOUR HERO", &fonts.bold, 30.0, ui::INK),
        ui::shadow(),
        ChildOf(screen),
    ));
    let grid = commands
        .spawn((
            Node {
                column_gap: px(16),
                ..default()
            },
            ChildOf(screen),
        ))
        .id();
    for (id, hero) in (0u16..).zip(HEROES) {
        let portrait = manifest.portrait(id);
        let card = commands
            .spawn((
                Card(id),
                Button,
                Node {
                    width: px(CARD.0),
                    height: px(CARD.1 + 30.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BorderColor::all(ui::EDGE),
                BackgroundColor(ui::PANEL_DEEP),
                ChildOf(grid),
            ))
            .id();
        commands.spawn((
            Node {
                width: px(CARD.0 - 4.0),
                height: px(CARD.1),
                ..default()
            },
            ImageNode::new(portrait.map(|p| server.load(p)).unwrap_or_default()),
            Pickable::IGNORE,
            ChildOf(card),
        ));
        commands.spawn((
            ui::text(hero.name, &fonts.bold, 16.0, ui::INK),
            Pickable::IGNORE,
            ChildOf(card),
        ));
    }
    let ready = commands
        .spawn((
            ReadyButton,
            Button,
            Node {
                width: px(220),
                height: px(48),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BorderColor::all(ui::EDGE),
            BackgroundColor(Color::srgb(0.12, 0.3, 0.14)),
            ChildOf(screen),
        ))
        .id();
    commands.spawn((
        ReadyText,
        ui::text("READY", &fonts.bold, 20.0, ui::INK),
        Pickable::IGNORE,
        ChildOf(ready),
    ));
    commands.spawn((
        Seats,
        ui::text("", &fonts.regular, 16.0, ui::INK_DIM),
        TextLayout::justify(Justify::Center),
        ChildOf(screen),
    ));
}

/// The screen while the match has not begun.
fn show(session: NonSend<Session>, mut screen: Single<&mut Visibility, With<Screen>>) {
    screen.set_if_neq(ui::visible(
        session.0.phase == Phase::Lobby && session.0.view().is_none(),
    ));
}

/// A click on a portrait picks that hero; on Ready, readies or unreadies.
#[allow(clippy::type_complexity)]
fn clicks(
    mut session: NonSendMut<Session>,
    cards: Query<(&Interaction, &Card), Changed<Interaction>>,
    ready: Query<&Interaction, (Changed<Interaction>, With<ReadyButton>)>,
) {
    let core = &mut session.0;
    if core.phase != Phase::Lobby || core.my_slot.is_none() {
        return;
    }
    for (interaction, card) in &cards {
        if *interaction == Interaction::Pressed && !core.ready {
            core.act(Intent::PickHero(card.0));
        }
    }
    let picked = my_pick(core).is_some();
    if ready.iter().any(|i| *i == Interaction::Pressed) && (picked || core.ready) {
        core.act(Intent::ToggleReady);
    }
}

/// The hero one's seat has picked, as the lobby tells it.
fn my_pick(core: &bota_client_core::App) -> Option<u16> {
    let slot = core.my_slot?;
    core.lobby
        .slots
        .iter()
        .find(|s| s.slot == slot)
        .and_then(|s| s.hero)
        .map(|h| h.0)
}

/// The picked card in gold, the Ready button's state, and every seat.
#[allow(clippy::type_complexity)]
fn looks(
    session: NonSend<Session>,
    mut cards: Query<(&Card, &Interaction, &mut BorderColor), Without<ReadyButton>>,
    mut button: Query<(&mut BackgroundColor, &mut Visibility), With<ReadyButton>>,
    mut texts: Query<(
        &mut Text,
        Option<&ReadyText>,
        Option<&Seats>,
        Option<&Heading>,
    )>,
) {
    let core = &session.0;
    if core.phase != Phase::Lobby {
        return;
    }
    let playing = core.my_slot.is_some();
    let mine = my_pick(core);
    for (card, interaction, mut rim) in &mut cards {
        let c = if mine == Some(card.0) {
            PICKED
        } else if *interaction == Interaction::Hovered && playing && !core.ready {
            ui::INK
        } else {
            ui::EDGE
        };
        rim.set_if_neq(BorderColor::all(c));
    }
    for (mut bg, mut v) in &mut button {
        v.set_if_neq(ui::visible(playing));
        bg.set_if_neq(BackgroundColor(match (core.ready, mine.is_some()) {
            (true, _) => Color::srgb(0.12, 0.42, 0.18),
            (false, true) => Color::srgb(0.12, 0.3, 0.14),
            (false, false) => Color::srgb(0.12, 0.13, 0.15),
        }));
    }
    let seats: Vec<String> = core
        .lobby
        .slots
        .iter()
        .map(|seat| {
            let hero = seat
                .hero
                .and_then(bota_core::hero_def)
                .map_or("picking...", |h| h.name);
            let name = if seat.name.is_empty() {
                "open"
            } else {
                seat.name.as_str()
            };
            let ready = if seat.ready { "  READY" } else { "" };
            format!("{:?}: {name}, {hero}{ready}", seat.team)
        })
        .collect();
    for (mut text, ready, list, heading) in &mut texts {
        let s = if ready.is_some() {
            if core.ready {
                "READY - click to cancel"
            } else {
                "READY"
            }
            .to_string()
        } else if list.is_some() {
            seats.join("\n")
        } else if heading.is_some() {
            if playing {
                "CHOOSE YOUR HERO"
            } else if core.lobby.me.is_some() {
                "SPECTATING - waiting for the match"
            } else {
                "CONNECTING..."
            }
            .to_string()
        } else {
            continue;
        };
        text.set_if_neq(Text(s));
    }
}
