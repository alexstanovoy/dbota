//! The tactical phase as Dota's strategy screen, drawn at the sizes measured in a
//! reference screenshot ([`crate::pregame::reference`]): along the top every seat's hero and the
//! clock over STRATEGY TIME; at the left the hero one enters battle as, its name, its talents,
//! innate and abilities (each with its popup) and the hero's model on a dark stage; at the right
//! STARTING ITEMS, the gold that remains, the inventory as the choices fill it, and the shop's
//! basic items to buy. What is bought here is bought when the match begins, in the slots they are
//! put in. The inventory uses the bottom panel's slots: drag moves an item and a right click
//! takes its pending purchase back at full price.
//!
//! Not drawn, as bota has nothing behind them: the strategy map (lane choices, enemy lane
//! predictions, ward placement), friends and foes, the tabs but this one, the guide's
//! recommendations, team items, the way back to the heroes (the pick is final) and "skip ahead"
//! (the server ends the phase by its clock alone).

use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use crate::dota_text::Texts;
use crate::manifest::Manifest;
use crate::player_colour::PlayerColours;
use crate::pregame::reference as r;
use crate::pregame::{Kit, NAME_FACE, STRATEGY, Spot, bar, panel};
use crate::select::{ClockSeen, TacticalPhase, clock};
use crate::session::Session;
use crate::shop::Catalog;
use crate::shop::{Place, spawn_cell};
use crate::theme::Face;
use crate::ui::{self, Fonts, dota};

/// The starting items' panel sizes (the install's dota_hud_pregame.vcss, in its pixels of a
/// 1080-line screen), which the reference screenshot shows unchanged.
const RIGHT_WIDTH: f32 = 883.0;
/// The gap between inventory slots, and the box's padding round them.
const SLOT_GAP: f32 = 6.0;
const BOX_PADDING: f32 = 12.0;
const INVENTORY: (f32, f32) = (62.0, 46.0);
const BACKPACK: (f32, f32) = (40.0, 30.0);
const LIST_ITEM: (f32, f32) = (41.0, 30.0);
/// Items in a row of a shop section's grid, and the gap between two sections.
const LIST_COLUMNS: f32 = 4.0;
const SECTIONS_GAP: f32 = 20.0;
/// Its colours: the panels' #0009, their headers' black, the controls'
/// titles #8e9795, the gold #ffcc33, ENTERING BATTLE AS #ffffff90.
const PANEL: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
const TITLE_INK: Color = Color::srgb(0.557, 0.592, 0.584);
const GOLD_INK: Color = Color::srgb(1.0, 0.8, 0.2);
const BATTLE_INK: Color = Color::srgba(1.0, 1.0, 1.0, 0.565);
/// Inventory slots a hero has, then its backpack's.
const BAG: usize = 6;
const BACKPACK_SLOTS: usize = 3;
/// The normal preparation storefront's authored categories and title fallbacks.
const SECTIONS: [(&str, &str); 3] = [
    ("pregame_consumables", "Consumables"),
    ("pregame_attributes", "Attributes"),
    ("pregame_accessories", "Accessories"),
];

/// The screen.
#[derive(Component)]
pub(crate) struct Screen;

/// The clock.
#[derive(Component)]
struct Clock;

/// The gold that remains.
#[derive(Component)]
struct Gold;

/// What the screen was built for: the seats' heroes and names, and one's own
/// seat.
#[derive(Default, PartialEq)]
struct Built(Vec<(u8, Option<u16>, String)>, Option<u8>, u16);

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (build, show, clock_text).chain());
}

/// Builds the screen anew when the tactical phase opens on other seats than
/// it was built for, or the items the match plays with change.
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
        // The screen goes with the phase: its hero's model must not stay on a stage (a camera, a
        // light, an animated actor) through the match.
        for screen in &screens {
            commands.entity(screen).despawn();
        }
        *built = Built::default();
        return;
    }
    let now = Built(
        core.lobby
            .slots
            .iter()
            .map(|s| (s.slot.0, s.hero.map(|h| h.0), s.name.clone()))
            .collect(),
        core.my_slot.map(|s| s.0),
        catalog.offered.items(),
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
            Hovered::default(),
            ui::fill(),
            BackgroundColor(Color::srgba(0.01, 0.012, 0.018, 0.97)),
            GlobalZIndex(25),
            Visibility::Hidden,
        ))
        .id();
    if let Some(path) = manifest.file("strategy.background") {
        commands.spawn((
            ui::fill(),
            // Stretched over the whole screen: left to its own proportions the square art covers
            // only a square of it, and its edges show as a lighter band between darker sides.
            ImageNode::new(server.load(path))
                .with_color(Color::srgba(1.0, 1.0, 1.0, 0.35))
                .with_mode(NodeImageMode::Stretch),
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

/// Along the top: the seats, the clock over STRATEGY TIME in the middle (Dota's `#Header`).
fn header(
    commands: &mut Commands,
    kit: &Kit,
    core: &bota_client_core::App,
    colours: &PlayerColours,
    root: Entity,
) {
    bar::spawn(commands, kit, bar::Style::Strategy, core, colours, root);
    let shot = STRATEGY;
    let centre = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                margin: UiRect::left(shot.val(-r::CLOCK_WIDTH / 2.0)),
                top: shot.val(r::STRATEGY_CLOCK[0]),
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
        Clock,
        shot.text(
            kit,
            "",
            (Face::Regular, r::STRATEGY_CLOCK[2], 0.0),
            Color::WHITE,
        ),
        Node {
            height: shot.val(r::STRATEGY_CLOCK[1]),
            ..default()
        },
        ChildOf(centre),
    ));
    commands.spawn((
        shot.text(
            kit,
            kit.word("DOTA_Hero_Selection_StrategyTime", "Strategy Time"),
            (Face::Semibold, r::CLOCK_CAPTION, 1.0),
            Color::WHITE,
        ),
        ChildOf(centre),
    ));
}

/// At the left: ENTERING BATTLE AS, the hero's name, its skills, and its model on the dark stage
/// (Dota's `#SelectedHeroDetails`).
fn details(commands: &mut Commands, kit: &Kit, hero: u16, root: Entity) {
    let Some(def) = bota_core::hero_def(bota_proto::HeroId(hero)) else {
        return;
    };
    let shot = STRATEGY;
    stage_light(commands, root, r::MODEL);
    let column = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: shot.val(r::ABILITY_ROW_LEFT),
                top: shot.val(r::ENTERING[1]),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        shot.text(
            kit,
            kit.word("DOTA_Hero_Selection_EnteringBattleAs", "Entering Battle As"),
            (Face::Regular, r::ENTERING[2], 2.0),
            BATTLE_INK,
        ),
        Node {
            margin: UiRect::left(shot.val(r::ENTERING[0] - r::ABILITY_ROW_LEFT)),
            ..default()
        },
        ChildOf(column),
    ));
    let name = commands
        .spawn((
            Node {
                height: shot.val(80.0),
                align_items: AlignItems::Center,
                column_gap: shot.val(12.0),
                margin: UiRect::left(shot.val(r::ENTERING[0] - r::ABILITY_ROW_LEFT)),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    if let Some(mark) = panel::Facts::of(def).primary_mark() {
        commands.spawn((
            Node {
                width: shot.val(r::ENTERING_ATTRIBUTE),
                height: shot.val(r::ENTERING_ATTRIBUTE),
                ..default()
            },
            kit.art(mark),
            ChildOf(name),
        ));
    }
    commands.spawn((
        shot.text(
            kit,
            def.name.to_uppercase(),
            (NAME_FACE, r::ENTERING[3], 1.0),
            Color::WHITE,
        ),
        TextShadow {
            offset: Vec2::new(0.0, 2.0),
            color: Color::BLACK,
        },
        ChildOf(name),
    ));
    let skills = panel::skills(commands, kit, shot, panel::DETAIL_SKILLS, def, column);
    commands
        .entity(skills)
        .entry::<Node>()
        .and_modify(move |mut node| {
            node.margin = UiRect::top(shot.val(3.0));
        });
    let [height, feet, middle] = r::MODEL;
    crate::main_menu::hero_stage::stand(
        commands,
        root,
        bota_proto::HeroId(hero),
        &crate::main_menu::hero_stage::Stand {
            middle,
            feet: shot.size(feet * r::STRATEGY_HEIGHT),
            height: shot.size(height * r::STRATEGY_HEIGHT),
        },
    );
}

/// At the right: STARTING ITEMS (Dota's `#StartingItems`), the gold that
/// remains, the inventory, and the shop's basic items.
fn starting_items(commands: &mut Commands, kit: &Kit, catalog: &Catalog, root: Entity) {
    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: STRATEGY
                    .val(r::STRATEGY_WIDTH - r::STARTING_ITEMS[0] - r::STARTING_ITEMS[2]),
                top: STRATEGY.val(r::STARTING_ITEMS[1]),
                width: dota(RIGHT_WIDTH),
                min_height: dota(340.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            Spot("strategy starting items"),
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
        children![STRATEGY.text(
            kit,
            kit.word("DOTA_Item_Build_Starting_Items", "Starting Items"),
            (Face::Semibold, 24.0, 4.0),
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
                width: dota(inventory_width()),
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
        STRATEGY.text(kit, "", (Face::Bold, 40.0, 0.0), GOLD_INK),
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
                width: percent(100),
                padding: UiRect::all(dota(BOX_PADDING)),
                row_gap: dota(SLOT_GAP),
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
        let row = commands
            .spawn((
                Node {
                    column_gap: dota(SLOT_GAP),
                    ..default()
                },
                ChildOf(inventory),
            ))
            .id();
        for at in slots {
            inventory_slot(commands, kit, row, at, size);
        }
    }
}

/// The inventory box's width: its widest row of slots and the padding round it.
fn inventory_width() -> f32 {
    3.0 * INVENTORY.0 + 2.0 * SLOT_GAP + 2.0 * BOX_PADDING
}

/// One slot of the inventory: the bottom panel's own item box, dark until a
/// choice fills it.
fn inventory_slot(commands: &mut Commands, kit: &Kit, row: Entity, at: usize, size: (f32, f32)) {
    crate::hud::spawn_slot_box(
        commands,
        kit.fonts,
        row,
        (size.0 * ui::DOTA, size.1 * ui::DOTA),
        crate::hud::ItemBox(at as u8),
        false,
        false,
    );
}

/// The shop's basic items: each category a column of its own, its title over a grid four items
/// wide, side by side as the reference shows them.
fn item_lists(commands: &mut Commands, kit: &Kit, catalog: &Catalog, contents: Entity) {
    let row = commands
        .spawn((
            Node {
                flex_grow: 1.0,
                column_gap: dota(SECTIONS_GAP),
                align_items: AlignItems::FlexStart,
                ..default()
            },
            ChildOf(contents),
        ))
        .id();
    let art = (kit.fonts, kit.manifest, kit.server);
    for (key, plain) in SECTIONS {
        let items: Vec<_> = catalog.category(key).collect();
        if items.is_empty() {
            continue;
        }
        let column = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: dota(4.0),
                    ..default()
                },
                ChildOf(row),
            ))
            .id();
        commands.spawn((
            STRATEGY.text(
                kit,
                kit.word(&format!("dota_pregame_shop_{key}"), plain),
                (Face::Regular, 24.0, 3.0),
                TITLE_INK,
            ),
            ChildOf(column),
        ));
        let list = commands
            .spawn((
                Node {
                    width: dota(LIST_COLUMNS * LIST_ITEM.0 + (LIST_COLUMNS - 1.0) * 4.0),
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
        for id in items {
            spawn_cell(commands, art, list, id, Place::Grid, size);
        }
    }
}

/// A control's title (Dota's `.StrategyControlTitle`).
fn title(commands: &mut Commands, kit: &Kit, parent: Entity, key: &str, plain: &str) {
    commands.spawn((
        STRATEGY.text(
            kit,
            kit.word(key, plain),
            (Face::Regular, 24.0, 3.0),
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

/// The light of the hero's room, geometry only: a pool on the floor under the hero and a soft
/// beam from above, both about the model's middle, as the original game's stage has them.
fn stage_light(commands: &mut Commands, root: Entity, [height, feet, middle]: [f32; 3]) {
    let shot = STRATEGY;
    let tall = height * r::STRATEGY_HEIGHT;
    let ground = feet * r::STRATEGY_HEIGHT;
    let glow = |alpha: f32| {
        BackgroundGradient::from(RadialGradient::new(
            UiPosition::CENTER,
            RadialGradientShape::Ellipse(percent(50), percent(50)),
            vec![
                ColorStop::new(Color::srgba(0.75, 0.82, 0.95, alpha), percent(0)),
                ColorStop::new(Color::NONE, percent(100)),
            ],
        ))
    };
    for (name, across, high, centre, alpha) in [
        (
            "strategy floor light",
            1.5 * tall,
            0.36 * tall,
            ground - 0.08 * tall,
            0.20,
        ),
        (
            "strategy beam",
            0.9 * tall,
            1.15 * tall,
            ground - 0.62 * tall,
            0.16,
        ),
    ] {
        commands.spawn((
            Name::new(name),
            Node {
                position_type: PositionType::Absolute,
                left: percent(middle * 100.0),
                margin: UiRect::left(shot.val(-across / 2.0)),
                top: shot.val(centre - high / 2.0),
                width: shot.val(across),
                height: shot.val(high),
                ..default()
            },
            glow(alpha),
            Pickable::IGNORE,
            ChildOf(root),
        ));
    }
}
