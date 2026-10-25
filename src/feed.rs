//! The kill feed in the top-right corner: the client's lines, and the
//! game's announcements in Dota's words with Dota's icons where the cache has
//! them (an Outpost captured, a Shrine of Wisdom gathered); without them, the
//! client's own text.

use bevy::prelude::*;
use bota_client_core::{App as Core, FeedLine};
use bota_proto::{EventKind, Team, UnitKind};

use crate::dota_text::{Part, Texts};
use crate::manifest::Manifest;
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

/// The feed's column under the frame rate, lines right-aligned.
pub fn spawn(commands: &mut Commands, fonts: &Fonts, root: Entity) {
    let column = commands
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
    for row in 0..ROWS {
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
}

/// A line laid out: up to [`SLOTS`] pairs of an icon (a cache file) and a
/// text.
type Laid = [(Option<String>, String); SLOTS];

fn show(
    session: NonSend<Session>,
    texts: Option<Res<Texts>>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    rows: Query<(&Row, &Children, &mut Node)>,
    mut slots: Query<(&Slot, Option<&mut ImageNode>, Option<&mut Text>, &mut Node), Without<Row>>,
) {
    let core = &session.0;
    let lines: Vec<&FeedLine> = core.feed.iter().rev().take(ROWS).rev().collect();
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
    let dota = texts
        .zip(line.told.as_ref())
        .and_then(|(texts, told)| dota_parts(core, texts, told));
    let Some(parts) = dota else {
        laid[0].1 = line.text.clone();
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
fn dota_parts(core: &Core, texts: &Texts, told: &EventKind) -> Option<Vec<Part>> {
    match *told {
        EventKind::Captured { unit, team, .. } => {
            let seen = core.view()?.unit(unit);
            let name = seen
                .zip(core.session())
                .map(|(u, s)| bota_core::unit_name(s.map(), UnitKind::Outpost, u.team, u.pos))
                .unwrap_or("Outpost")
                .to_owned();
            let (key, values) = match core.my_team() {
                Some(mine) if mine == team => {
                    ("DOTA_HUD_OutpostCaptured", vec![("outpost_name", name)])
                }
                Some(_) => ("DOTA_HUD_OutpostLost", vec![("outpost_name", name)]),
                None => (
                    "DOTA_HUD_OutpostCaptured_Spectator",
                    vec![
                        ("outpost_name", name),
                        ("team_name", side_name(team).to_owned()),
                    ],
                ),
            };
            texts.label_parts(key, &values)
        }
        EventKind::ShrineActivated { team, xp, by, .. } => {
            let rune = texts.label("DOTA_HUD_Rune_XP", &[("xp", xp.to_string())])?;
            let who = by
                .and_then(|hero| core.view()?.unit(hero)?.owner)
                .map_or_else(|| side_name(team).to_owned(), |slot| core.seat_name(slot));
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
