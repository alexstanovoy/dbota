//! The scoreboard's parts, laid out as Dota's `dota_hud_scoreboard` lays
//! them out, at its sizes: Dota's pixels are of a 1080-line screen and ours
//! of a 900-line one, hence [`DOTA`].

use bevy::prelude::*;
use bota_proto::Team;

use crate::dota_text::Texts;
use crate::manifest::Manifest;
use crate::ui::{self, Fonts};

use super::model::BAG;

/// Interface pixels to one of Dota's.
const DOTA: f32 = 900.0 / 1080.0;
/// Most rows a side is given, however many seats a view names.
pub const MAX_ROWS: usize = 12;

/// Dota's pixels as ours.
pub fn d(v: f32) -> Val {
    px(v * DOTA)
}

/// Dota's font size as ours.
fn f(v: f32) -> f32 {
    v * DOTA
}

fn hex(rgba: u32) -> Color {
    let [r, g, b, a] = rgba.to_be_bytes();
    Color::srgba_u8(r, g, b, a)
}

/// Dota's colours of the scoreboard's text.
pub const HEADING: u32 = 0xafd2e3ff;
pub const GOLD: u32 = 0xffcc33ff;
pub const SCORE: u32 = 0xacacacff;
pub const LEVEL: u32 = 0xe7d291ff;
pub const SECONDS: u32 = 0x8a878dff;
const INVENTORY: u32 = 0xcebc83ff;
const NAME: u32 = 0xdcdcdcff;
const HERO_NAME: u32 = 0xa8a8a8ff;
/// A row's ground, and one's own row's.
pub const ROW: u32 = 0x111920bb;
pub fn own_row() -> BackgroundGradient {
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        ColorStop::new(hex(0x373c3bff), percent(0)),
        ColorStop::new(hex(0x505959ff), percent(40)),
        ColorStop::new(hex(0x505959ff), percent(60)),
        ColorStop::new(hex(0x2f3535ff), percent(100)),
    ]))
}

pub fn color(rgba: u32) -> Color {
    hex(rgba)
}

/// The whole scoreboard, its key held or its button pressed.
#[derive(Component)]
pub struct Board;

/// The items beside the rows (Dota's Show Items).
#[derive(Component)]
pub struct Inventories;

/// The button that shows and hides the items.
#[derive(Component)]
pub struct ShowItems;

/// Where a side's rows go, one a seat of the match.
#[derive(Component)]
pub struct Seats(pub Team);

/// Where a side's items' boxes go, one a seat, beside its rows.
#[derive(Component)]
pub struct Bags(pub Team);

/// The kills of a side, in its heading.
#[derive(Component)]
pub struct TeamScore(pub Team);

/// Which row a part belongs to: a side's `rank`-th seat.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct At {
    pub team: Team,
    pub rank: u8,
}

/// What a part of a row shows.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Row,
    /// The player's colour down the row's edge.
    Color,
    Hero,
    Respawn,
    Name,
    HeroName,
    /// A talent's pip: `TalentSlot(n)`.
    Pip(u8),
    /// The experience ring round an ally's level, and the level in it.
    Ring,
    RingLevel,
    /// The plain level of a foe.
    Level,
    Gold,
    NetWorth,
    Kills,
    Deaths,
    Assists,
    LastHits,
    Denies,
    Ult,
    UltSeconds,
    /// An inventory (`0..6`) or backpack (`6..9`) slot.
    Item(u8),
}

/// What spawning needs.
pub struct Kit<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub fonts: &'a Fonts,
    pub manifest: &'a Manifest,
    pub server: &'a AssetServer,
    pub texts: &'a Texts,
}

impl Kit<'_, '_, '_> {
    /// One of Dota's labels, or ours where the cache has none.
    fn label(&self, key: &str, fallback: &str) -> String {
        self.texts
            .label(key, &[])
            .unwrap_or_else(|| fallback.to_owned())
            .to_uppercase()
    }

    fn text(&mut self, parent: Entity, s: impl Into<String>, size: f32, rgba: u32) -> Entity {
        self.commands
            .spawn((
                ui::text(s, &self.fonts.bold, f(size), hex(rgba)),
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id()
    }

    fn node(&mut self, parent: Entity, node: Node) -> Entity {
        self.commands
            .spawn((node, Pickable::IGNORE, ChildOf(parent)))
            .id()
    }
}

/// The scoreboard under the top bar at the left, hidden: Dota's backdrop
/// with both sides' headings, the items beside it; the rows come with the
/// match's seats (see [`seat`]), and the backdrop fits them, as Dota's lists
/// the players in the game and no empty slot.
pub fn spawn(kit: &mut Kit, root: Entity) {
    let board = kit
        .commands
        .spawn((
            Board,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: d(54.0),
                align_items: AlignItems::FlexStart,
                ..default()
            },
            GlobalZIndex(15),
            Visibility::Hidden,
            Interaction::default(),
            crate::hud::Blocker,
            ChildOf(root),
        ))
        .id();
    let back = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(hex(0x000000da)),
            BoxShadow::new(hex(0x000000f5), px(0), px(0), px(0), d(8.0)),
            ChildOf(board),
        ))
        .id();
    backdrop(kit, back, 1.0, 0.0);
    for team in [Team::Radiant, Team::Dire] {
        heading(kit, back, team);
        let rows = kit.node(
            back,
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        kit.commands.entity(rows).insert(Seats(team));
    }
    buttons(kit, back);
    inventories(kit, board);
}

/// Dota's scoreboard backdrop over the dark ground, covering the node
/// `scale` times over and placed `down` the way from its top to its foot,
/// as Dota's `background-size` and `background-position` put it; the node
/// clips it.
fn backdrop(kit: &mut Kit, parent: Entity, scale: f32, down: f32) {
    let Some(path) = kit.manifest.file("hud.scoreboard_bg") else {
        return;
    };
    kit.commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            top: percent(-100.0 * (scale - 1.0) * down),
            width: percent(100.0 * scale),
            height: percent(100.0 * scale),
            ..default()
        },
        ImageNode::new(kit.server.load(path)),
        ui::Cover,
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// A side's heading: its kills and name; over the Radiant's rows, the
/// columns' names.
fn heading(kit: &mut Kit, parent: Entity, team: Team) {
    let row = kit.node(
        parent,
        Node {
            height: d(32.0),
            align_items: AlignItems::FlexEnd,
            ..default()
        },
    );
    let (glow, name) = match team {
        Team::Dire => (0xe74d088f, kit.label("DOTA_BadGuysCaps", "The Dire")),
        _ => (0x9be40c8f, kit.label("DOTA_GoodGuysCaps", "The Radiant")),
    };
    let shadow = TextShadow {
        offset: Vec2::ZERO,
        color: hex(glow),
    };
    let score = kit.node(
        row,
        Node {
            width: d(48.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    let label = kit.text(score, "0", 30.0, 0xffffffff);
    kit.commands.entity(label).insert((TeamScore(team), shadow));
    let title = kit.node(
        row,
        Node {
            width: d(288.0),
            margin: UiRect::left(d(8.0)),
            padding: UiRect::top(d(8.0)),
            ..default()
        },
    );
    let label = kit.text(title, name, 18.0, 0xffffffff);
    kit.commands.entity(label).insert(shadow);
    if team == Team::Radiant {
        columns(kit, row);
    }
}

/// The columns' names, each over its column (Dota's `SubheaderDesc`).
fn columns(kit: &mut Kit, row: Entity) {
    let names = [
        kit.label("DOTA_Scoreboard_Header_TalentTree", "TT"),
        kit.label("DOTA_Scoreboard_Header_Lvl", "Lvl"),
        kit.label("DOTA_Scoreboard_Header_Gold", "Gold"),
        kit.label("dota_stat_dropdown_networth", "Net Worth"),
        kit.label("DOTA_Scoreboard_K", "K"),
        kit.label("DOTA_Scoreboard_D", "D"),
        kit.label("DOTA_Scoreboard_A", "A"),
        "LH".to_owned(),
        "DN".to_owned(),
        kit.label("DOTA_Scoreboard_Ult", "Ult"),
    ];
    for (name, column) in names.into_iter().zip(super::row::COLUMNS) {
        let cell = kit.node(
            row,
            Node {
                justify_content: JustifyContent::Center,
                margin: UiRect::bottom(d(2.0)),
                ..column.node()
            },
        );
        let label = kit.text(cell, name, 15.0, HEADING);
        kit.commands
            .entity(label)
            .insert(TextLayout::linebreak(bevy::text::LineBreak::NoWrap));
    }
}

/// The buttons along the foot (Dota's `NestedMenuButtons`): Show Items.
fn buttons(kit: &mut Kit, parent: Entity) {
    let bar = kit.node(
        parent,
        Node {
            height: d(30.0),
            margin: UiRect::all(d(5.0)),
            justify_content: JustifyContent::FlexEnd,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let art = ui::art(kit.manifest, kit.server, "hud.show_items", ui::INK_DIM);
    kit.commands.spawn((
        ShowItems,
        Button,
        Node {
            width: d(32.0),
            height: d(32.0),
            margin: UiRect::horizontal(d(4.0)),
            ..default()
        },
        art,
        ChildOf(bar),
    ));
}

/// The items beside the rows: each side's block under its INVENTORY
/// heading, as tall as its heading and rows, the Dire's down past the foot's
/// buttons too.
fn inventories(kit: &mut Kit, board: Entity) {
    let panel = kit
        .commands
        .spawn((
            Inventories,
            Node {
                width: d(258.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(board),
        ))
        .id();
    let title = kit.label("DOTA_HUD_Scoreboard_inventory", "Inventory");
    // The foot's buttons: 30 high with 5 either side.
    for (team, foot, down) in [(Team::Radiant, 0.0, 0.0), (Team::Dire, 40.0, 0.52)] {
        let block = kit
            .commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::bottom(d(foot)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(hex(0x000000da)),
                BoxShadow::new(hex(0x000000ff), px(0), px(0), px(0), d(4.0)),
                Pickable::IGNORE,
                ChildOf(panel),
            ))
            .id();
        backdrop(kit, block, 2.6, down);
        let head = kit.node(
            block,
            Node {
                height: d(32.0),
                padding: UiRect::new(d(10.0), px(0), d(10.0), px(0)),
                ..default()
            },
        );
        kit.text(head, title.clone(), 15.0, INVENTORY);
        let rows = kit.node(
            block,
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        kit.commands.entity(rows).insert(Bags(team));
    }
}

/// A seat's row in `seats` and its items' box in `bags`.
pub fn seat(kit: &mut Kit, seats: Entity, bags: Entity, at: At) {
    super::row::spawn(kit, seats, at);
    inventory(kit, bags, at);
}

/// One seat's items (Dota's `DOTAScoreboardPlayerInventory`).
fn inventory(kit: &mut Kit, parent: Entity, at: At) {
    let row = kit
        .commands
        .spawn((
            Node {
                width: d(242.0),
                height: d(64.0),
                margin: UiRect::new(d(4.0), d(4.0), d(3.0), d(3.0)),
                padding: UiRect::left(d(4.0)),
                border: UiRect::all(d(1.0)),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.9)),
            BorderColor::all(Color::BLACK),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    let grid = kit.node(
        row,
        Node {
            width: d(141.0),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        },
    );
    for slot in 0..6 {
        item(kit, grid, at, slot, (47.0, 32.0), 0.0);
    }
    let backpack = kit.node(
        row,
        Node {
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    for slot in 6..BAG as u8 {
        item(kit, backpack, at, slot, (28.0, 21.0), 1.0);
    }
    let neutral = kit.node(
        row,
        Node {
            flex_grow: 1.0,
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    let art = ui::art(kit.manifest, kit.server, "hud.neutral_slot", ui::PANEL_DEEP);
    kit.commands.spawn((
        Node {
            width: d(40.0),
            height: d(40.0),
            border: UiRect::all(d(1.0)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BorderColor::all(Color::BLACK),
        art,
        Pickable::IGNORE,
        ChildOf(neutral),
    ));
}

fn item(kit: &mut Kit, parent: Entity, at: At, slot: u8, (w, h): (f32, f32), edge: f32) {
    kit.commands.spawn((
        at,
        Field::Item(slot),
        Node {
            width: d(w),
            height: d(h),
            border: UiRect::all(d(edge)),
            ..default()
        },
        BorderColor::all(Color::BLACK),
        BackgroundColor(Color::srgba(0.07, 0.08, 0.1, 1.0)),
        ImageNode::default(),
        ui::Cover,
        Pickable::IGNORE,
        ChildOf(parent),
    ));
}

/// Dota's font sizes as ours, for the rows.
pub fn font(v: f32) -> f32 {
    f(v)
}

/// A name or a hero's name in its colour.
pub fn name_color(hero: bool) -> Color {
    hex(if hero { HERO_NAME } else { NAME })
}
