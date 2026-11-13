//! Hero selection, as Dota opens a match: the seats along the top and the time left to pick, every
//! hero the server offers as a card in the group of its primary attribute, the panel of the hero
//! under the pointer (else the one chosen), LOCK IN and RANDOM. A click on a card chooses a hero
//! for the panel; LOCK IN sends the pick and the seat's readiness together, and a pick is final
//! once locked in (the server's rule), so nothing is sent before it. Once every pick is final the
//! tactical phase shows Dota's strategy screen ([`crate::strategy`]). A spectator sees the seats
//! and waits.
//!
//! Nothing here is the game's own that bota has no data for: no friends-and-foes statistics, no
//! guides, no stickers, no level badges on the cards, no sort or filter rows (a pool of a few
//! heroes needs none).

use bevy::prelude::*;
use bevy::{
    picking::{events::PointerPress, hover::Hovered, pointer::PointerButton},
    ui_widgets::Button,
};
use bota_client_core::{Intent, Phase};
use bota_core::HEROES;
use bota_proto::{Attribute, PickPhase, Role};

use crate::dota_text::Texts;
use crate::hud::Blocker;
use crate::manifest::Manifest;
use crate::player_colour::PlayerColours;
use crate::pregame::reference as r;
use crate::pregame::{self, Kit, Lift, PICK, Spot, bar, panel};
use crate::session::Session;
use crate::theme::{self, Face};
use crate::ui::{self, Cover, Fonts};

/// The whole screen.
#[derive(Component)]
struct Screen;

/// A hero's card, by hero id.
#[derive(Component)]
struct Card(u16);

/// The dark over a card whose hero cannot be taken.
#[derive(Component)]
struct Veil(u16);

/// LOCK IN, and the words on it: the hero it locks in.
#[derive(Component)]
struct LockIn;

#[derive(Component)]
struct LockInName;

/// RANDOM.
#[derive(Component)]
struct RandomButton;

/// The big countdown, and the line under it.
#[derive(Component)]
struct ClockText;

#[derive(Component)]
struct ClockCaption;

/// The line over the grid.
#[derive(Component)]
struct Heading;

/// What the screen says about where the selection stands, under the heading.
#[derive(Component)]
struct Status;

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

/// The selection clock as last told, and when it was told.
#[derive(Resource, Default)]
pub struct ClockSeen {
    phase: PickPhase,
    at: f32,
}

impl ClockSeen {
    /// The seconds left of the phase running, if it counts down.
    pub fn left(&self, now: f32) -> Option<f32> {
        let told = match self.phase {
            PickPhase::Selecting { left_ms } | PickPhase::Late { left_ms } => left_ms,
            PickPhase::Tactical { left_ms } => left_ms,
            PickPhase::Waiting => return None,
        };
        Some((told as f32 / 1000.0 - (now - self.at)).max(0.0))
    }
}

/// `m:ss`, rounded up as clocks show a countdown.
pub fn clock(secs: f32) -> String {
    let whole = secs.ceil() as u32;
    format!("{}:{:02}", whole / 60, whole % 60)
}

/// What the seat has done on this screen: the hero it chose for the panel (nothing is sent until
/// LOCK IN) and the hero the panel shows now.
#[derive(Resource, Default)]
pub struct Choice {
    pub chosen: Option<u16>,
    shown: Option<u16>,
}

/// What a RANDOM draws from: a state of its own, so a test fixes the draw.
#[derive(Resource)]
pub struct Die(pub u64);

impl Die {
    /// A die seeded from the clock: the draw of a person is not meant to repeat.
    fn from_clock() -> Die {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos() as u64);
        Die(nanos)
    }

    /// The next number (splitmix64).
    pub(crate) fn roll(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// What the screen was built for: who sits where, the heroes on offer and the rule of repeats.
#[derive(Default, PartialEq)]
struct Built(bar::Roster, Vec<u16>, bool);

/// The look of the cards, the buttons and the status.
const CARD_EDGE: Color = Color::srgba(0.42, 0.47, 0.54, 0.6);
const CHOSEN: Color = Color::srgb(0.50, 0.83, 0.96);
const DEAD: Color = Color::srgba(0.02, 0.03, 0.04, 0.72);
const BUTTON: Color = Color::srgb(0.15, 0.21, 0.28);
const BUTTON_LIT: Color = Color::srgb(0.21, 0.29, 0.38);
const BUTTON_EDGE: Color = Color::srgb(0.30, 0.38, 0.47);
const BUTTON_OFF: Color = Color::srgb(0.09, 0.11, 0.14);
const WARNING: Color = Color::srgb(0.98, 0.54, 0.42);

pub fn plugin(app: &mut App) {
    app.init_resource::<ClockSeen>()
        .init_resource::<TacticalPhase>()
        .init_resource::<Choice>()
        .insert_resource(Die::from_clock())
        .add_observer(clicks)
        .add_systems(
            Update,
            (build, show, note_clock, looks, panel_content, words).chain(),
        )
        .add_systems(
            PreUpdate,
            tactical
                .after(crate::session::pump)
                .before(bevy::picking::PickingSystems::Hover),
        );
}

/// The heroes a seat may pick, as the server's pool says (every hero when it names none).
fn pool(core: &bota_client_core::App) -> Vec<u16> {
    (0u16..)
        .zip(HEROES)
        .map(|(id, _)| id)
        .filter(|id| offered(core, *id))
        .collect()
}

/// Builds the screen anew when the seats or the pool change.
#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    session: NonSend<Session>,
    art: (Res<Fonts>, Res<Manifest>, Res<AssetServer>, Res<Texts>),
    colours: Res<PlayerColours>,
    screens: Query<Entity, With<Screen>>,
    mut choice: ResMut<Choice>,
    mut built: Local<Built>,
) {
    let core = &session.0;
    if core.phase != Phase::Lobby {
        return;
    }
    let heroes = pool(core);
    let now = Built(
        bar::Roster::of(core),
        heroes.clone(),
        core.lobby.same_heroes,
    );
    if *built == now && !screens.is_empty() {
        return;
    }
    *built = now;
    for screen in &screens {
        commands.entity(screen).despawn();
    }
    choice.shown = None;
    let kit = Kit {
        fonts: &art.0,
        manifest: &art.1,
        server: &art.2,
        texts: &art.3,
    };
    let root = commands
        .spawn((
            Screen,
            Name::new("pick screen"),
            Blocker,
            Hovered::default(),
            ui::fill(),
            theme::downward((
                Color::srgb_u8(0x1D, 0x26, 0x2F),
                Color::srgb_u8(0x0D, 0x11, 0x16),
            )),
            // Under the popups of the abilities (30), which it shows.
            GlobalZIndex(25),
            Visibility::Hidden,
        ))
        .id();
    bar::spawn(&mut commands, &kit, bar::Style::Pick, core, &colours, root);
    clock_block(&mut commands, &kit, root);
    heading_block(&mut commands, &kit, root);
    groups(&mut commands, &kit, &heroes, root);
    right_column(&mut commands, &kit, core, root);
}

/// The countdown at the top middle with a line under it.
fn clock_block(commands: &mut Commands, kit: &Kit, root: Entity) {
    let shot = PICK;
    let centre = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                margin: UiRect::left(shot.val(-r::CLOCK_WIDTH / 2.0)),
                top: shot.val(r::CLOCK[0]),
                width: shot.val(r::CLOCK_WIDTH),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        ClockText,
        shot.text(kit, "", (Face::Regular, r::CLOCK[2], 0.0), theme::INK_1),
        Node {
            height: shot.val(r::CLOCK[1]),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(centre),
    ));
    commands.spawn((
        ClockCaption,
        shot.text(
            kit,
            "",
            (Face::Semibold, r::CLOCK_CAPTION, 1.0),
            theme::INK_2,
        ),
        Pickable::IGNORE,
        ChildOf(centre),
    ));
}

/// CHOOSE YOUR HERO, and under it what the selection says of itself.
fn heading_block(commands: &mut Commands, kit: &Kit, root: Entity) {
    let shot = PICK;
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: shot.val(r::HEADING[0]),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: shot.val(6.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Heading,
        shot.text(
            kit,
            "",
            (Face::Regular, r::HEADING[1], r::HEADING[2]),
            theme::INK_2,
        ),
        Pickable::IGNORE,
        ChildOf(column),
    ));
    commands.spawn((
        Status,
        shot.text(kit, "", (Face::Regular, 24.0, 0.0), theme::INK_4),
        TextLayout::justify(Justify::Center),
        Pickable::IGNORE,
        ChildOf(column),
    ));
}

/// The groups of cards, each under the mark and the name of its attribute: only the groups that
/// hold a hero the server offers, side by side from the left.
fn groups(commands: &mut Commands, kit: &Kit, pool: &[u16], root: Entity) {
    let shot = PICK;
    let row = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: shot.val(r::GROUP_LEFT),
                top: shot.val(r::GROUP_HEADER[0]),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    let mut first_card = true;
    for (primary, mark, title) in [
        (Some(Attribute::Strength), Some("hud.strength"), "STRENGTH"),
        (Some(Attribute::Agility), Some("hud.agility"), "AGILITY"),
        (
            Some(Attribute::Intelligence),
            Some("hud.intelligence"),
            "INTELLIGENCE",
        ),
        (None, None, "UNIVERSAL"),
    ] {
        let heroes: Vec<u16> = pool
            .iter()
            .copied()
            .filter(|id| {
                bota_core::hero_def(bota_proto::HeroId(*id))
                    .is_some_and(|d| d.unit.primary == primary)
            })
            .collect();
        if heroes.is_empty() {
            continue;
        }
        let group = commands
            .spawn((
                Node {
                    width: shot.val(r::GROUP_PITCH),
                    flex_direction: FlexDirection::Column,
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row),
            ))
            .id();
        group_header(commands, kit, mark, title, group);
        let cards = commands
            .spawn((
                Node {
                    margin: UiRect::left(shot.val(r::CARD_INSET)),
                    width: shot.val(r::CARD_COLUMNS as f32 * r::CARD_PITCH[0] - 13.0),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: shot.val(r::CARD_PITCH[0] - r::CARD[0]),
                    row_gap: shot.val(r::CARD_PITCH[1] - r::CARD[1]),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(group),
            ))
            .id();
        for id in heroes {
            card(commands, kit, id, first_card, cards);
            first_card = false;
        }
    }
}

fn group_header(
    commands: &mut Commands,
    kit: &Kit,
    mark: Option<&str>,
    title: &str,
    group: Entity,
) {
    let shot = PICK;
    let header = commands
        .spawn((
            Node {
                height: shot.val(r::CARDS_TOP - r::GROUP_HEADER[0]),
                align_items: AlignItems::Center,
                column_gap: shot.val(10.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(group),
        ))
        .id();
    if let Some(mark) = mark {
        commands.spawn((
            Node {
                width: shot.val(r::GROUP_HEADER[1]),
                height: shot.val(r::GROUP_HEADER[1]),
                ..default()
            },
            kit.art(mark),
            Pickable::IGNORE,
            ChildOf(header),
        ));
    }
    commands.spawn((
        shot.text(kit, title, (Face::Semibold, 22.0, 4.0), theme::INK_4),
        Pickable::IGNORE,
        ChildOf(header),
    ));
}

/// A hero's card: its tall portrait in a frame that lights under the pointer.
fn card(commands: &mut Commands, kit: &Kit, id: u16, first: bool, cards: Entity) {
    let shot = PICK;
    let name = bota_core::hero_def(bota_proto::HeroId(id)).map_or("", |d| d.name);
    let card = commands
        .spawn((
            Card(id),
            Button,
            Lift(1.1),
            // Scripts click `pick <hero>`.
            Name::new(format!("pick {}", name.to_lowercase())),
            Node {
                width: shot.val(r::CARD[0]),
                height: shot.val(r::CARD[1]),
                border: UiRect::all(px(2)),
                flex_shrink: 0.0,
                ..default()
            },
            Cover,
            kit.image(kit.manifest.portrait(id)),
            BorderColor::all(CARD_EDGE),
            ChildOf(cards),
        ))
        .id();
    if first {
        commands.entity(card).insert(Spot("pick card 0"));
    }
    commands.spawn((
        Veil(id),
        ui::fill(),
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
        ChildOf(card),
    ));
}

/// At the right: the panel of the hero shown and, under it, LOCK IN and RANDOM.
fn right_column(commands: &mut Commands, kit: &Kit, core: &bota_client_core::App, root: Entity) {
    let shot = PICK;
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: shot.val(r::PANEL[0]),
                top: shot.val(r::PANEL[1]),
                width: shot.val(r::PANEL[2]),
                flex_direction: FlexDirection::Column,
                row_gap: shot.val(r::BUTTONS_GAP[1]),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    panel::shell(commands, column);
    let forced = core.lobby.heroes.len() == 1;
    if core.my_slot.is_none() || forced {
        return;
    }
    let row = commands
        .spawn((
            Node {
                column_gap: shot.val(r::BUTTONS_GAP[0]),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id();
    let lock = commands
        .spawn((
            LockIn,
            Button,
            Lift(1.03),
            Name::new("pick ready"),
            Spot("pick lock in"),
            button_node(shot.val(r::LOCK_IN[0]), shot.val(r::LOCK_IN[1])),
            BackgroundColor(BUTTON_OFF),
            BorderColor::all(BUTTON_EDGE),
            ChildOf(row),
        ))
        .id();
    commands.spawn((
        shot.text(kit, "LOCK IN", (Face::Regular, 26.0, 4.0), theme::INK_4),
        Pickable::IGNORE,
        ChildOf(lock),
    ));
    commands.spawn((
        LockInName,
        shot.text(kit, "", (pregame::NAME_FACE, 38.0, 1.0), theme::INK_1),
        TextLayout::no_wrap(),
        Pickable::IGNORE,
        ChildOf(lock),
    ));
    let random = commands
        .spawn((
            RandomButton,
            Button,
            Lift(1.03),
            Name::new("pick random"),
            Spot("pick random"),
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: shot.val(8.0),
                ..button_node(shot.val(r::RANDOM[0]), shot.val(r::RANDOM[1]))
            },
            BackgroundColor(BUTTON_OFF),
            BorderColor::all(BUTTON_EDGE),
            ChildOf(row),
        ))
        .id();
    die(commands, shot, random);
    commands.spawn((
        shot.text(kit, "RANDOM", (Face::Regular, 22.0, 2.0), theme::INK_3),
        Pickable::IGNORE,
        ChildOf(random),
    ));
}

fn button_node(width: Val, height: Val) -> Node {
    Node {
        width,
        height,
        flex_direction: FlexDirection::Column,
        padding: UiRect::axes(PICK.val(18.0), PICK.val(6.0)),
        border: UiRect::all(px(1)),
        ..default()
    }
}

/// RANDOM's die: a rounded square with the five pips of a five.
fn die(commands: &mut Commands, shot: pregame::Shot, button: Entity) {
    let face = commands
        .spawn((
            Node {
                width: shot.val(44.0),
                height: shot.val(44.0),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::all(shot.val(9.0)),
                ..default()
            },
            BorderColor::all(theme::INK_3),
            Pickable::IGNORE,
            ChildOf(button),
        ))
        .id();
    for (x, y) in [(0.2, 0.2), (0.7, 0.2), (0.45, 0.45), (0.2, 0.7), (0.7, 0.7)] {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(x * 100.0),
                top: percent(y * 100.0),
                width: shot.val(7.0),
                height: shot.val(7.0),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(theme::INK_3),
            Pickable::IGNORE,
            ChildOf(face),
        ));
    }
}

/// The screen while the match has not begun and the tactical phase has not
/// opened the shop.
fn show(session: NonSend<Session>, mut screens: Query<&mut Visibility, With<Screen>>) {
    let on = session.0.phase == Phase::Lobby
        && session.0.view().is_none()
        && !session.0.in_tactical_phase();
    for mut screen in &mut screens {
        screen.set_if_neq(ui::visible(on));
    }
}

/// Remembers when the clock was last told, to count down from there.
fn note_clock(session: NonSend<Session>, time: Res<Time>, mut seen: ResMut<ClockSeen>) {
    if seen.phase != session.0.lobby.pick {
        seen.phase = session.0.lobby.pick;
        seen.at = time.elapsed_secs();
    }
}

/// Whether the tactical phase is on, for the systems that cannot read the
/// session. Set in the frame's `PreUpdate` right after the session is pumped, so it is never a
/// frame behind what the pump did (the first queued buy goes out in the frame the match begins).
fn tactical(session: NonSend<Session>, mut phase: ResMut<TacticalPhase>) {
    phase.set_if_neq(TacticalPhase(session.0.in_tactical_phase()));
}

/// Whether this client may choose, lock in or draw a hero now: it holds a seat, has not locked in
/// and the server offers more than one hero.
fn may_pick(core: &bota_client_core::App) -> bool {
    core.phase == Phase::Lobby
        && core.my_slot.is_some()
        && !locked(core)
        && core.lobby.heroes.len() != 1
}

/// Whether the seat has locked its pick in.
fn locked(core: &bota_client_core::App) -> bool {
    let slot = core.my_slot;
    core.ready
        || core
            .lobby
            .slots
            .iter()
            .any(|s| Some(s.slot) == slot && s.ready && s.hero.is_some())
}

/// A click on a card chooses that hero for the panel; on LOCK IN sends the pick and the readiness;
/// on RANDOM draws a hero among those that may be picked and locks it in.
#[allow(clippy::type_complexity)]
fn clicks(
    event: On<PointerPress>,
    mut session: NonSendMut<Session>,
    mut choice: ResMut<Choice>,
    mut die: ResMut<Die>,
    cards: Query<&Card>,
    lock: Query<(), With<LockIn>>,
    random: Query<(), With<RandomButton>>,
) {
    let core = &mut session.0;
    if event.button != PointerButton::Primary || !may_pick(core) {
        return;
    }
    if let Ok(card) = cards.get(event.entity)
        && free(core, card.0)
    {
        choice.chosen = Some(card.0);
    }
    if lock.contains(event.entity)
        && let Some(hero) = choice.chosen.filter(|hero| free(core, *hero))
    {
        lock_in(core, hero);
    }
    if random.contains(event.entity) {
        let candidates: Vec<u16> = pool(core).into_iter().filter(|h| free(core, *h)).collect();
        if !candidates.is_empty() {
            let hero = candidates[(die.roll() % candidates.len() as u64) as usize];
            choice.chosen = Some(hero);
            lock_in(core, hero);
        }
    }
}

/// Sends the pick, then the readiness that makes it final.
fn lock_in(core: &mut bota_client_core::App, hero: u16) {
    core.act(Intent::PickHero(hero));
    if !core.ready {
        core.act(Intent::ToggleReady);
    }
}

/// Whether the server's pool has this hero (every hero when it names none).
fn offered(core: &bota_client_core::App, hero: u16) -> bool {
    core.lobby.heroes.is_empty() || core.lobby.heroes.iter().any(|h| h.0 == hero)
}

/// Whether this seat may take the hero: offered, and not another seat's while the rules forbid
/// two of one.
fn free(core: &bota_client_core::App, hero: u16) -> bool {
    offered(core, hero) && !taken(core, hero)
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

/// The cards' frames, the buttons' states and which hero the panel is to show.
#[allow(clippy::type_complexity)]
fn looks(
    session: NonSend<Session>,
    mut choice: ResMut<Choice>,
    mut cards: Query<(&Card, &Hovered, &mut BorderColor)>,
    mut veils: Query<(&Veil, &mut BackgroundColor), (Without<LockIn>, Without<RandomButton>)>,
    mut lock: Query<(&Hovered, &mut BackgroundColor), (With<LockIn>, Without<RandomButton>)>,
    mut random: Query<(&Hovered, &mut BackgroundColor), (With<RandomButton>, Without<LockIn>)>,
    mut lock_name: Query<&mut Text, With<LockInName>>,
) {
    let core = &session.0;
    if core.phase != Phase::Lobby {
        // The next selection starts with nothing chosen.
        choice.chosen = None;
        return;
    }
    let may = may_pick(core);
    if locked(core) {
        choice.chosen = my_pick(core);
    } else if core.lobby.heroes.len() == 1 {
        choice.chosen = core.lobby.heroes.first().map(|h| h.0);
    } else if choice.chosen.is_some_and(|hero| !free(core, hero)) {
        choice.chosen = None;
    }
    let mut hovered_hero = None;
    for (card, hovered, mut edge) in &mut cards {
        if hovered.get() && may && free(core, card.0) {
            hovered_hero = Some(card.0);
        }
        let ring = if choice.chosen == Some(card.0) {
            CHOSEN
        } else {
            CARD_EDGE
        };
        edge.set_if_neq(BorderColor::all(ring));
    }
    for (veil, mut ground) in &mut veils {
        ground.set_if_neq(BackgroundColor(if free(core, veil.0) {
            Color::NONE
        } else {
            DEAD
        }));
    }
    choice.shown = hovered_hero
        .or(choice.chosen)
        .or_else(|| pool(core).first().copied());
    let button = |on: bool, hovered: &Hovered| match (on, hovered.get()) {
        (false, _) => BUTTON_OFF,
        (true, true) => BUTTON_LIT,
        (true, false) => BUTTON,
    };
    for (hovered, mut ground) in &mut lock {
        ground.set_if_neq(BackgroundColor(button(
            may && choice.chosen.is_some(),
            hovered,
        )));
    }
    for (hovered, mut ground) in &mut random {
        ground.set_if_neq(BackgroundColor(button(may, hovered)));
    }
    let name = choice
        .chosen
        .and_then(|hero| bota_core::hero_def(bota_proto::HeroId(hero)))
        .map_or(String::new(), |hero| hero.name.to_uppercase());
    for mut text in &mut lock_name {
        text.set_if_neq(Text(name.clone()));
    }
}

/// Puts the hero the panel is to show in it, once it changes.
fn panel_content(
    mut commands: Commands,
    art: (Res<Fonts>, Res<Manifest>, Res<AssetServer>, Res<Texts>),
    choice: Res<Choice>,
    panels: Query<Entity, With<panel::HeroPanel>>,
    mut shown: Local<Option<(Entity, Option<u16>)>>,
) {
    let Ok(panel_entity) = panels.single() else {
        *shown = None;
        return;
    };
    if *shown == Some((panel_entity, choice.shown)) {
        return;
    }
    *shown = Some((panel_entity, choice.shown));
    commands.entity(panel_entity).despawn_related::<Children>();
    let Some(hero) = choice
        .shown
        .and_then(|h| bota_core::hero_def(bota_proto::HeroId(h)))
    else {
        return;
    };
    let kit = Kit {
        fonts: &art.0,
        manifest: &art.1,
        server: &art.2,
        texts: &art.3,
    };
    panel::fill(&mut commands, &kit, panel_entity, hero);
}

/// The words: the heading, the clock and the line under the heading.
#[allow(clippy::type_complexity)]
fn words(
    session: NonSend<Session>,
    menu: Option<NonSend<crate::main_menu::MenuCore>>,
    seen: Res<ClockSeen>,
    time: Res<Time>,
    mut texts: ParamSet<(
        Query<&mut Text, With<ClockText>>,
        Query<&mut Text, With<ClockCaption>>,
        Query<&mut Text, With<Heading>>,
        Query<(&mut Text, &mut TextColor), With<Status>>,
    )>,
) {
    let core = &session.0;
    if core.phase != Phase::Lobby {
        return;
    }
    let playing = core.my_slot.is_some();
    let forced = core.lobby.heroes.len() == 1;
    let left = seen.left(time.elapsed_secs()).map(clock);
    let (caption, status, warn) = status_words(core, seen.phase, left.as_deref(), menu.as_deref());
    for mut text in &mut texts.p0() {
        text.set_if_neq(Text(left.clone().unwrap_or_default()));
    }
    for mut text in &mut texts.p1() {
        text.set_if_neq(Text(caption.to_string()));
    }
    let heading = if playing && forced {
        "YOUR HERO"
    } else {
        "CHOOSE YOUR HERO"
    };
    for mut text in &mut texts.p2() {
        text.set_if_neq(Text(heading.to_string()));
    }
    for (mut text, mut colour) in &mut texts.p3() {
        text.set_if_neq(Text(status.clone()));
        colour.set_if_neq(TextColor(if warn { WARNING } else { theme::INK_4 }));
    }
}

/// The line under the clock, and the line under the heading with whether it warns.
fn status_words(
    core: &bota_client_core::App,
    phase: PickPhase,
    left: Option<&str>,
    menu: Option<&crate::main_menu::MenuCore>,
) -> (&'static str, String, bool) {
    let caption = match phase {
        PickPhase::Waiting => "WAITING FOR PLAYERS",
        PickPhase::Late { .. } => "PICK NOW",
        PickPhase::Selecting { .. } | PickPhase::Tactical { .. } => "",
    };
    let mut status = if core.my_slot.is_none() {
        if core.lobby.me.is_some() {
            "Spectating: waiting for the match".to_string()
        } else {
            "Connecting...".to_string()
        }
    } else if matches!(phase, PickPhase::Late { .. }) {
        format!(
            "Losing 2 gold every second: a hero is given in {}",
            left.unwrap_or("0:00")
        )
    } else if locked(core) {
        let others = core
            .lobby
            .slots
            .iter()
            .filter(|s| !s.ready && s.role != Some(Role::Bot) && Some(s.slot) != core.my_slot)
            .count();
        if others == 0 {
            "Locked in".to_string()
        } else {
            "Locked in: waiting for the others".to_string()
        }
    } else {
        String::new()
    };
    // A local lobby this client hosts: where the others join while a seat is open.
    let open = core.lobby.slots.iter().any(|seat| seat.name.is_empty());
    if let Some(at) = menu.and_then(|m| m.hosted_at())
        && open
    {
        status = format!("Others join by address at {at}");
    }
    (caption, status, matches!(phase, PickPhase::Late { .. }))
}
