//! The kill feed in the top-right corner: the client's lines, and the
//! game's announcements in Dota's words with Dota's icons where the cache has
//! them (an Outpost captured, a Shrine of Wisdom gathered, a rune taken,
//! bottled or destroyed); without them, the client's own text.

use bevy::prelude::*;
use bota_client_core::{App as Core, FeedKind, FeedLine, Killer, RuneAct, Victim};
use bota_proto::{RuneKind, Team, UnitKind, Vec2};

use crate::dota_text::{Part, Texts};
use crate::manifest::Manifest;
use crate::seat_name::seat_name;
use crate::ui::{self, Fonts};

/// Lines shown at most: what the client keeps.
const ROWS: usize = 8;
/// Icons and texts a line holds at most, alternating, an icon first: a kill's killer, kill icon
/// and victim, and up to four assists.
const SLOTS: usize = 7;
// The styles below are Dota's, copied from two of its stylesheets in the install (the manifest's
// `style` lines read only `@define` colours, not rule values like these, nor gradients), in its
// pixels, which `dota` scales:
//   ROW  = panorama/styles/hud/dota_hud_combat_event_row.vcss_c
//   LIST = panorama/styles/hud/dota_hud_combat_events.vcss_c
/// A colour as the stylesheets write it, `#rrggbbaa`.
const fn hex(r: u8, g: u8, b: u8, a: u8) -> Color {
    Color::srgba(
        r as f32 / 255.0,
        g as f32 / 255.0,
        b as f32 / 255.0,
        a as f32 / 255.0,
    )
}

/// ROW `DOTACombatEventRow`: the height of a row and the gap above it (`margin-top`).
const ROW_HEIGHT: f32 = 30.0;
const ROW_GAP: f32 = 5.0;
/// ROW `.EventListLabel`: the size of a row's text (`font-size: 18px; font-weight: bold`, white,
/// shadowed).
pub(crate) const LABEL_SIZE: f32 = 18.0;
/// ROW `.CombatEventHeroIcon`: the width of a hero icon, which the other icons of a line share here.
const ICON: f32 = 24.0;
/// ROW `#SlashContainer` (`width`), and `#TeamColorBar` (`width`, `height`, `margin-left`,
/// `margin-top`, `transform: rotateZ(25deg)`).
const SLASH_WIDTH: f32 = 18.0;
const BAR: (f32, f32) = (12.0, 44.0);
const BAR_LEFT: f32 = 7.0;
const BAR_TOP: f32 = -6.0;
pub(crate) const BAR_TURN_DEGREES: f32 = 25.0;
/// ROW `.AllyEvent .EventListLabel` and `.EnemyEvent .EventListLabel`: the shade behind every row
/// (`from( #00000088 )`) and the tints of the side it is about (`to( #2E4826cc )`, `to( #5B3525cc
/// )`).
pub(crate) const SHADE: Color = hex(0x00, 0x00, 0x00, 0x88);
pub(crate) const ALLY_TINT: Color = hex(0x2E, 0x48, 0x26, 0xcc);
pub(crate) const ENEMY_TINT: Color = hex(0x5B, 0x35, 0x25, 0xcc);
/// ROW `.AllyEvent #TeamColorBar` and `.EnemyEvent #TeamColorBar`: the ends of the bar's
/// gradient, top to bottom.
pub(crate) const ALLY_BAR: [Color; 2] = [hex(0x62, 0x9f, 0x52, 0xff), hex(0x43, 0x6e, 0x38, 0xff)];
pub(crate) const ENEMY_BAR: [Color; 2] = [hex(0xd1, 0x47, 0x1f, 0xff), hex(0xa4, 0x38, 0x19, 0xff)];
/// LIST `#ToastManager > .ToastPanel`: the seconds of a row's entrance (`animation-duration: .41s`,
/// `@keyframes 'CombatEvent'`, which ROW also holds: from a fifth of its size at 0 to one and a half
/// at 50 % and one at 100 %, fading in over the first half); of the slide and fade of its coming
/// and going (`transition-duration: toastExitDuration`, `@define toastExitDuration: 0.2s`); and the
/// way it slides in from (`transform: translateX( -40px )`, mirrored with `.HUDFlipped`).
pub(crate) const ENTER_SECONDS: f32 = 0.41;
const SLIDE_SECONDS: f32 = 0.2;
pub(crate) const SLIDE: f32 = 40.0;
/// How long the client keeps a line: Dota's own script, which holds that, is not in the install's
/// styles.
const LINE_SECONDS: f32 = bota_client_core::FEED_LINE_SECONDS;

/// One line of the feed, from the top.
#[derive(Component)]
struct Row(usize, Place);

/// Whose side a line is good for, as Dota tints it: its own (`.AllyEvent`) or the other's
/// (`.EnemyEvent`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Ally,
    Enemy,
}

impl Side {
    fn tint(self) -> Color {
        match self {
            Side::Ally => ALLY_TINT,
            Side::Enemy => ENEMY_TINT,
        }
    }

    fn bar(self) -> [Color; 2] {
        match self {
            Side::Ally => ALLY_BAR,
            Side::Enemy => ENEMY_BAR,
        }
    }
}

/// How a row is dressed now, to dress it anew only when it changes: its side and its opacity.
#[derive(Component, Default, PartialEq)]
struct Dress {
    side: Option<Side>,
    opacity: f32,
}

/// The slanted end of a row.
#[derive(Component)]
struct Slash;

/// The slanted bar of a row's side.
#[derive(Component)]
struct Bar;

/// Where a line is drawn: the kill feed at the top right (a hero's fall, the connection), or the
/// toasts at the left above the HUD, over the chat (a building's fall, a rune, a Shrine, an
/// Outpost), as Dota places them.
#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
enum Place {
    Corner,
    Left,
}

impl Place {
    /// Where a line of this kind belongs.
    fn of(kind: &FeedKind) -> Place {
        match kind {
            FeedKind::StructureFell { .. }
            | FeedKind::Rune { .. }
            | FeedKind::ShrineActivated { .. }
            | FeedKind::Captured { .. } => Place::Left,
            _ => Place::Corner,
        }
    }
}

/// A piece of a line: its `n`th icon or text.
#[derive(Component, Clone, Copy)]
enum Slot {
    Icon(usize),
    Text(usize),
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, show);
}

/// The feed's corner column and the lines made in it so far, and the rows made in the toast
/// column the chat holds.
#[derive(Resource)]
struct Column {
    entity: Entity,
    rows: usize,
    toast_rows: usize,
}

/// The feed's column under the frame rate, lines right-aligned. Its lines
/// are made as the feed fills, so a match with none carries none.
pub fn spawn(commands: &mut Commands, root: Entity) {
    let entity = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(10),
                top: px(30),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    commands.insert_resource(Column {
        entity,
        rows: 0,
        toast_rows: 0,
    });
}

/// The next line of the column: [`SLOTS`] pairs of an icon and a text, with the slanted end and
/// bar a line about one side wears.
fn spawn_row(commands: &mut Commands, fonts: &Fonts, column: Entity, row: usize, place: Place) {
    let line = commands
        .spawn((
            Row(row, place),
            Dress::default(),
            Node {
                display: Display::None,
                align_items: AlignItems::Center,
                height: ui::dota(ROW_HEIGHT),
                margin: UiRect::top(ui::dota(ROW_GAP)),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id();
    // The slanted end is at the side the row is read from: the left of the toasts, the right of
    // the feed (Dota's flipped HUD).
    let slash = |commands: &mut Commands| {
        commands.spawn((
            Slash,
            Node {
                width: ui::dota(SLASH_WIDTH),
                height: Val::Percent(100.0),
                display: Display::None,
                ..default()
            },
            BackgroundColor(SHADE),
            Pickable::IGNORE,
            ChildOf(line),
        ));
    };
    if place == Place::Left {
        slash(commands);
        commands.spawn((
            Bar,
            Node {
                position_type: PositionType::Absolute,
                left: ui::dota(BAR_LEFT),
                top: ui::dota(BAR_TOP),
                width: ui::dota(BAR.0),
                height: ui::dota(BAR.1),
                display: Display::None,
                ..default()
            },
            UiTransform::from_rotation(Rot2::degrees(BAR_TURN_DEGREES)),
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(Color::WHITE, percent(0)),
                ColorStop::new(Color::WHITE, percent(100)),
            ])),
            Pickable::IGNORE,
            ChildOf(line),
        ));
    }
    for n in 0..SLOTS {
        commands.spawn((
            Slot::Icon(n),
            Node {
                width: ui::dota(ICON),
                height: ui::dota(ICON),
                margin: UiRect::right(ui::dota(4.0)),
                display: Display::None,
                ..default()
            },
            ImageNode::default(),
            Pickable::IGNORE,
            ChildOf(line),
        ));
        commands.spawn((
            Slot::Text(n),
            ui::text("", &fonts.bold, LABEL_SIZE * ui::DOTA, Color::WHITE),
            ui::shadow(),
            Node {
                padding: UiRect::axes(ui::dota(4.0), px(0)),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(line),
        ));
    }
    if place == Place::Corner {
        slash(commands);
    }
}

/// A line laid out: up to [`SLOTS`] pairs of an icon (a cache file) and a
/// text.
type Laid = [(Option<String>, String); SLOTS];

/// The scale a row has `age` seconds after it came (`@keyframes CombatEvent`): from a fifth to
/// one and a half in the first half of its entrance, then down to one.
fn entrance_scale(age: f32) -> f32 {
    let at = (age / ENTER_SECONDS).clamp(0.0, 1.0);
    if at < 0.5 {
        0.2 + (1.5 - 0.2) * (at / 0.5)
    } else {
        1.5 - 0.5 * ((at - 0.5) / 0.5)
    }
}

/// How opaque a row is `age` seconds after it came: fading in with the entrance, out in the
/// seconds before the client drops the line.
fn opacity_at(age: f32) -> f32 {
    let entering = (age / (ENTER_SECONDS / 2.0)).clamp(0.0, 1.0);
    let leaving = ((LINE_SECONDS - age) / SLIDE_SECONDS).clamp(0.0, 1.0);
    entering.min(leaving)
}

/// How far from its place a row is `age` seconds after it came, from 1 (the whole of [`SLIDE`])
/// to 0, and again as it goes.
fn displacement_at(age: f32) -> f32 {
    let coming = 1.0 - (age / SLIDE_SECONDS).clamp(0.0, 1.0);
    let going = 1.0 - ((LINE_SECONDS - age) / SLIDE_SECONDS).clamp(0.0, 1.0);
    coming.max(going)
}

fn faded(color: Color, opacity: f32) -> Color {
    color.with_alpha(color.alpha() * opacity)
}

/// Whose side a line is good for, from the viewer's: a kill by its own side, an enemy structure
/// fallen, a building captured by its own; none for the rest, which Dota leaves undyed. A viewer
/// of no side is taken for the Radiant, as the minimap and the scoreboard take it.
fn side_of(core: &Core, kind: &FeedKind) -> Option<Side> {
    let mine = core.my_team().unwrap_or(Team::Radiant);
    let team_of = |slot: bota_proto::SlotId| core.view()?.player(slot).map(|p| p.team);
    let good_for = |team: Team| {
        if team == mine {
            Side::Ally
        } else {
            Side::Enemy
        }
    };
    match *kind {
        FeedKind::HeroDied { victim, killer, .. } => match (killer, victim) {
            (Some(Killer::Seat(slot)), _) => team_of(slot).map(good_for),
            (Some(Killer::Unit { team, .. }), _) => Some(good_for(team)),
            (None, Victim::Seat(slot)) => team_of(slot).map(|team| good_for(team).flip()),
            (None, Victim::Hero(team)) => Some(good_for(team).flip()),
        },
        FeedKind::StructureFell { team, .. } => Some(good_for(team).flip()),
        FeedKind::Captured { by, .. } => Some(good_for(by)),
        _ => None,
    }
}

impl Side {
    fn flip(self) -> Side {
        match self {
            Side::Ally => Side::Enemy,
            Side::Enemy => Side::Ally,
        }
    }
}

/// The parts of a row besides the row itself, which `show` dresses.
#[derive(bevy::ecs::system::SystemParam)]
struct Parts<'w, 's> {
    slots: Query<
        'w,
        's,
        (
            &'static Slot,
            Option<&'static mut ImageNode>,
            Option<&'static mut Text>,
            Option<&'static mut TextColor>,
            &'static mut Node,
        ),
        (Without<Row>, Without<Slash>, Without<Bar>),
    >,
    slashes: Query<
        'w,
        's,
        (&'static mut Node, &'static mut BackgroundColor),
        (With<Slash>, Without<Row>, Without<Slot>, Without<Bar>),
    >,
    bars: Query<
        'w,
        's,
        (&'static mut Node, &'static mut BackgroundGradient),
        (With<Bar>, Without<Row>, Without<Slot>, Without<Slash>),
    >,
}

fn show(
    mut commands: Commands,
    column: Option<ResMut<Column>>,
    toasts: Option<Single<Entity, With<crate::chat::ToastColumn>>>,
    fonts: Res<Fonts>,
    sight: crate::fog::EventSight,
    texts: Option<Res<Texts>>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut rows: Query<(
        Entity,
        &Row,
        &Children,
        &mut Node,
        &mut UiTransform,
        &mut Dress,
    )>,
    mut parts: Parts,
) {
    let core = sight.core();
    let all = visible_lines(core, &sight);
    let of = |place| -> Vec<&FeedLine> {
        all.iter()
            .copied()
            .filter(|line| Place::of(&line.kind) == place)
            .collect()
    };
    let (corner, left) = (of(Place::Corner), of(Place::Left));
    let Some(mut column) = column else {
        return;
    };
    while column.rows < corner.len() {
        spawn_row(
            &mut commands,
            &fonts,
            column.entity,
            column.rows,
            Place::Corner,
        );
        column.rows += 1;
    }
    if let Some(toasts) = toasts {
        while column.toast_rows < left.len() {
            spawn_row(
                &mut commands,
                &fonts,
                *toasts,
                column.toast_rows,
                Place::Left,
            );
            column.toast_rows += 1;
        }
    }
    for (entity, Row(row, place), children, mut node, mut transform, mut dress) in &mut rows {
        let line = match place {
            Place::Corner => corner.get(*row),
            Place::Left => left.get(*row),
        };
        let display = if line.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let Some(line) = line else {
            continue;
        };
        let scale = bevy::math::Vec2::splat(entrance_scale(line.age));
        if transform.scale != scale {
            transform.scale = scale;
        }
        // The toasts at the left come in from the left, the feed at the right from the right.
        let from = match place {
            Place::Left => -SLIDE,
            Place::Corner => SLIDE,
        };
        let slid = Val2::px(from * displacement_at(line.age) * ui::DOTA, 0.0);
        if transform.translation != slid {
            transform.translation = slid;
        }
        let wanted = Dress {
            side: side_of(core, &line.kind),
            opacity: opacity_at(line.age),
        };
        let dressed = *dress != wanted;
        if dressed {
            dress_row(&mut commands, entity, *place, &wanted);
        }
        let laid = lay(core, texts.as_deref(), &manifest, line);
        for child in children {
            if let Ok((mut node, mut background)) = parts.slashes.get_mut(*child) {
                if dressed {
                    let display = if wanted.side.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    };
                    node.display = display;
                    let colour = match (place, wanted.side) {
                        (Place::Corner, Some(side)) => side.tint(),
                        _ => SHADE,
                    };
                    background.0 = faded(colour, wanted.opacity);
                }
                continue;
            }
            if let Ok((mut node, mut gradient)) = parts.bars.get_mut(*child) {
                if dressed {
                    node.display = if wanted.side.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    };
                    if let Some(side) = wanted.side {
                        let [top, bottom] = side.bar().map(|c| faded(c, wanted.opacity));
                        *gradient = BackgroundGradient::from(LinearGradient::to_bottom(vec![
                            ColorStop::new(top, percent(0)),
                            ColorStop::new(bottom, percent(100)),
                        ]));
                    }
                }
                continue;
            }
            let Ok((slot, image, text, colour, mut node)) = parts.slots.get_mut(*child) else {
                continue;
            };
            match (*slot, image, text) {
                (Slot::Icon(n), Some(mut image), _) => {
                    let path = laid[n].0.as_deref();
                    let display = if path.is_some() {
                        Display::Flex
                    } else {
                        Display::None
                    };
                    if node.display != display {
                        node.display = display;
                    }
                    if let Some(path) = path {
                        let handle: Handle<Image> = server.load(path.to_owned());
                        if image.image != handle {
                            image.image = handle;
                        }
                    }
                    if dressed {
                        image.color = faded(Color::WHITE, wanted.opacity);
                    }
                }
                (Slot::Text(n), _, Some(mut text)) => {
                    text.set_if_neq(Text(laid[n].1.clone()));
                    if let (true, Some(mut colour)) = (dressed, colour) {
                        colour.0 = faded(Color::WHITE, wanted.opacity);
                    }
                }
                _ => {}
            }
        }
        *dress = wanted;
    }
}

/// Gives a row the tint of its side behind it, or none: Dota's label gradient runs from the side's
/// colour at the end of the row that has the slanted end to the shade at the other.
fn dress_row(commands: &mut Commands, row: Entity, place: Place, dress: &Dress) {
    let Some(side) = dress.side else {
        commands.entity(row).remove::<BackgroundGradient>();
        return;
    };
    let tint = faded(side.tint(), dress.opacity);
    let shade = faded(SHADE, dress.opacity);
    let (from, to) = match place {
        Place::Left => (tint, shade),
        Place::Corner => (shade, tint),
    };
    commands
        .entity(row)
        .insert(BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(from, percent(0)),
            ColorStop::new(to, percent(100)),
        ])));
}

fn visible_lines<'a>(core: &'a Core, sight: &crate::fog::EventSight) -> Vec<&'a FeedLine> {
    let mut lines: Vec<&FeedLine> = core
        .feed
        .iter()
        .rev()
        .filter(|line| {
            line.told
                .as_ref()
                .is_none_or(|event| sight.allows(event, crate::fog::EventSubject::Event))
        })
        .take(ROWS)
        .collect();
    lines.reverse();
    lines
}

/// A line as Dota words it, or the client's text.
fn lay(core: &Core, texts: Option<&Texts>, manifest: &Manifest, line: &FeedLine) -> Laid {
    if let Some(laid) = hero_kill(core, manifest, &line.kind) {
        return laid;
    }
    let mut laid: Laid = Default::default();
    let dota = texts.and_then(|texts| dota_parts(core, texts, &line.kind));
    let Some(parts) = dota else {
        laid[0].1 = text(core, &line.kind);
        return laid;
    };
    // Icons and texts alternate, an icon first; a text with no icon before
    // it takes its pair's text, a second icon in a row the next pair's.
    let mut at = 0;
    let mut texted = false;
    for part in parts {
        match part {
            Part::Icon(name) => {
                if texted || laid[at].0.is_some() {
                    at += 1;
                    texted = false;
                }
                if at == SLOTS {
                    break;
                }
                laid[at].0 = icon_path(core, manifest, &name, &line.kind);
            }
            Part::Text(text) => {
                laid[at].1.push_str(&text);
                texted = true;
            }
        }
    }
    laid
}

/// A hero's fall as Dota's feed draws it: the killer's hero icon, the kill icon (the ally's when
/// one's own side struck it, the enemy's when the other did) and the fallen hero's icon, then the
/// icons of the heroes that assisted. None when a hero in it is not known by its seat (a tower's
/// or a creep's kill, a hero no seat drives), which the text tells.
fn hero_kill(core: &Core, manifest: &Manifest, kind: &FeedKind) -> Option<Laid> {
    let FeedKind::HeroDied {
        victim,
        killer,
        assists,
        ..
    } = *kind
    else {
        return None;
    };
    let (Victim::Seat(fallen), Some(Killer::Seat(striker))) = (victim, killer) else {
        return None;
    };
    let hero_icon = |slot: bota_proto::SlotId| {
        let hero = core.view()?.player(slot)?.hero;
        manifest.hero_icon(hero.0)
    };
    let team_of = |slot: bota_proto::SlotId| core.view()?.player(slot).map(|p| p.team);
    let swords = match (core.my_team(), team_of(striker)) {
        (Some(mine), Some(theirs)) if mine == theirs => "hud.kill_ally",
        (Some(_), Some(_)) => "hud.kill_enemy",
        _ => "hud.kill",
    };
    let mut laid: Laid = Default::default();
    laid[0].0 = Some(hero_icon(striker)?);
    laid[1].0 = manifest.file(swords);
    laid[2].0 = Some(hero_icon(fallen)?);
    let helpers = bota_proto::assisting_seats(assists).filter_map(hero_icon);
    for (pair, icon) in laid[3..].iter_mut().zip(helpers) {
        pair.0 = Some(icon);
    }
    Some(laid)
}

/// What a tower or a barracks that fell was called, in Dota's words: its side, its lane and its
/// tier, or whether the barracks is the melee or the ranged one.
fn building_parts(texts: &Texts, kind: &FeedKind) -> Option<Vec<Part>> {
    let FeedKind::StructureFell {
        team,
        kind: building,
        lane,
        tier,
        ranged,
        killer,
    } = *kind
    else {
        return None;
    };
    let word = |key: &str| texts.label(key, &[]);
    let side = word(match team {
        Team::Radiant => "DOTA_Location_Name_GoodGuys",
        _ => "DOTA_Location_Name_BadGuys",
    })?;
    let lane = word(match lane? {
        0 => "DOTA_Location_Name_MidLane",
        1 => "DOTA_Location_Name_TopLane",
        _ => "DOTA_Location_Name_BotLane",
    })?;
    let parts = match building {
        UnitKind::Tower => {
            let tier = word(&format!("DOTA_Location_Name_Tower{}", tier?))?;
            texts.label_parts(
                "DOTA_HUD_Tower_Killed",
                &[
                    ("killer", String::new()),
                    ("tower_name", format!("{side} {lane} {tier}")),
                    ("gold", String::new()),
                ],
            )?
        }
        UnitKind::Barracks => {
            let rax = word(if ranged? {
                "DOTA_Chat_Ranged"
            } else {
                "DOTA_Chat_Melee"
            })?;
            texts.label_parts(
                "DOTA_HUD_BarracksKilled",
                &[
                    ("killer", String::new()),
                    ("team", side),
                    ("lane", lane),
                    ("rax_type", rax),
                    ("gold", String::new()),
                ],
            )?
        }
        _ => return None,
    };
    // The hero that struck it leads, its icon before the sword.
    let killer =
        matches!(killer, Some(Killer::Seat(_))).then(|| Part::Icon("killer_icon".to_owned()));
    Some(killer.into_iter().chain(parts).collect())
}

/// The parts of Dota's feed line for an event the client words with the
/// install's labels; none for any other, or without the labels.
fn dota_parts(core: &Core, texts: &Texts, kind: &FeedKind) -> Option<Vec<Part>> {
    match *kind {
        FeedKind::StructureFell { .. } => building_parts(texts, kind),
        FeedKind::ItemPlaced { item, team } => {
            let keys = crate::faces::placed_labels(item)?;
            texts.label_parts(keys[usize::from(team == Team::Dire)], &[])
        }
        FeedKind::Loot {
            item,
            by,
            team,
            destroyed,
        } => {
            let labels = crate::faces::loot_labels(item)?;
            texts.label_parts(
                labels[usize::from(destroyed)],
                &[("player_name", activator(core, by, team))],
            )
        }
        FeedKind::BossRoared {
            objective: bota_proto::ObjectiveKind::Roshan,
            team,
        } => texts.label_parts(
            if team == Team::Radiant {
                "DOTA_HUD_RadiantRoshanRoar"
            } else {
                "DOTA_HUD_DireRoshanRoar"
            },
            &[],
        ),
        FeedKind::BossFell {
            objective: bota_proto::ObjectiveKind::Roshan,
            team: Some(team),
        } => texts.label_parts(
            if team == Team::Radiant {
                "DOTA_HUD_KilledRoshanGood"
            } else {
                "DOTA_HUD_KilledRoshanBad"
            },
            &[("gold", String::new())],
        ),
        FeedKind::Captured { building, by } => {
            let name = outpost_name(core, Some(texts), building);
            let (key, values) = match core.my_team() {
                Some(mine) if mine == by => {
                    ("DOTA_HUD_OutpostCaptured", vec![("outpost_name", name)])
                }
                Some(_) => ("DOTA_HUD_OutpostLost", vec![("outpost_name", name)]),
                None => (
                    "DOTA_HUD_OutpostCaptured_Spectator",
                    vec![
                        ("outpost_name", name),
                        ("team_name", side_name(by).to_owned()),
                    ],
                ),
            };
            texts.label_parts(key, &values)
        }
        FeedKind::ShrineActivated { by, team, xp } => {
            let rune = texts.label("DOTA_HUD_Rune_XP", &[("xp", xp.to_string())])?;
            let who = activator(core, by, team);
            texts.label_parts(
                "DOTA_HUD_ActivatedRune",
                &[
                    ("player_name", format!("{who} ")),
                    ("rune_name", format!(" {rune}")),
                ],
            )
        }
        FeedKind::Rune {
            by,
            team,
            kind,
            act,
        } => {
            let who = format!("{} ", activator(core, by, team));
            let name = crate::runes::text_name(kind);
            let (key, rune) = match act {
                RuneAct::Activated { gold } => {
                    let gold = gold.map_or_else(String::new, |g| g.to_string());
                    let rune = texts.label(&format!("DOTA_HUD_Rune_{name}"), &[("gold", gold)])?;
                    ("DOTA_HUD_ActivatedRune", rune)
                }
                RuneAct::Bottled => (
                    "DOTA_HUD_BottledRune",
                    texts.label(&format!("DOTA_HUD_Rune_{name}"), &[("gold", String::new())])?,
                ),
                RuneAct::Denied => (
                    "DOTA_HUD_DenyRune",
                    texts.label(&format!("DOTA_HUD_Rune_Deny{name}"), &[])?,
                ),
            };
            texts.label_parts(
                key,
                &[
                    ("player_name", who),
                    ("rune_name", format!(" {}", rune.trim())),
                ],
            )
        }
        _ => None,
    }
}

/// The seat that gathered a Shrine, else its side.
fn activator(core: &Core, by: Option<bota_proto::SlotId>, team: Team) -> String {
    by.map_or_else(|| side_name(team).to_owned(), |slot| seat_name(core, slot))
}

/// What an Outpost is called: Dota's name for the one on the Radiant or the
/// Dire half where the install has it, else dbota's own; the home side is
/// found by the place the map puts an Outpost at.
fn outpost_name(
    core: &Core,
    texts: Option<&Texts>,
    building: Option<(UnitKind, Team, Vec2)>,
) -> String {
    let home = building.and_then(|(_, _, pos)| {
        core.session()?
            .map()
            .outposts
            .iter()
            .find(|outpost| outpost.at == pos)
            .map(|outpost| outpost.home)
    });
    let (key, own) = match home {
        Some(Team::Radiant) => ("DOTA_OutpostName_South", "Southwest Outpost"),
        Some(Team::Dire) => ("DOTA_OutpostName_North", "Northeast Outpost"),
        _ => ("DOTA_OutpostName_Default", "Outpost"),
    };
    texts
        .and_then(|texts| texts.label(key, &[]))
        .unwrap_or_else(|| own.to_owned())
}

/// The line of a feed entry in the client's own words, for a game without
/// the install's labels.
pub fn text(core: &Core, kind: &FeedKind) -> String {
    match *kind {
        FeedKind::ItemPlaced { item, team } => format!(
            "{} placed {}",
            side_name(team),
            bota_core::item_def(item).map_or("an item", |def| def.name)
        ),
        FeedKind::Loot {
            item,
            by,
            team,
            destroyed,
        } => format!(
            "{} {} {}",
            activator(core, by, team),
            if destroyed { "denied" } else { "picked up" },
            bota_core::item_def(item).map_or("the item", |def| def.name)
        ),
        FeedKind::BossRoared { objective: _, team } => {
            format!("Roshan roars at {}", side_name(team))
        }
        FeedKind::BossFell { objective, team } => {
            let name = match objective {
                bota_proto::ObjectiveKind::Roshan => "Roshan",
                bota_proto::ObjectiveKind::Tormentor => "Tormentor",
                _ => "The objective",
            };
            match team {
                Some(team) => format!("{name} was slain by {}", side_name(team)),
                None => format!("{name} has fallen"),
            }
        }
        FeedKind::HeroDied {
            victim,
            killer,
            gold,
            assists,
        } => death(core, victim, killer, gold, assists),
        FeedKind::StructureFell {
            team,
            kind,
            lane,
            tier,
            ranged,
            killer: _,
        } => structure(team, kind, lane, tier, ranged),
        FeedKind::Disconnected { seat } => format!("{} disconnected", seat_name(core, seat)),
        FeedKind::ProtocolBroken { ref why } => format!("the server broke the protocol: {why}"),
        FeedKind::ConnectionLost => "connection to the server lost".to_string(),
        FeedKind::Captured { building, by } => {
            let name = outpost_name(core, None, building);
            match core.my_team() {
                Some(mine) if mine == by => format!("Your team captured {name}"),
                Some(_) => format!("The enemy team captured {name}"),
                None => format!("{name} captured by {}", side_name(by)),
            }
        }
        FeedKind::ShrineActivated { by, team, xp } => {
            format!(
                "{} activated Shrine of Wisdom ({xp} XP)",
                activator(core, by, team)
            )
        }
        FeedKind::Rune {
            by,
            team,
            kind,
            act,
        } => {
            let who = activator(core, by, team);
            let rune = crate::runes::text_name(kind);
            match act {
                RuneAct::Activated { gold: Some(gold) } => {
                    format!("{who} activated a {rune} rune ({gold} gold)")
                }
                RuneAct::Activated { gold: None } => format!("{who} activated a {rune} rune"),
                RuneAct::Bottled => format!("{who} bottled a {rune} rune"),
                RuneAct::Denied => format!("{who} denied a {rune} rune"),
            }
        }
    }
}

/// A building's fall in the client's own words: its side, its lane and its tier.
fn structure(
    team: Team,
    kind: UnitKind,
    lane: Option<u8>,
    tier: Option<u8>,
    ranged: Option<bool>,
) -> String {
    let lane = lane.map_or("", |lane| match lane {
        0 => " mid",
        1 => " top",
        _ => " bottom",
    });
    let tier = tier.map_or_else(String::new, |tier| format!(" tier {tier}"));
    let ranged = match ranged {
        Some(true) => " ranged",
        Some(false) => " melee",
        None => "",
    };
    format!(
        "a {team:?}{lane}{tier}{ranged} {} has fallen",
        bota_core::kind_def(kind).name
    )
}

/// A hero's death, with its killer, the gold it paid and the seats that assisted when they are
/// known.
fn death(core: &Core, victim: Victim, killer: Option<Killer>, gold: i32, assists: u16) -> String {
    let victim = match victim {
        Victim::Seat(slot) => seat_name(core, slot),
        Victim::Hero(team) => format!("a {team:?} hero"),
    };
    let by = killer.map(|killer| match killer {
        Killer::Seat(slot) => seat_name(core, slot),
        Killer::Unit {
            kind: UnitKind::Tower,
            team,
        } => format!("the {team:?} tower"),
        Killer::Unit {
            kind: UnitKind::Fountain,
            team,
        } => format!("the {team:?} fountain"),
        Killer::Unit { kind, team } => format!("a {team:?} {}", bota_core::kind_def(kind).name),
    });
    let helpers: Vec<String> = bota_proto::assisting_seats(assists)
        .map(|slot| seat_name(core, slot))
        .collect();
    let with = if helpers.is_empty() {
        String::new()
    } else {
        format!(" (assisted by {})", helpers.join(", "))
    };
    match by {
        Some(by) if gold > 0 => format!("{victim} was slain by {by}{with} for {gold} gold"),
        Some(by) => format!("{victim} was slain by {by}{with}"),
        None => format!("{victim} died"),
    }
}

/// The cache path of the icon a feed label's icon variable stands for in a line of a kind: the
/// hero that struck a building by its icon, any other by its logical name in the cache.
fn icon_path(core: &Core, manifest: &Manifest, name: &str, kind: &FeedKind) -> Option<String> {
    if let (
        "killer_icon",
        FeedKind::StructureFell {
            killer: Some(Killer::Seat(slot)),
            ..
        },
    ) = (name, kind)
    {
        let hero = core.view()?.player(*slot)?.hero;
        return manifest.hero_icon(hero.0);
    }
    icon_file(core, name, kind).and_then(|logical| manifest.file(logical))
}

/// The cache's icon for a feed label's icon variable in a line of a kind.
fn icon_file(core: &Core, name: &str, kind: &FeedKind) -> Option<&'static str> {
    Some(match (name, kind) {
        // What fell with a side: the enemy struck it for the side that lost it, the ally's icon
        // for the other; a viewer with no side sees the plain one.
        ("killed_icon", FeedKind::StructureFell { team, .. }) => match core.my_team() {
            Some(mine) if mine == *team => "hud.kill_enemy",
            Some(_) => "hud.kill_ally",
            None => "hud.kill",
        },
        ("outpost_captured_icon", _) => "hud.outpost_captured",
        ("outpost_lost_icon", _) => "hud.outpost_lost",
        ("outpost_icon", _) => "hud.outpost",
        ("aegis_icon", FeedKind::Loot { .. }) => "hud.aegis",
        ("roshan_icon", FeedKind::BossFell { .. } | FeedKind::BossRoared { .. }) => "hud.roshan",
        ("rune_icon", FeedKind::Rune { kind, .. }) => rune_icon(*kind),
        // The Shrine's wisdom.
        ("rune_icon", _) => "hud.rune_timer.wisdom",
        _ => return None,
    })
}

/// A rune's icon in the feed: its buff's spellicon where Dota has one, else the HUD's rune timer
/// of its sort (the Bounty Rune's, or the power runes', which the Water Rune shares).
fn rune_icon(kind: RuneKind) -> &'static str {
    match kind {
        RuneKind::Haste => "icon.rune.haste",
        RuneKind::AmplifyDamage => "icon.rune.doubledamage",
        RuneKind::Arcane => "icon.rune.arcane",
        RuneKind::Regeneration => "icon.rune.regeneration",
        RuneKind::Invisibility => "icon.rune.invisibility",
        RuneKind::Shield => "icon.rune.shield",
        RuneKind::Bounty => "hud.rune_timer.bounty",
        RuneKind::Water | RuneKind::Illusion => "hud.rune_timer.power",
    }
}

/// A side as Dota's English names it.
pub fn side_name(team: Team) -> &'static str {
    match team {
        Team::Radiant => "The Radiant",
        Team::Dire => "The Dire",
        Team::Neutral => "The neutrals",
    }
}

/// The lines the feed shows, from the top, with where each is drawn (true: the toasts at the
/// left): the cache paths of the icons shown and the text.
#[cfg(test)]
pub fn shown(world: &mut World) -> Vec<(bool, Vec<String>, String)> {
    let server = world.resource::<AssetServer>().clone();
    let mut rows: Vec<(bool, usize, Vec<String>, String)> = Vec::new();
    let lines: Vec<(usize, Place, Vec<Entity>, bool)> = world
        .query::<(&Row, &Children, &Node)>()
        .iter(world)
        .map(|(row, children, node)| {
            (
                row.0,
                row.1,
                children.to_vec(),
                node.display != Display::None,
            )
        })
        .collect();
    for (row, place, children, shown) in lines {
        if !shown {
            continue;
        }
        let (mut icons, mut text) = (Vec::new(), String::new());
        for child in children {
            if let (Some(Slot::Icon(_)), Some(image), Some(node)) = (
                world.get::<Slot>(child),
                world.get::<ImageNode>(child),
                world.get::<Node>(child),
            ) && node.display != Display::None
                && let Some(path) = server.get_path(image.image.id())
            {
                icons.push(path.to_string());
            }
            if let (Some(Slot::Text(_)), Some(piece)) =
                (world.get::<Slot>(child), world.get::<Text>(child))
            {
                text.push_str(&piece.0);
            }
        }
        rows.push((place == Place::Left, row, icons, text));
    }
    rows.sort_by_key(|(left, row, ..)| (*left, *row));
    rows.into_iter()
        .map(|(left, _, icons, text)| (left, icons, text))
        .collect()
}

/// How a visible row is dressed, for the end-to-end tests: where it is drawn (true: the toasts at
/// the left), its gradient's ends, its slanted end's and bar's colours (none when not shown), its
/// scale, and its text's size in interface pixels.
#[cfg(test)]
pub struct Look {
    pub left: bool,
    pub gradient: Option<[Color; 2]>,
    pub slash: Option<Color>,
    pub bar: Option<[Color; 2]>,
    pub bar_turn_degrees: Option<f32>,
    pub scale: f32,
    /// How far the row is from its place across, in interface pixels.
    pub slid: f32,
    pub text_size: f32,
}

#[cfg(test)]
pub fn looks(world: &mut World) -> Vec<Look> {
    let rows: Vec<(Entity, bool)> = world
        .query::<(Entity, &Row, &Node)>()
        .iter(world)
        .filter(|(_, _, node)| node.display != Display::None)
        .map(|(entity, row, _)| (entity, row.1 == Place::Left))
        .collect();
    let ends = |gradient: &BackgroundGradient| -> Option<[Color; 2]> {
        let Some(bevy::ui::Gradient::Linear(linear)) = gradient.0.first() else {
            return None;
        };
        Some([linear.stops.first()?.color, linear.stops.last()?.color])
    };
    rows.into_iter()
        .map(|(entity, left)| {
            let children = world
                .get::<Children>(entity)
                .expect("a row's parts")
                .to_vec();
            let mut look = Look {
                left,
                gradient: world.get::<BackgroundGradient>(entity).and_then(ends),
                slash: None,
                bar: None,
                bar_turn_degrees: None,
                scale: world
                    .get::<UiTransform>(entity)
                    .expect("transformed")
                    .scale
                    .x,
                slid: match world
                    .get::<UiTransform>(entity)
                    .expect("transformed")
                    .translation
                    .x
                {
                    Val::Px(x) => x,
                    other => panic!("slid in pixels, not {other:?}"),
                },
                text_size: 0.0,
            };
            for child in children {
                let shown = world
                    .get::<Node>(child)
                    .is_some_and(|node| node.display != Display::None);
                if world.get::<Slash>(child).is_some() && shown {
                    look.slash = world.get::<BackgroundColor>(child).map(|c| c.0);
                }
                if world.get::<Bar>(child).is_some() && shown {
                    look.bar = world.get::<BackgroundGradient>(child).and_then(ends);
                    look.bar_turn_degrees = world
                        .get::<UiTransform>(child)
                        .map(|t| t.rotation.as_degrees());
                }
                if let Some(font) = world.get::<TextFont>(child) {
                    look.text_size = match font.font_size {
                        FontSize::Px(px) => px,
                        _ => 0.0,
                    };
                }
            }
            look
        })
        .collect()
}

#[cfg(test)]
pub(crate) fn root(world: &World) -> Option<Entity> {
    world.get_resource::<Column>().map(|column| column.entity)
}
