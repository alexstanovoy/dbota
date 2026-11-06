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
const SIZE: f32 = 14.0;
/// Interface pixels of an icon's side in a line.
const ICON: f32 = 18.0;

/// One line of the feed, from the top.
#[derive(Component)]
struct Row(usize, Place);

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

/// The next line of the column: [`SLOTS`] pairs of an icon and a text.
fn spawn_row(commands: &mut Commands, fonts: &Fonts, column: Entity, row: usize, place: Place) {
    let line = commands
        .spawn((
            Row(row, place),
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
    toasts: Option<Single<Entity, With<crate::chat::ToastColumn>>>,
    fonts: Res<Fonts>,
    sight: crate::fog::EventSight,
    texts: Option<Res<Texts>>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    rows: Query<(&Row, &Children, &mut Node)>,
    mut slots: Query<(&Slot, Option<&mut ImageNode>, Option<&mut Text>, &mut Node), Without<Row>>,
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
    for (Row(row, place), children, mut node) in rows {
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
