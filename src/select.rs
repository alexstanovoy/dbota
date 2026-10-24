//! Hero selection, as Dota opens a match: every hero's portrait in a grid,
//! picked with a click, a Ready button, and each seat's pick and readiness,
//! with the time left to pick. Once every pick is final the tactical phase
//! opens the shop to choose what to buy when the match begins. Mouse only. A
//! spectator sees the seats and waits.

use bevy::prelude::*;
use bota_client_core::{Intent, Phase};
use bota_core::HEROES;
use bota_proto::{PickPhase, Role};

use crate::hud::Blocker;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// How far down the tactical phase's line starts: below the top bar's
/// portraits and clock.
const BANNER_TOP: f32 = 60.0;

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

/// Whether the tactical phase is on, for the systems that cannot read the
/// session (a run condition may not be on the main thread).
#[derive(Resource, Default, PartialEq)]
pub struct TacticalPhase(pub bool);

/// Whether one's hero is known: the match is, or the tactical phase before
/// it holds the pick. The shop works then, what is chosen in the tactical
/// phase queued for the match's start, and the hero panel shows the hero.
pub fn match_or_tactical(
    known: Res<crate::session::MatchKnown>,
    tactical: Res<TacticalPhase>,
) -> bool {
    known.0 || tactical.0
}

/// The line over the shop in the tactical phase.
#[derive(Component)]
struct Banner;

/// The banner's smaller lines: the gold and what is queued.
#[derive(Component)]
struct BannerHint;

/// The selection clock as last told, and when it was told.
#[derive(Resource, Default)]
struct ClockSeen {
    phase: PickPhase,
    at: f32,
}

impl ClockSeen {
    /// The seconds left of the phase running, if it counts down.
    fn left(&self, now: f32) -> Option<f32> {
        let told = match self.phase {
            PickPhase::Selecting { left_ms } | PickPhase::Late { left_ms } => left_ms,
            PickPhase::Tactical { left_ms } => left_ms,
            PickPhase::Waiting => return None,
        };
        Some((told as f32 / 1000.0 - (now - self.at)).max(0.0))
    }
}

/// `m:ss`, rounded up as clocks show a countdown.
fn clock(secs: f32) -> String {
    let whole = secs.ceil() as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

const CARD: (f32, f32) = (150.0, 214.0);
const DEAD: Color = Color::srgb(0.12, 0.12, 0.13);
const PICKED: Color = Color::srgb(0.95, 0.75, 0.3);

pub fn plugin(app: &mut App) {
    app.init_resource::<ClockSeen>()
        .init_resource::<TacticalPhase>()
        .add_systems(Startup, spawn)
        .add_systems(Update, (note_clock, show, clicks, looks, tactical).chain());
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
                Name::new(format!("pick {}", hero.name.to_lowercase())),
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
            Name::new("pick ready"),
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
    // Under the top bar, as Dota's strategy time line sits under its clock;
    // its smaller lines keep clear of the shop open at the right.
    let banner = commands
        .spawn((
            Banner,
            ui::text("", &fonts.bold, 22.0, ui::INK),
            ui::shadow(),
            TextLayout::justify(Justify::Center),
            Node {
                position_type: PositionType::Absolute,
                top: px(BANNER_TOP),
                left: px(0),
                right: px(0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .id();
    commands.spawn((
        BannerHint,
        TextSpan::default(),
        TextFont {
            font: fonts.regular.clone().into(),
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(ui::INK),
        ChildOf(banner),
    ));
    commands.spawn((
        Seats,
        ui::text("", &fonts.regular, 16.0, ui::INK_DIM),
        TextLayout::justify(Justify::Center),
        ChildOf(screen),
    ));
}

/// Remembers when the clock was last told, to count down from there.
fn note_clock(session: NonSend<Session>, time: Res<Time>, mut seen: ResMut<ClockSeen>) {
    if seen.phase != session.0.lobby.pick {
        seen.phase = session.0.lobby.pick;
        seen.at = time.elapsed_secs();
    }
}

/// The screen while the match has not begun and the tactical phase has not
/// opened the shop.
fn show(session: NonSend<Session>, mut screen: Single<&mut Visibility, With<Screen>>) {
    screen.set_if_neq(ui::visible(
        session.0.phase == Phase::Lobby
            && session.0.view().is_none()
            && !session.0.in_tactical_phase(),
    ));
}

/// The tactical phase: the shop open, the time left, and what is queued
/// against the starting gold.
fn tactical(
    session: NonSend<Session>,
    seen: Res<ClockSeen>,
    time: Res<Time>,
    mut panels: ResMut<crate::input::Panels>,
    mut phase: ResMut<TacticalPhase>,
    mut opened: Local<bool>,
    mut banner: Single<(&mut Text, &mut Visibility), With<Banner>>,
    mut hint: Single<&mut TextSpan, With<BannerHint>>,
) {
    let core = &session.0;
    let on = core.in_tactical_phase();
    phase.set_if_neq(TacticalPhase(on));
    banner.1.set_if_neq(ui::visible(on));
    if !on {
        *opened = false;
        return;
    }
    if !*opened {
        panels.shop = true;
        *opened = true;
    }
    let left = seen.left(time.elapsed_secs()).map_or(String::new(), clock);
    let names: Vec<&str> = core
        .queued_buys
        .iter()
        .filter_map(|id| bota_client_core::ITEMS.get(usize::from(*id)))
        .map(|face| face.name)
        .collect();
    let queue = if names.is_empty() {
        "click items to buy them when the match begins; right-click takes one back".to_string()
    } else {
        format!("on the list: {}", names.join(", "))
    };
    banner
        .0
        .set_if_neq(Text(format!("TACTICAL PHASE - the match begins in {left}")));
    let lines = format!(
        "\nGold {} of {}\n{queue}",
        core.gold_after_queue(),
        bota_core::rules::STARTING_GOLD
    );
    if hint.0 != lines {
        hint.0 = lines;
    }
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
        if *interaction == Interaction::Pressed && !core.ready && !taken(core, card.0) {
            core.act(Intent::PickHero(card.0));
        }
    }
    let picked = my_pick(core).is_some();
    if ready.iter().any(|i| *i == Interaction::Pressed) && (picked || core.ready) {
        core.act(Intent::ToggleReady);
    }
}

/// Whether another seat holds this hero while the rules forbid two of one.
fn taken(core: &bota_client_core::App, hero: u16) -> bool {
    !core.lobby.same_heroes
        && core
            .lobby
            .slots
            .iter()
            .any(|s| Some(s.slot) != core.my_slot && s.hero.is_some_and(|h| h.0 == hero))
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

/// The line over the grid while picking.
fn heading_text(phase: PickPhase, left: Option<f32>) -> String {
    let time = left.map_or(String::new(), clock);
    match phase {
        PickPhase::Selecting { .. } => format!("CHOOSE YOUR HERO  {time}"),
        PickPhase::Late { .. } => {
            format!("PICK NOW - losing 2 gold a second, a hero is given in {time}")
        }
        PickPhase::Waiting | PickPhase::Tactical { .. } => "CHOOSE YOUR HERO".to_string(),
    }
}

/// The picked card in gold, the Ready button's state, and every seat.
#[allow(clippy::type_complexity)]
fn looks(
    session: NonSend<Session>,
    menu: Option<NonSend<crate::main_menu::MenuCore>>,
    seen: Res<ClockSeen>,
    time: Res<Time>,
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
        } else if taken(core, card.0) {
            DEAD
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
    let mut seats: Vec<String> = core
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
            let kind = if seat.role == Some(Role::Bot) {
                " (bot)"
            } else {
                ""
            };
            format!("{:?}: {name}{kind}, {hero}{ready}", seat.team)
        })
        .collect();
    // A local lobby this client hosts: where the others join while a seat
    // is open.
    let open = core.lobby.slots.iter().any(|seat| seat.name.is_empty());
    if let Some(at) = menu.as_deref().and_then(|m| m.hosted_at())
        && open
    {
        seats.push(format!("\nOthers join under DIRECT CONNECT at {at}"));
    }
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
                heading_text(seen.phase, seen.left(time.elapsed_secs()))
            } else if core.lobby.me.is_some() {
                "SPECTATING - waiting for the match".to_string()
            } else {
                "CONNECTING...".to_string()
            }
        } else {
            continue;
        };
        text.set_if_neq(Text(s));
    }
}
