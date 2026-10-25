//! The kill feed in the top-right corner: the client's lines, and the
//! game's announcements in Dota's words with Dota's icons where the cache has
//! them (an Outpost captured, a Shrine of Wisdom gathered); without them, the
//! client's own text.

use bevy::prelude::*;
use bota_client_core::{App as Core, FeedKind, FeedLine, Killer, Victim};
use bota_proto::{Team, UnitKind, Vec2};

use crate::dota_text::{Part, Texts};
use crate::manifest::Manifest;
use crate::seat_name::seat_name;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Lines shown at most: what the client keeps.
const ROWS: usize = 8;
/// Icons and texts a line holds at most, alternating, an icon first.
const SLOTS: usize = 3;
const SIZE: f32 = 14.0;
/// Interface pixels of an icon's side in a line.
const ICON: f32 = 18.0;

/// One line of the feed, from the top.
#[derive(Component)]
struct Row(usize);

/// A piece of a line: its `n`th icon or text.
#[derive(Component, Clone, Copy)]
enum Slot {
    Icon(usize),
    Text(usize),
}

pub fn plugin(app: &mut App) {
    app.add_systems(Update, show);
}

/// The feed's column and the lines made in it so far.
#[derive(Resource)]
struct Column {
    entity: Entity,
    rows: usize,
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
    commands.insert_resource(Column { entity, rows: 0 });
}

/// The next line of the column: [`SLOTS`] pairs of an icon and a text.
fn spawn_row(commands: &mut Commands, fonts: &Fonts, column: Entity, row: usize) {
    let line = commands
        .spawn((
            Row(row),
            Node {
                display: Display::None,
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id();
    for n in 0..SLOTS {
        commands.spawn((
            Slot::Icon(n),
            Node {
                width: px(ICON),
                height: px(ICON),
                display: Display::None,
                ..default()
            },
            ImageNode::default(),
            Pickable::IGNORE,
            ChildOf(line),
        ));
        commands.spawn((
            Slot::Text(n),
            ui::text("", &fonts.regular, SIZE, ui::INK),
            ui::shadow(),
            Pickable::IGNORE,
            ChildOf(line),
        ));
    }
}

/// A line laid out: up to [`SLOTS`] pairs of an icon (a cache file) and a
/// text.
type Laid = [(Option<String>, String); SLOTS];

fn show(
    mut commands: Commands,
    column: Option<ResMut<Column>>,
    fonts: Res<Fonts>,
    session: NonSend<Session>,
    texts: Option<Res<Texts>>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    rows: Query<(&Row, &Children, &mut Node)>,
    mut slots: Query<(&Slot, Option<&mut ImageNode>, Option<&mut Text>, &mut Node), Without<Row>>,
) {
    let core = &session.0;
    let lines: Vec<&FeedLine> = core.feed.iter().rev().take(ROWS).rev().collect();
    let Some(mut column) = column else {
        return;
    };
    while column.rows < lines.len() {
        spawn_row(&mut commands, &fonts, column.entity, column.rows);
        column.rows += 1;
    }
    for (Row(row), children, mut node) in rows {
        let line = lines.get(*row);
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
        let laid = lay(core, texts.as_deref(), &manifest, line);
        for child in children {
            let Ok((slot, image, text, mut node)) = slots.get_mut(*child) else {
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
                }
                (Slot::Text(n), _, Some(mut text)) => {
                    text.set_if_neq(Text(laid[n].1.clone()));
                }
                _ => {}
            }
        }
    }
}

/// A line as Dota words it, or the client's text.
fn lay(core: &Core, texts: Option<&Texts>, manifest: &Manifest, line: &FeedLine) -> Laid {
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
                laid[at].0 = icon_file(&name).and_then(|logical| manifest.file(logical));
            }
            Part::Text(text) => {
                laid[at].1.push_str(&text);
                texted = true;
            }
        }
    }
    laid
}

/// The parts of Dota's feed line for an event the client words with the
/// install's labels; none for any other, or without the labels.
fn dota_parts(core: &Core, texts: &Texts, kind: &FeedKind) -> Option<Vec<Part>> {
    match *kind {
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
        FeedKind::HeroDied {
            victim,
            killer,
            gold,
        } => death(core, victim, killer, gold),
        FeedKind::StructureFell { team } => format!("a {team:?} structure has fallen"),
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
    }
}

/// A hero's death, with its killer and the gold it paid when they are known.
fn death(core: &Core, victim: Victim, killer: Option<Killer>, gold: i32) -> String {
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
    match by {
        Some(by) if gold > 0 => format!("{victim} was slain by {by} for {gold} gold"),
        Some(by) => format!("{victim} was slain by {by}"),
        None => format!("{victim} died"),
    }
}

/// The cache's icon for a feed label's icon variable.
fn icon_file(name: &str) -> Option<&'static str> {
    Some(match name {
        "outpost_captured_icon" => "hud.outpost_captured",
        "outpost_lost_icon" => "hud.outpost_lost",
        "outpost_icon" => "hud.outpost",
        // The only rune a feed line tells here is the Shrine's wisdom.
        "rune_icon" => "hud.rune_timer.wisdom",
        _ => return None,
    })
}

/// A side as Dota's English names it.
pub fn side_name(team: Team) -> &'static str {
    match team {
        Team::Radiant => "The Radiant",
        Team::Dire => "The Dire",
        Team::Neutral => "The neutrals",
    }
}
