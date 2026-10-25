//! The tactical phase as Dota's strategy screen (`#StrategyScreen` of the
//! install's `dota_hud_pregame` layout, at its sizes for a 1080-line
//! screen): along the top every seat's hero and the clock over STRATEGY
//! TIME; at the left the hero one enters battle as, its name and abilities;
//! at the right STARTING ITEMS, the gold that remains, the inventory as the
//! choices fill it, and the shop's basic items to buy. What is bought here is
//! bought when the match begins, in the slots they are put in. The inventory
//! is the bottom panel's own: drag moves an item, and the item menu's Sell
//! takes it back, as selling it does in Dota.

use bevy::prelude::*;
use bota_core::item_def;
use bota_proto::{ItemId, Team};

use crate::dota_text::Texts;
use crate::manifest::Manifest;
use crate::player_colour::PlayerColours;
use crate::select::{ClockSeen, TacticalPhase, clock};
use crate::session::Session;
use crate::shop::Catalog;
use crate::shop::{Place, spawn_cell};
use crate::ui::{self, Fonts, dota};

/// Dota's strategy screen sizes (dota_hud_pregame.vcss), in its pixels.
const DETAILS: (f32, f32, f32) = (180.0, 130.0, 630.0);
const RIGHT: (f32, f32, f32) = (200.0, 132.0, 883.0);
const LEFT_COLUMN: f32 = 272.0;
const INVENTORY: (f32, f32) = (62.0, 46.0);
const BACKPACK: (f32, f32) = (40.0, 30.0);
const LIST_ITEM: (f32, f32) = (41.0, 30.0);
/// Its colours: the panels' #0009, their headers' black, the controls'
/// titles #8e9795, the gold #ffcc33, ENTERING BATTLE AS #ffffff90.
const PANEL: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const TITLE_INK: Color = Color::srgb(0.557, 0.592, 0.584);
const GOLD_INK: Color = Color::srgb(1.0, 0.8, 0.2);
const BATTLE_INK: Color = Color::srgba(1.0, 1.0, 1.0, 0.565);
/// Inventory slots a hero has, then its backpack's.
const BAG: usize = 6;
const BACKPACK_SLOTS: usize = 3;
/// The shop's categories offered to start with (`shops.txt` names): its
/// basics but the secret shop's, and the cheap upgrades.
const SECTIONS: [&str; 5] = [
    "consumables",
    "attributes",
    "weapons_armor",
    "misc",
    "basics",
];

/// The screen.
#[derive(Component)]
struct Screen;

/// The ability of the hero one enters battle as, by id: hovered, it opens
/// the ability's tooltip as in the game.
#[derive(Component)]
pub struct StrategyAbility(pub u16);

/// The clock.
#[derive(Component)]
struct Clock;

/// The gold that remains.
#[derive(Component)]
struct Gold;

/// What the screen was built for: the seats' heroes and names, and one's own
/// seat.
#[derive(Default, PartialEq)]
struct Built(Vec<(u8, Option<u16>, String)>, Option<u8>);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (build, show, clock_text).chain());
}

/// Builds the screen anew when the tactical phase opens on other seats than
/// it was built for.
#[allow(clippy::too_many_arguments)]
fn build(
    mut commands: Commands,
    session: NonSend<Session>,
    phase: Res<TacticalPhase>,
    art: (Res<Fonts>, Res<Manifest>, Res<AssetServer>),
    texts: Res<Texts>,
    catalog: Res<Catalog>,
    colours: Res<PlayerColours>,
    screens: Query<Entity, With<Screen>>,
    mut built: Local<Built>,
) {
    let core = &session.0;
    if !phase.0 {
        return;
    }
    let now = Built(
        core.lobby
            .slots
            .iter()
            .map(|s| (s.slot.0, s.hero.map(|h| h.0), s.name.clone()))
            .collect(),
        core.my_slot.map(|s| s.0),
    );
    if *built == now && !screens.is_empty() {
        return;
    }
    *built = now;
    for screen in &screens {
        commands.entity(screen).despawn();
    }
    let (fonts, manifest, server) = (&*art.0, &*art.1, &*art.2);
    let kit = Kit {
        fonts,
        manifest,
        server,
        texts: &texts,
    };
    let root = commands
        .spawn((
            Screen,
            crate::hud::Blocker,
            Interaction::default(),
            ui::fill(),
            BackgroundColor(Color::srgba(0.01, 0.012, 0.018, 0.97)),
            GlobalZIndex(25),
            Visibility::Hidden,
        ))
        .id();
    if let Some(path) = manifest.file("strategy.background") {
        commands.spawn((
            ui::fill(),
            ImageNode::new(server.load(path)).with_color(Color::srgba(1.0, 1.0, 1.0, 0.35)),
            Pickable::IGNORE,
            ChildOf(root),
        ));
    }
    header(&mut commands, &kit, core, &colours, root);
    let hero = core
        .lobby
        .slots
        .iter()
        .find(|s| Some(s.slot) == core.my_slot)
        .and_then(|s| s.hero);
    if let Some(hero) = hero {
        details(&mut commands, &kit, hero.0, root);
    }
    starting_items(&mut commands, &kit, &catalog, root);
}

/// What the parts of the screen are drawn with.
struct Kit<'a> {
    fonts: &'a Fonts,
    manifest: &'a Manifest,
    server: &'a AssetServer,
    texts: &'a Texts,
}

impl Kit<'_> {
    /// Dota's line of `key` from the cache, else `plain`, in capitals.
    fn word(&self, key: &str, plain: &str) -> String {
        self.texts
            .label(key, &[])
            .unwrap_or_else(|| plain.to_owned())
            .to_uppercase()
    }

    fn image(&self, path: Option<String>) -> ImageNode {
        path.map_or_else(
            || ImageNode::default().with_color(Color::NONE),
            |path| ImageNode::new(self.server.load(path)),
        )
    }
}

/// Along the top: Radiant's seats, the clock over STRATEGY TIME, Dire's
/// seats, each seat's hero under its player's colour (Dota's `#Header`).
fn header(
    commands: &mut Commands,
    kit: &Kit,
    core: &bota_client_core::App,
    colours: &PlayerColours,
    root: Entity,
) {
    let row = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(0),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    side(commands, kit, (core, colours), Team::Radiant, row);
    let centre = commands
        .spawn((
            Node {
                width: dota(250.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    commands.spawn((
        Clock,
        ui::text("", &kit.fonts.regular, 64.0 * ui::DOTA, Color::WHITE),
        Node {
            height: dota(88.0),
            ..default()
        },
        ChildOf(centre),
    ));
    commands.spawn((
        ui::text(
            kit.word("DOTA_Hero_Selection_StrategyTime", "Strategy Time"),
            &kit.fonts.bold,
            18.0 * ui::DOTA,
            TITLE_INK,
        ),
        ChildOf(centre),
    ));
    side(commands, kit, (core, colours), Team::Dire, row);
}

/// One side's seats in the header, towards the clock: each its hero under
/// its player's colour.
fn side(
    commands: &mut Commands,
    kit: &Kit,
    (core, colours): (&bota_client_core::App, &PlayerColours),
    team: Team,
    row: Entity,
) {
    let seats = commands
        .spawn((
            Node {
                column_gap: dota(4.0),
                justify_content: if team == Team::Radiant {
                    JustifyContent::FlexEnd
                } else {
                    JustifyContent::FlexStart
                },
                width: dota(5.0 * 70.0),
                padding: UiRect::horizontal(dota(8.0)),
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    for (rank, seat) in core
        .lobby
        .slots
        .iter()
        .filter(|s| s.team == team)
        .enumerate()
    {
        let card = commands
            .spawn((
                Node {
                    width: dota(66.0),
                    height: dota(100.0),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BackgroundColor(Color::srgb(0.05, 0.05, 0.06)),
                ChildOf(seats),
            ))
            .id();
        let strip = Node {
            height: dota(4.0),
            width: percent(100),
            ..default()
        };
        commands.spawn((
            strip,
            BackgroundColor(colours.at(team, rank)),
            ChildOf(card),
        ));
        let picture = seat.hero.and_then(|h| kit.manifest.portrait(h.0));
        let fill = Node {
            flex_grow: 1.0,
            width: percent(100),
            ..default()
        };
        commands.spawn((fill, kit.image(picture), ChildOf(card)));
    }
}

/// At the left: ENTERING BATTLE AS, the hero's name, its abilities and its
/// picture (Dota's `#SelectedHeroDetails`).
fn details(commands: &mut Commands, kit: &Kit, hero: u16, root: Entity) {
    let Some(def) = bota_core::hero_def(bota_proto::HeroId(hero)) else {
        return;
    };
    let (left, top, width) = DETAILS;
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: dota(left),
                top: dota(top),
                width: dota(width),
                flex_direction: FlexDirection::Column,
                row_gap: dota(8.0),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        ui::text(
            kit.word("DOTA_Hero_Selection_EnteringBattleAs", "Entering Battle As"),
            &kit.fonts.regular,
            18.0 * ui::DOTA,
            BATTLE_INK,
        ),
        ChildOf(column),
    ));
    commands.spawn((
        ui::text(
            def.name.to_uppercase(),
            &kit.fonts.bold,
            60.0 * ui::DOTA,
            Color::WHITE,
        ),
        TextShadow {
            offset: Vec2::new(0.0, 2.0),
            color: Color::BLACK,
        },
        ChildOf(column),
    ));
    let abilities = commands
        .spawn((
            Node {
                column_gap: dota(8.0),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    for id in def.abilities {
        let icon = Node {
            width: dota(64.0),
            height: dota(64.0),
            ..default()
        };
        let art = kit.image(kit.manifest.ability_icon(id.0));
        commands.spawn((
            StrategyAbility(id.0),
            Interaction::default(),
            icon,
            art,
            ChildOf(abilities),
        ));
    }
    commands.spawn((
        Node {
            width: dota(330.0),
            height: dota(471.0),
            margin: UiRect::top(dota(16.0)),
            ..default()
        },
        kit.image(kit.manifest.portrait(hero)),
        ChildOf(column),
    ));
}

/// At the right: STARTING ITEMS (Dota's `#StartingItems`), the gold that
/// remains, the inventory, and the shop's basic items.
fn starting_items(commands: &mut Commands, kit: &Kit, catalog: &Catalog, root: Entity) {
    let (right, top, width) = RIGHT;
    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: dota(right),
                top: dota(top),
                width: dota(width),
                min_height: dota(340.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(PANEL),
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Node {
            width: percent(100),
            padding: UiRect::new(dota(14.0), dota(8.0), dota(6.0), dota(6.0)),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        ChildOf(panel),
        children![ui::text(
            kit.word("DOTA_Item_Build_Starting_Items", "Starting Items"),
            &kit.fonts.bold,
            16.0 * ui::DOTA,
            Color::WHITE,
        )],
    ));
    let contents = commands
        .spawn((
            Node {
                margin: UiRect::all(dota(12.0)),
                column_gap: dota(20.0),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    left_column(commands, kit, contents);
    item_lists(commands, kit, catalog, contents);
}

/// REMAINING GOLD and INVENTORY: six slots and the backpack's three.
fn left_column(commands: &mut Commands, kit: &Kit, contents: Entity) {
    let column = commands
        .spawn((
            Node {
                width: dota(LEFT_COLUMN),
                flex_direction: FlexDirection::Column,
                row_gap: dota(4.0),
                ..default()
            },
            ChildOf(contents),
        ))
        .id();
    title(
        commands,
        kit,
        column,
        "DOTA_Hero_Selection_RemainingGold",
        "Remaining Gold",
    );
    let gold = commands
        .spawn((
            Node {
                column_gap: dota(4.0),
                align_items: AlignItems::Center,
                margin: UiRect::bottom(dota(7.0)),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    commands.spawn((
        Gold,
        ui::text("", &kit.fonts.bold, 24.0 * ui::DOTA, GOLD_INK),
        ChildOf(gold),
    ));
    commands.spawn((
        Node {
            width: dota(24.0),
            height: dota(24.0),
            ..default()
        },
        kit.image(kit.manifest.file("hud.gold")),
        ChildOf(gold),
    ));
    title(
        commands,
        kit,
        column,
        "DOTA_Hero_Selection_Inventory",
        "Inventory",
    );
    let inventory = commands
        .spawn((
            Node {
                width: dota(276.0),
                padding: UiRect::top(dota(9.0)),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                border: UiRect::vertical(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.19, 0.2, 0.21, 0.4)),
            BorderColor {
                top: Color::srgba(0.33, 0.33, 0.33, 0.27),
                bottom: Color::srgba(0.07, 0.07, 0.07, 0.27),
                ..BorderColor::DEFAULT
            },
            ChildOf(column),
        ))
        .id();
    let rows = [
        (0..3, INVENTORY),
        (3..BAG, INVENTORY),
        (BAG..BAG + BACKPACK_SLOTS, BACKPACK),
    ];
    for (slots, size) in rows {
        let row = commands.spawn((Node::default(), ChildOf(inventory))).id();
        for at in slots {
            inventory_slot(commands, kit, row, at, size);
        }
    }
}

/// One slot of the inventory: the bottom panel's own item box, dark until a
/// choice fills it.
fn inventory_slot(commands: &mut Commands, kit: &Kit, row: Entity, at: usize, size: (f32, f32)) {
    let slot = crate::hud::spawn_slot_box(
        commands,
        kit.fonts,
        row,
        (size.0 * ui::DOTA, size.1 * ui::DOTA),
        crate::hud::ItemBox(at as u8),
        false,
        false,
    );
    commands
        .entity(slot)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.margin = UiRect::new(px(0), dota(6.0), px(0), dota(6.0));
        });
}

/// The shop's basic items, under each category's title.
fn item_lists(commands: &mut Commands, kit: &Kit, catalog: &Catalog, contents: Entity) {
    let column = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: dota(4.0),
                ..default()
            },
            ChildOf(contents),
        ))
        .id();
    title(
        commands,
        kit,
        column,
        "dota_item_build_common_starting_items",
        "Common Items",
    );
    let art = (kit.fonts, kit.manifest, kit.server);
    // Without the cache, the client core's basic groups.
    let from_cache = catalog.sections.iter().any(|s| s.key.is_some());
    let sections = catalog.sections.iter().filter(|s| match s.key {
        Some(key) => SECTIONS.contains(&key),
        None => !from_cache && s.tab == crate::shop::Tab::Basics,
    });
    for section in sections {
        commands.spawn((
            ui::text(
                section.title.to_uppercase(),
                &kit.fonts.regular,
                12.0 * ui::DOTA,
                TITLE_INK,
            ),
            ChildOf(column),
        ));
        let list = commands
            .spawn((
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: dota(4.0),
                    row_gap: dota(4.0),
                    margin: UiRect::bottom(dota(4.0)),
                    ..default()
                },
                ChildOf(column),
            ))
            .id();
        let size = (LIST_ITEM.0 * ui::DOTA, LIST_ITEM.1 * ui::DOTA);
        for id in &section.items {
            spawn_cell(commands, art, list, *id, Place::Grid, size);
        }
    }
}

/// A control's title (Dota's `.StrategyControlTitle`).
fn title(commands: &mut Commands, kit: &Kit, parent: Entity, key: &str, plain: &str) {
    commands.spawn((
        ui::text(
            kit.word(key, plain),
            &kit.fonts.regular,
            16.0 * ui::DOTA,
            TITLE_INK,
        ),
        ChildOf(parent),
    ));
}

/// The screen while the tactical phase is on.
fn show(phase: Res<TacticalPhase>, mut screens: Query<&mut Visibility, With<Screen>>) {
    for mut screen in &mut screens {
        screen.set_if_neq(ui::visible(phase.0));
    }
}

/// The clock and the gold that remains.
fn clock_text(
    session: NonSend<Session>,
    phase: Res<TacticalPhase>,
    seen: Res<ClockSeen>,
    time: Res<Time>,
    mut clocks: Query<&mut Text, (With<Clock>, Without<Gold>)>,
    mut golds: Query<&mut Text, With<Gold>>,
) {
    if !phase.0 {
        return;
    }
    let left = seen.left(time.elapsed_secs()).map_or(String::new(), clock);
    for mut text in &mut clocks {
        text.set_if_neq(Text(left.clone()));
    }
    let gold = session.0.gold_after_queue().to_string();
    for mut text in &mut golds {
        text.set_if_neq(Text(gold.clone()));
    }
}

/// The inventory as the choices fill it: what stacks in Dota stacks, up to
/// its limit; the rest take a slot each, the inventory's first.
pub fn slots_of(queued: &[u16]) -> Vec<(u16, u16)> {
    let mut slots: Vec<(u16, u16)> = Vec::new();
    for &id in queued {
        let def = item_def(ItemId(id));
        let (charges, limit) =
            def.map_or((0, 0), |d| (u16::from(d.charges), u16::from(d.stack_limit)));
        let stacks = limit > 0 && charges > 0;
        match slots
            .iter_mut()
            .find(|(held, count)| stacks && *held == id && *count + charges <= limit)
        {
            Some((_, count)) => *count += charges,
            None => slots.push((id, charges)),
        }
    }
    slots
}
