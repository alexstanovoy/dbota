//! Dota's HUD: the bottom panel with the portrait, the numbers, the health
//! and mana bars, the abilities and the items; the top bar with the clock and
//! both heroes; one's own numbers in the corner; the kill feed and the
//! passing notices.

use std::f32::consts::TAU;

use bevy::prelude::*;
use bota_client_core::{
    App as Core, BAG_SLOTS, ClientSession, INVENTORY_SLOTS, Look, TOWN_PORTAL_SLOT, ability_look,
    item_look,
};
use bota_proto::{AbilityView, ItemView, Kit, PlayerView, Team, UnitKind, UnitView};

use crate::config::Settings;
use crate::keys::Action;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Extent, Fonts};

// The hero panel's measures, in interface pixels: Dota's at 720 lines
// times 5/3, so that an ability box is ours.

/// The size of an ability box.
const ABILITY: f32 = 58.0;
/// The size of an item box.
const ITEM: (f32, f32) = (66.0, 48.0);
/// The height of a backpack box.
const BACKPACK_ROW: f32 = 30.0;
/// The portrait's frame, of Dota's shape.
pub const PORTRAIT: (f32, f32) = (176.0, 156.0);
/// The ring round the level.
const LEVEL_RING: f32 = 42.0;
/// The innate's round mark.
const INNATE: f32 = 34.0;
/// The level-up plate over an ability.
const LEVEL_UP: (f32, f32) = (ABILITY, 30.0);
/// The height of the row of level pips under an ability, with its gap.
const PIPS: f32 = 10.0;
/// Pips an ability box has room for.
const MAX_PIPS: u8 = 4;
/// The height of the health and the mana bar.
const POOL: f32 = 24.0;
/// The round neutral and town portal slots.
const ROUND_SLOT: f32 = 46.0;
/// The panel's padding and the gaps of the ability row.
const GAP: f32 = 6.0;
/// A slot box's edge.
const SLOT_EDGE: Color = Color::srgb(0.2, 0.22, 0.27);
/// The gold of the name over the portrait.
const NAME_GOLD: Color = Color::srgb(0.93, 0.74, 0.4);

/// An ability box of the bottom panel, by slot.
#[derive(Component, Clone, Copy)]
pub struct AbilityBox(pub u8);

/// An item box of the bottom panel or the stash, by slot: bag below
/// [`BAG_SLOTS`], stash from it, then the town portal slot.
#[derive(Component, Clone, Copy)]
pub struct ItemBox(pub u8);

/// A seat's portrait in the top bar, by seat.
#[derive(Component, Clone, Copy)]
pub struct SeatBox(pub u8);

/// Dota's level-up plate over an ability box, by slot.
#[derive(Component, Clone, Copy)]
struct LevelUp(u8);

/// The innate ability's round mark beside the abilities.
#[derive(Component)]
pub struct InnateButton;

/// The neutral item's round slot, empty as the simulator has no neutral
/// items.
#[derive(Component)]
struct NeutralSlot;

/// What a part of a slot box shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Part {
    Icon,
    Sweep,
    Seconds,
    Key,
    Cost,
    Pip(u8),
    Shade,
    Charges,
}

/// The parent box of a part, to find a part's slot.
#[derive(Component, Clone, Copy)]
struct PartOf(Entity);

/// The sun or the moon beside the clock, as Dota's top bar shows the time
/// of day.
#[derive(Component)]
struct DayNight;

/// A text the HUD keeps up to date, by what it says.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Readout {
    Clock,
    Score(Team),
    PanelName,
    PanelLevel,
    Hp,
    Mana,
    HpRegen,
    ManaRegen,
    LastHits,
    Kda,
    Feed,
    Notice,
    Fps,
    SeatLevel(u8),
    SeatDead(u8),
}

/// A bar the HUD keeps filled.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Gauge {
    Hp,
    Mana,
    SeatHp(u8),
}

/// The portrait image of the bottom panel: Dota's picture of the hero, or
/// the background of a render of the unit's model, either cut to the frame
/// as Dota cuts it ([`ui::Cover`]).
#[derive(Component)]
struct Portrait;

/// The render of the unit's model in the bottom panel's portrait, over its
/// background.
#[derive(Component)]
struct PortraitRender;

/// The ring round the panel's level, filled with the experience gathered
/// towards the next.
#[derive(Component)]
struct XpRing;

/// The share of the way from a level to the next a hero's experience is.
fn xp_share(level: u8, xp: i32) -> f32 {
    let at = usize::from(level.max(1)) - 1;
    match (
        bota_core::rules::XP_THRESHOLDS.get(at),
        bota_core::rules::XP_THRESHOLDS.get(at + 1),
    ) {
        (Some(&from), Some(&to)) => {
            ((i64::from(xp) - i64::from(from)) as f32 / (to - from).max(1) as f32).clamp(0.0, 1.0)
        }
        _ => 1.0,
    }
}

/// The level ring, gold over the share of the way to the next level.
fn xp_ring(share: f32) -> BackgroundGradient {
    let gold = Color::srgb(0.95, 0.75, 0.3);
    let empty = Color::srgba(0.1, 0.1, 0.12, 0.9);
    ui::split_disc(share.clamp(0.0, 1.0) * TAU, gold, empty)
}

/// Fills the level ring with the panel hero's experience; no ring for what
/// has no level.
fn xp(
    session: NonSend<Session>,
    mut ring: Single<(&mut BackgroundGradient, &mut Visibility), With<XpRing>>,
) {
    let core = &session.0;
    let (seat, unit, _) = subject(core);
    let share = seat.map_or(0.0, |p| xp_share(p.level, p.xp));
    ring.0.set_if_neq(xp_ring(share));
    ring.1
        .set_if_neq(ui::visible(unit.is_some_and(|u| u.level > 0)));
}

/// The whole HUD, hidden in the lobby.
#[derive(Component)]
pub struct HudRoot;

/// A part of the HUD that swallows clicks meant for the world.
#[derive(Component)]
pub struct Blocker;

/// The two docks of the bottom row, which other parts of the interface fill:
/// the minimap's at the left and the shop's at the right.
#[derive(Resource, Clone, Copy)]
pub struct Docks {
    /// Where the minimap goes.
    pub minimap: Entity,
    /// The column of buttons attached to the minimap's right side.
    pub minimap_buttons: Entity,
    /// The shop's corner.
    pub right: Entity,
    /// The courier's buttons, which the shop's corner places right of the
    /// gold, as Dota's corner has them.
    pub courier: Entity,
    root: Entity,
    row: Entity,
}

pub fn plugin(app: &mut App) {
    app.add_systems(PreStartup, spawn_docks)
        .add_systems(Startup, spawn_hud)
        .add_systems(
            Update,
            (
                readouts,
                gauges,
                slots,
                portrait,
                time_of_day,
                seats,
                xp,
                level_ups,
                innate,
            )
                .run_if(|k: Res<crate::session::MatchKnown>| k.0),
        );
}

/// The bottom row is a grid of three columns: the docks take their own
/// width and share the rest equally, so the hero panel between them is
/// centred while the window has room and shifts, never overlaps, when not.
fn spawn_docks(mut commands: Commands) {
    let root = commands.spawn((HudRoot, ui::fill(), Pickable::IGNORE)).id();
    let row = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                width: percent(100),
                display: Display::Grid,
                grid_template_columns: vec![
                    GridTrack::minmax(
                        MinTrackSizingFunction::MinContent,
                        MaxTrackSizingFunction::Fraction(1.0),
                    ),
                    GridTrack::max_content(),
                    GridTrack::minmax(
                        MinTrackSizingFunction::MinContent,
                        MaxTrackSizingFunction::Fraction(1.0),
                    ),
                ],
                align_items: AlignItems::End,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    let dock = |commands: &mut Commands, column: i16, justify: JustifySelf, extent: Extent| {
        commands
            .spawn((
                extent,
                Node {
                    grid_column: GridPlacement::start(column),
                    grid_row: GridPlacement::start(1),
                    justify_self: justify,
                    flex_direction: FlexDirection::Row,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row),
            ))
            .id()
    };
    let left = dock(&mut commands, 1, JustifySelf::Start, Extent::Left);
    let right = dock(&mut commands, 3, JustifySelf::End, Extent::Right);
    let minimap = commands
        .spawn((
            Node {
                margin: UiRect::new(px(6), px(0), px(0), px(6)),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(left),
        ))
        .id();
    let minimap_buttons = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::FlexEnd,
                margin: UiRect::bottom(px(6)),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(left),
        ))
        .id();
    let courier = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    commands.insert_resource(Docks {
        minimap,
        minimap_buttons,
        right,
        courier,
        root,
        row,
    });
}

fn spawn_hud(
    mut commands: Commands,
    fonts: Res<Fonts>,
    docks: Res<Docks>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    texts: Res<crate::dota_text::Texts>,
) {
    let mut kit = Spawner {
        commands: &mut commands,
        fonts: &fonts,
        manifest: &manifest,
        server: &server,
        texts: &texts,
    };
    spawn_bottom(&mut kit, docks.row);
    spawn_top(kit.commands, &fonts, docks.root);
    spawn_corner(kit.commands, &fonts, docks.root);
}

/// What spawning the hero panel needs.
pub(crate) struct Spawner<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub fonts: &'a Fonts,
    pub manifest: &'a Manifest,
    pub server: &'a AssetServer,
    pub texts: &'a crate::dota_text::Texts,
}

/// A dark panel that blocks world clicks.
fn panel(node: Node) -> impl Bundle {
    (
        node,
        BackgroundColor(ui::PANEL),
        BorderColor::all(ui::EDGE),
        Interaction::default(),
        Blocker,
    )
}

/// The hero panel, in the middle column of the bottom row, laid out as
/// Dota's: the level and the portrait with the numbers over it, the
/// abilities over the health and mana bars, the inventory with its round
/// slots.
fn spawn_bottom(kit: &mut Spawner, row: Entity) {
    let bar = kit
        .commands
        .spawn((
            Extent::Bar,
            panel(Node {
                grid_column: GridPlacement::start(2),
                grid_row: GridPlacement::start(1),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexEnd,
                padding: UiRect::all(px(GAP)),
                column_gap: px(10),
                border: UiRect::top(px(2)).with_left(px(2)).with_right(px(2)),
                border_radius: BorderRadius::top(px(10)),
                ..default()
            }),
            ChildOf(row),
        ))
        .id();
    spawn_level(kit, bar);
    spawn_portrait(kit, bar);
    spawn_abilities(kit, bar);
    spawn_items(kit, bar);
}

/// The level in a ring that fills with the experience to the next, left of
/// the portrait's foot.
fn spawn_level(kit: &mut Spawner, bar: Entity) {
    let ring = kit
        .commands
        .spawn((
            XpRing,
            Node {
                width: px(LEVEL_RING),
                height: px(LEVEL_RING),
                margin: UiRect::right(px(-4)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            xp_ring(0.0),
            ChildOf(bar),
        ))
        .id();
    let disc = kit
        .commands
        .spawn((
            Node {
                width: px(LEVEL_RING - 8.0),
                height: px(LEVEL_RING - 8.0),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.035, 0.95)),
            ChildOf(ring),
        ))
        .id();
    kit.commands.spawn((
        Readout::PanelLevel,
        ui::text("", &kit.fonts.bold, 17.0, ui::GOLDEN),
        ChildOf(disc),
    ));
}

/// The portrait, filled with its picture as Dota fills it, the unit's name
/// on a banner over its top, and the numbers over its right side and foot.
fn spawn_portrait(kit: &mut Spawner, bar: Entity) {
    let portrait = kit
        .commands
        .spawn((
            Portrait,
            Node {
                width: px(PORTRAIT.0),
                height: px(PORTRAIT.1),
                border: UiRect::all(px(2)),
                ..default()
            },
            BorderColor::all(ui::EDGE),
            BackgroundColor(ui::PANEL_DEEP),
            ui::Cover,
            ChildOf(bar),
        ))
        .id();
    kit.commands.spawn((
        PortraitRender,
        ui::fill(),
        ui::Cover,
        Visibility::Hidden,
        ChildOf(portrait),
    ));
    let banner = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: px(-2),
                margin: UiRect::bottom(px(2)),
                padding: UiRect::axes(px(8), px(2)),
                border_radius: BorderRadius::top(px(4)),
                ..default()
            },
            BackgroundColor(ui::PANEL),
            Pickable::IGNORE,
            ChildOf(portrait),
        ))
        .id();
    kit.commands.spawn((
        Readout::PanelName,
        ui::text("", &kit.fonts.bold, 15.0, NAME_GOLD),
        ui::shadow(),
        TextLayout::no_wrap(),
        ChildOf(banner),
    ));
    crate::stats::spawn(kit.commands, kit.fonts, kit.manifest, kit.server, portrait);
}

/// The abilities with the talent tree and the innate before them, the
/// level-up plates over them and the buffs over those, and the health and
/// mana bars under them.
fn spawn_abilities(kit: &mut Spawner, bar: Entity) {
    let column = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                height: px(PORTRAIT.1),
                ..default()
            },
            ChildOf(bar),
        ))
        .id();
    let float = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: px(0),
                margin: UiRect::bottom(px(LEVEL_UP.1 + 2.0 * GAP)),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(column),
        ))
        .id();
    crate::buffs::spawn_row(kit.commands, kit.fonts, float);
    let row = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexStart,
                column_gap: px(GAP),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    crate::talents::spawn_button(kit, row, column);
    spawn_innate(kit, row);
    for slot in 0..6u8 {
        spawn_ability_cell(kit, row, slot);
    }
    spawn_pools(kit, column);
}

/// The innate ability's round mark between the talent tree and the
/// abilities, as Dota shows it; hovered, its tooltip.
fn spawn_innate(kit: &mut Spawner, row: Entity) {
    let button = kit
        .commands
        .spawn((
            InnateButton,
            Button,
            Node {
                width: px(INNATE),
                height: px(INNATE),
                align_self: AlignSelf::Center,
                margin: UiRect::bottom(px(PIPS)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::srgb(0.32, 0.33, 0.36)),
            BackgroundColor(ui::PANEL_DEEP),
            Visibility::Hidden,
            ChildOf(row),
        ))
        .id();
    kit.commands.spawn((
        Node {
            width: percent(70),
            height: percent(70),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        ui::art(
            kit.manifest,
            kit.server,
            "hud.innate",
            Color::srgb(0.75, 0.6, 0.3),
        ),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// One ability: its box, its level pips under it, and Dota's level-up plate
/// over it while a point could go into it.
fn spawn_ability_cell(kit: &mut Spawner, row: Entity, slot: u8) {
    let cell = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(3),
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    let ability = spawn_slot_box(
        kit.commands,
        kit.fonts,
        cell,
        (ABILITY, ABILITY),
        AbilityBox(slot),
        true,
        false,
    );
    let pips = kit
        .commands
        .spawn((
            Node {
                height: px(PIPS - 3.0),
                column_gap: px(3),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(cell),
        ))
        .id();
    for pip in 0..MAX_PIPS {
        kit.commands.spawn((
            Part::Pip(pip),
            PartOf(ability),
            Node {
                width: px(9),
                height: px(PIPS - 3.0),
                border_radius: BorderRadius::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
            ChildOf(pips),
        ));
    }
    spawn_level_up(kit, cell, slot);
}

/// Dota's gold plate with a plus over an ability a point could go into.
fn spawn_level_up(kit: &mut Spawner, cell: Entity, slot: u8) {
    let plate = kit
        .commands
        .spawn((
            LevelUp(slot),
            Button,
            Blocker,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                width: px(LEVEL_UP.0),
                height: px(LEVEL_UP.1),
                margin: UiRect::bottom(px(GAP)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            ui::art(
                kit.manifest,
                kit.server,
                "hud.levelup_button",
                Color::srgb(0.85, 0.7, 0.35),
            ),
            Visibility::Hidden,
            ChildOf(cell),
        ))
        .id();
    match kit.manifest.file("hud.levelup_well") {
        Some(path) => {
            kit.commands.spawn((
                Node {
                    width: px(LEVEL_UP.1 - 6.0),
                    height: px(LEVEL_UP.1 - 6.0),
                    ..default()
                },
                ImageNode::new(kit.server.load(path)),
                Pickable::IGNORE,
                ChildOf(plate),
            ));
        }
        None => {
            kit.commands.spawn((
                ui::text("+", &kit.fonts.bold, 24.0, Color::srgb(0.3, 0.2, 0.05)),
                Pickable::IGNORE,
                ChildOf(plate),
            ));
        }
    }
}

/// The health and mana bars under the abilities, from the talent tree to
/// the last ability, the pool in the middle and the regeneration at the
/// right.
fn spawn_pools(kit: &mut Spawner, column: Entity) {
    let pools = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    for (gauge, readout, regen, color) in [
        (Gauge::Hp, Readout::Hp, Readout::HpRegen, ui::HP_FRIEND),
        (Gauge::Mana, Readout::Mana, Readout::ManaRegen, ui::MANA),
    ] {
        let well = kit
            .commands
            .spawn((
                Node {
                    height: px(POOL),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                ChildOf(pools),
            ))
            .id();
        kit.commands.spawn((
            gauge,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                height: percent(100),
                width: percent(100),
                ..default()
            },
            BackgroundColor(color),
            ChildOf(well),
        ));
        kit.commands.spawn((
            readout,
            ui::text("", &kit.fonts.bold, POOL - 6.0, ui::INK),
            ui::shadow(),
            ChildOf(well),
        ));
        kit.commands.spawn((
            regen,
            Node {
                position_type: PositionType::Absolute,
                right: px(6),
                ..default()
            },
            ui::text("", &kit.fonts.bold, POOL - 9.0, ui::INK),
            ui::shadow(),
            ChildOf(well),
        ));
    }
}

/// The inventory's two rows of three, the backpack row under them, and the
/// round neutral and town portal slots beside them. The stash is the shop's.
fn spawn_items(kit: &mut Spawner, bar: Entity) {
    let grid = kit
        .commands
        .spawn((
            Node {
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::px(3, ITEM.0),
                column_gap: px(5),
                row_gap: px(5),
                ..default()
            },
            ChildOf(bar),
        ))
        .id();
    for slot in 0..BAG_SLOTS {
        let size = if slot >= INVENTORY_SLOTS {
            (ITEM.0, BACKPACK_ROW)
        } else {
            ITEM
        };
        spawn_slot_box(
            kit.commands,
            kit.fonts,
            grid,
            size,
            ItemBox(slot),
            slot < INVENTORY_SLOTS,
            false,
        );
    }
    spawn_round_slots(kit, bar);
}

/// Dota's round slots right of the inventory: the neutral item's on top
/// (the simulator has no neutral items: it stays empty) and the town
/// portal's level with the backpack.
fn spawn_round_slots(kit: &mut Spawner, bar: Entity) {
    let column = kit
        .commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                height: px(2.0 * ITEM.1 + BACKPACK_ROW + 10.0),
                ..default()
            },
            ChildOf(bar),
        ))
        .id();
    let neutral = kit
        .commands
        .spawn((
            NeutralSlot,
            Interaction::default(),
            Blocker,
            Node {
                width: px(ROUND_SLOT),
                height: px(ROUND_SLOT),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BorderColor::all(SLOT_EDGE),
            BackgroundColor(ui::PANEL_DEEP),
            ChildOf(column),
        ))
        .id();
    kit.commands.spawn((
        Node {
            border_radius: BorderRadius::MAX,
            ..ui::fill()
        },
        ui::art(kit.manifest, kit.server, "hud.neutral_slot", Color::NONE),
        Pickable::IGNORE,
        ChildOf(neutral),
    ));
    let portal = spawn_slot_box(
        kit.commands,
        kit.fonts,
        column,
        (ROUND_SLOT, ROUND_SLOT),
        ItemBox(TOWN_PORTAL_SLOT),
        true,
        true,
    );
    if let Some(path) = kit.manifest.file("hud.tp_slot") {
        // The empty slot's own art, under the scroll's icon.
        kit.commands.spawn((
            Node {
                border_radius: BorderRadius::MAX,
                ..ui::fill()
            },
            ImageNode::new(kit.server.load(path)),
            ZIndex(-1),
            Pickable::IGNORE,
            ChildOf(portal),
        ));
    }
}

/// One clickable slot box with its icon, its wait and its labels; `round`
/// for Dota's round slots.
pub(crate) fn spawn_slot_box(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    (w, h): (f32, f32),
    tag: impl Component,
    keyed: bool,
    round: bool,
) -> Entity {
    let radius = if round {
        BorderRadius::MAX
    } else {
        BorderRadius::ZERO
    };
    let slot = commands
        .spawn((
            tag,
            Button,
            Node {
                width: px(w),
                height: px(h),
                border: UiRect::all(px(1)),
                border_radius: radius,
                ..default()
            },
            BorderColor::all(SLOT_EDGE),
            BackgroundColor(ui::PANEL_DEEP),
            ChildOf(parent),
        ))
        .id();
    let face = |part: Part| (part, PartOf(slot), Pickable::IGNORE, ChildOf(slot));
    let rounded = Node {
        border_radius: radius,
        ..ui::fill()
    };
    commands.spawn((face(Part::Icon), rounded.clone(), ImageNode::default()));
    commands.spawn((
        face(Part::Shade),
        rounded.clone(),
        BackgroundColor(Color::NONE),
    ));
    commands.spawn((
        face(Part::Sweep),
        rounded,
        // Nothing waits yet: clear all round (`dress` turns it).
        ui::split_disc(TAU, Color::NONE, Color::srgba(0.0, 0.0, 0.0, 0.0)),
    ));
    spawn_slot_labels(commands, fonts, slot, keyed);
    slot
}

/// The seconds in the middle of a slot box, its key, its cost and its
/// charges.
fn spawn_slot_labels(commands: &mut Commands, fonts: &Fonts, slot: Entity, keyed: bool) {
    let at = |left: Option<f32>, top: Option<f32>, right: Option<f32>, bottom: Option<f32>| Node {
        position_type: PositionType::Absolute,
        left: left.map_or(Val::Auto, px),
        top: top.map_or(Val::Auto, px),
        right: right.map_or(Val::Auto, px),
        bottom: bottom.map_or(Val::Auto, px),
        ..default()
    };
    let mut label = |part: Part, node: Node, size: f32, color: Color| {
        commands.spawn((
            part,
            PartOf(slot),
            ui::text("", &fonts.bold, size, color),
            ui::shadow(),
            node,
            Pickable::IGNORE,
            ChildOf(slot),
        ));
    };
    if keyed {
        label(
            Part::Key,
            at(Some(3.0), Some(1.0), None, None),
            12.0,
            ui::INK,
        );
    }
    label(
        Part::Cost,
        at(None, None, Some(3.0), Some(2.0)),
        12.0,
        Color::srgb(0.55, 0.75, 1.0),
    );
    label(
        Part::Charges,
        at(None, None, Some(3.0), Some(1.0)),
        13.0,
        ui::INK,
    );
    let centre = commands
        .spawn((
            Node {
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..ui::fill()
            },
            Pickable::IGNORE,
            ChildOf(slot),
        ))
        .id();
    commands.spawn((
        Part::Seconds,
        PartOf(slot),
        ui::text("", &fonts.bold, 22.0, ui::INK),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(centre),
    ));
}

/// The clock and the seats along the top.
fn spawn_top(commands: &mut Commands, fonts: &Fonts, root: Entity) {
    let top = commands
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
    let top = commands
        .spawn((
            Extent::TopCentre,
            Node {
                align_items: AlignItems::FlexStart,
                column_gap: px(6),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(top),
        ))
        .id();
    spawn_seat(commands, fonts, top, 0);
    let clock = commands
        .spawn((
            panel(Node {
                width: px(178),
                height: px(54),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(10)),
                border: UiRect::bottom(px(2)).with_left(px(2)).with_right(px(2)),
                border_radius: BorderRadius::bottom(px(8)),
                ..default()
            }),
            ChildOf(top),
        ))
        .id();
    commands.spawn((
        Readout::Score(Team::Radiant),
        ui::text(
            "0",
            &fonts.bold,
            20.0,
            crate::overlay::team_color(Team::Radiant),
        ),
        ChildOf(clock),
    ));
    let time = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(5),
                ..default()
            },
            ChildOf(clock),
        ))
        .id();
    commands.spawn((
        DayNight,
        Node {
            width: px(24),
            height: px(24),
            ..default()
        },
        ImageNode::default(),
        ChildOf(time),
    ));
    commands.spawn((
        Readout::Clock,
        ui::text("0:00", &fonts.bold, 22.0, ui::INK),
        ChildOf(time),
    ));
    commands.spawn((
        Readout::Score(Team::Dire),
        ui::text(
            "0",
            &fonts.bold,
            20.0,
            crate::overlay::team_color(Team::Dire),
        ),
        ChildOf(clock),
    ));
    spawn_seat(commands, fonts, top, 1);
}

/// One seat's box in the top bar: portrait, level, health, score.
fn spawn_seat(commands: &mut Commands, fonts: &Fonts, parent: Entity, seat: u8) {
    let column = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: px(2),
                margin: UiRect::top(px(4)),
                ..default()
            },
            ChildOf(parent),
        ))
        .id();
    let card = commands
        .spawn((
            SeatBox(seat),
            Button,
            Node {
                width: px(86),
                height: px(48),
                border: UiRect::all(px(2)),
                ..default()
            },
            BorderColor::all(ui::EDGE),
            BackgroundColor(ui::PANEL_DEEP),
            ImageNode::default(),
            ChildOf(column),
        ))
        .id();
    commands.spawn((
        Readout::SeatLevel(seat),
        ui::text("", &fonts.bold, 15.0, ui::GOLDEN),
        ui::shadow(),
        Node {
            position_type: PositionType::Absolute,
            right: px(3),
            bottom: px(1),
            ..default()
        },
        ChildOf(card),
    ));
    commands.spawn((
        Readout::SeatDead(seat),
        ui::text("", &fonts.bold, 22.0, ui::INK),
        ui::shadow(),
        Node {
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..ui::fill()
        },
        BackgroundColor(Color::NONE),
        ChildOf(card),
    ));
    let well = commands
        .spawn((
            Node {
                width: px(86),
                height: px(6),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
            ChildOf(column),
        ))
        .id();
    commands.spawn((
        Gauge::SeatHp(seat),
        Node {
            height: percent(100),
            width: percent(100),
            ..default()
        },
        BackgroundColor(crate::overlay::team_color(if seat == 0 {
            Team::Radiant
        } else {
            Team::Dire
        })),
        ChildOf(well),
    ));
}

/// One's own numbers in the top-left corner, the feed and the frame rate in
/// the top-right, and the notices in the middle.
fn spawn_corner(commands: &mut Commands, fonts: &Fonts, root: Entity) {
    let plate = commands
        .spawn((
            Extent::Corner,
            panel(Node {
                position_type: PositionType::Absolute,
                left: px(ui::CORNER_MARGIN),
                top: px(ui::CORNER_MARGIN),
                flex_direction: FlexDirection::Row,
                column_gap: px(16),
                padding: UiRect::axes(px(12), px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            }),
            ChildOf(root),
        ))
        .id();
    for (label, readout, color) in [
        ("LH / DN", Readout::LastHits, ui::INK),
        ("K / D / A", Readout::Kda, ui::INK),
    ] {
        let cell = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                ChildOf(plate),
            ))
            .id();
        commands.spawn((
            ui::text(label, &fonts.bold, 10.0, ui::INK_DIM),
            ChildOf(cell),
        ));
        commands.spawn((
            readout,
            ui::text("-", &fonts.bold, 20.0, color),
            ChildOf(cell),
        ));
    }
    commands.spawn((
        Readout::Fps,
        ui::text("", &fonts.regular, 11.0, ui::INK_DIM),
        Node {
            position_type: PositionType::Absolute,
            right: px(10),
            top: px(6),
            ..default()
        },
        ChildOf(root),
    ));
    commands.spawn((
        Readout::Feed,
        ui::text("", &fonts.regular, 14.0, ui::INK),
        ui::shadow(),
        TextLayout::justify(Justify::Right),
        Node {
            position_type: PositionType::Absolute,
            right: px(10),
            top: px(30),
            ..default()
        },
        ChildOf(root),
    ));
    let middle = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(28),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Readout::Notice,
        ui::text("", &fonts.bold, 20.0, ui::INK),
        ui::shadow(),
        TextLayout::justify(Justify::Center),
        ChildOf(middle),
    ));
}

/// The seat, unit and kit the bottom panel is about.
pub(crate) fn subject(core: &Core) -> (Option<&PlayerView>, Option<&UnitView>, Option<&Kit>) {
    let Some(view) = core.view() else {
        return (None, None, None);
    };
    if let Some((unit, _)) = core.selected.and_then(|id| core.known(id))
        && unit.kind != UnitKind::Hero
    {
        return (None, Some(unit), None);
    }
    let Some(p) = core.panel_slot().and_then(|s| view.player(s)) else {
        return (None, None, None);
    };
    let unit = core
        .body_of(p.slot)
        .and_then(|id| core.known(id))
        .map(|(u, _)| u);
    (Some(p), unit, p.kit.as_ref())
}

/// Every text of the HUD.
fn readouts(
    session: NonSend<Session>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut texts: Query<(&Readout, &mut Text)>,
) {
    let core = &session.0;
    let (Some(view), Some(map)) = (core.view(), core.session().map(ClientSession::map)) else {
        return;
    };
    let rate = u32::from(core.tick_rate().max(1));
    let (p, unit, _) = subject(core);
    let mine = core.panel_slot().and_then(|s| view.player(s));
    for (readout, mut text) in &mut texts {
        let s = match *readout {
            Readout::Clock => clock(i64::from(view.tick) - i64::from(core.pregame_ticks()), rate),
            Readout::Score(team) => view
                .players
                .iter()
                .filter(|p| p.team == team)
                .map(|p| u32::from(p.kills))
                .sum::<u32>()
                .to_string(),
            // In capitals, as Dota's banner writes it.
            Readout::PanelName => p
                .and_then(|p| bota_core::hero_def(p.hero))
                .map(|hero| hero.name.to_string())
                .or(unit.map(|u| crate::units::unit_name(map, u).to_string()))
                .unwrap_or_default()
                .to_uppercase(),
            Readout::PanelLevel => unit
                .filter(|u| u.level > 0)
                .map(|u| u.level.to_string())
                .unwrap_or_default(),
            Readout::Hp => unit
                .map(|u| format!("{} / {}", u.hp.max(0), u.max_hp))
                .unwrap_or_default(),
            Readout::Mana => unit
                .filter(|u| u.max_mana > 0)
                .map(|u| format!("{} / {}", u.mana.max(0), u.max_mana))
                .unwrap_or_default(),
            Readout::HpRegen => unit.map(|u| regen_text(u.hp_regen)).unwrap_or_default(),
            Readout::ManaRegen => unit
                .filter(|u| u.max_mana > 0)
                .map(|u| regen_text(u.mana_regen))
                .unwrap_or_default(),
            Readout::LastHits => mine
                .map(|p| format!("{} / {}", p.last_hits, p.denies))
                .unwrap_or_default(),
            Readout::Kda => mine
                .map(|p| format!("{} / {} / {}", p.kills, p.deaths, p.assists))
                .unwrap_or_default(),
            Readout::Feed => core
                .feed
                .iter()
                .map(|f| f.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            Readout::Notice => notice(core, rate),
            Readout::Fps => {
                let fps = diagnostics
                    .get(&bevy::diagnostic::FrameTimeDiagnosticsPlugin::FPS)
                    .and_then(|d| d.smoothed())
                    .unwrap_or(0.0);
                let ping = core
                    .session()
                    .and_then(ClientSession::order_delay_ms)
                    .map_or("-".to_string(), |ms| format!("{ms} ms"));
                format!("ping {ping}   {fps:.0} fps")
            }
            Readout::SeatLevel(seat) => seat_of(view, seat)
                .map(|p| p.level.to_string())
                .unwrap_or_default(),
            Readout::SeatDead(seat) => seat_of(view, seat)
                .filter(|p| p.unit.is_none())
                .map(|p| p.respawn_left.div_ceil(rate).to_string())
                .unwrap_or_default(),
        };
        text.set_if_neq(Text(s));
    }
}

/// The game clock, negative before the horn.
fn clock(ticks: i64, rate: u32) -> String {
    let seconds = ticks.div_euclid(i64::from(rate));
    let sign = if seconds < 0 { "-" } else { "" };
    format!("{sign}{}:{:02}", seconds.abs() / 60, seconds.abs() % 60)
}

fn seat_of(view: &bota_proto::WorldView, seat: u8) -> Option<&PlayerView> {
    view.players.get(usize::from(seat))
}

/// The word of the moment in the middle of the screen.
fn notice(core: &Core, rate: u32) -> String {
    if core.lost {
        return "CONNECTION LOST".to_string();
    }
    if core.over.is_none() && core.source.is_redialing() {
        return "RECONNECTING...".to_string();
    }
    if let Some((winner, _)) = &core.over {
        return match winner {
            Team::Radiant => "RADIANT VICTORY".to_string(),
            Team::Dire => "DIRE VICTORY".to_string(),
            Team::Neutral => "DRAW".to_string(),
        };
    }
    let dead = core
        .my_slot
        .and_then(|s| core.view()?.player(s))
        .filter(|p| p.unit.is_none());
    if let Some(p) = dead {
        return format!("Respawn in {}", p.respawn_left.div_ceil(rate));
    }
    if let Some((reason, _)) = &core.reject {
        return reason.clone();
    }
    if let Some(line) = &core.console {
        return format!("> {line}_");
    }
    if core.attack_move_armed {
        return "ATTACK: click a target or a point".to_string();
    }
    if core.aiming.is_some() {
        return "Left-click to aim, right-click or Esc to cancel".to_string();
    }
    String::new()
}

/// The health and mana bars of the panel and the seats.
fn gauges(session: NonSend<Session>, mut gauges: Query<(&Gauge, &mut Node, &mut BackgroundColor)>) {
    let core = &session.0;
    let (_, unit, _) = subject(core);
    let Some(view) = core.view() else {
        return;
    };
    for (gauge, mut node, mut bg) in &mut gauges {
        let share = match *gauge {
            Gauge::Hp => unit.map_or(0.0, |u| u.hp.max(0) as f32 / u.max_hp.max(1) as f32),
            Gauge::Mana => unit.map_or(0.0, |u| u.mana.max(0) as f32 / u.max_mana.max(1) as f32),
            Gauge::SeatHp(seat) => seat_of(view, seat)
                .and_then(|p| p.unit)
                .and_then(|id| view.unit(id))
                .map_or(0.0, |u| u.hp.max(0) as f32 / u.max_hp.max(1) as f32),
        };
        let width = percent(share * 100.0);
        if node.width != width {
            node.width = width;
        }
        if *gauge == Gauge::Hp {
            let color = if share < 0.25 {
                ui::HP_FOE
            } else {
                ui::HP_FRIEND
            };
            bg.set_if_neq(BackgroundColor(color));
        }
    }
}

/// Everything a slot box shows of what is in it.
struct SlotFace {
    icon: Option<String>,
    seconds: String,
    look: Look,
    sweep: f32,
    key: String,
    cost: String,
    pips: (u8, u8),
    charges: String,
    no_mana: bool,
    frame: Color,
}

/// The abilities of the subject the bottom panel is about: what its tooltips describe too.
pub(crate) fn subject_abilities<'a>(
    unit: Option<&'a UnitView>,
    kit: Option<&'a Kit>,
) -> &'a [AbilityView] {
    kit.map(|k| k.abilities.as_slice())
        .or(unit.map(|u| u.abilities.as_slice()))
        .unwrap_or_default()
}

/// The item in a slot of the subject the bottom panel is about: what its tooltip describes too.
pub(crate) fn subject_item(
    core: &Core,
    unit: Option<&UnitView>,
    kit: Option<&Kit>,
    slot: u8,
) -> Option<ItemView> {
    if slot == TOWN_PORTAL_SLOT {
        match kit {
            Some(kit) => kit.town_portal,
            None => unit?.town_portal,
        }
    } else if slot >= BAG_SLOTS {
        let view = core.view()?;
        let p = view.player(core.my_slot?)?;
        *p.stash.as_ref()?.get(usize::from(slot - BAG_SLOTS))?
    } else {
        let items = kit
            .map(|k| k.items.as_slice())
            .or(unit.map(|u| u.items.as_slice()))?;
        *items.get(usize::from(slot))?
    }
}

/// Whether the panel's subject is one's own to command.
fn owns_panel(core: &Core, unit: Option<&UnitView>, kit: Option<&Kit>) -> bool {
    unit.is_some_and(|u| core.drives(u.id)) || (kit.is_some() && core.panel_slot() == core.my_slot)
}

/// Shows Dota's level-up plate over each ability one's own hero could put a
/// point into; a click on it learns a level.
fn level_ups(
    mut session: NonSendMut<Session>,
    mut plates: Query<(&LevelUp, &mut Visibility)>,
    clicks: Query<(&LevelUp, &Interaction), Changed<Interaction>>,
) {
    let core = &mut session.0;
    let (_, unit, kit) = subject(core);
    let own = owns_panel(core, unit, kit);
    let live = matches!(core.source, bota_client_core::Source::Live(_));
    let abilities = subject_abilities(unit, kit);
    let can = |slot: u8| {
        own && abilities
            .get(usize::from(slot))
            .is_some_and(|a| a.can_level)
    };
    for (plate, mut visibility) in &mut plates {
        visibility.set_if_neq(ui::visible(can(plate.0)));
    }
    let learn = clicks
        .iter()
        .find(|(plate, i)| live && **i == Interaction::Pressed && can(plate.0))
        .map(|(plate, _)| plate.0);
    if let Some(slot) = learn {
        crate::input::press(core, bota_client_core::Slot::Ability(slot), true);
    }
}

/// Shows the innate's mark for a hero that has an innate ability.
fn innate(session: NonSend<Session>, mut mark: Single<&mut Visibility, With<InnateButton>>) {
    let has = subject(&session.0)
        .1
        .and_then(|u| u.hero)
        .and_then(bota_core::hero_def)
        .is_some_and(|h| h.innate.is_some());
    mark.set_if_neq(ui::visible(has));
}

/// The face of an ability slot.
fn ability_face(
    core: &Core,
    manifest: &Manifest,
    settings: &Settings,
    unit: Option<&UnitView>,
    kit: Option<&Kit>,
    slot: u8,
) -> Option<SlotFace> {
    let abilities = subject_abilities(unit, kit);
    let a = abilities.get(usize::from(slot))?;
    let mana = if kit.is_some() {
        0
    } else {
        unit.map_or(0, |u| u.mana)
    };
    let look = ability_look(Some(a), mana);
    let own = owns_panel(core, unit, kit);
    let aiming = own && core.aiming == Some(bota_client_core::Slot::Ability(slot));
    let rate = u32::from(core.tick_rate().max(1));
    Some(SlotFace {
        seconds: if a.cooldown_left > 0 {
            a.cooldown_left.div_ceil(rate).to_string()
        } else {
            String::new()
        },
        icon: manifest.ability_icon(a.id.0),
        look,
        sweep: unit.map_or(0.0, |u| core.derived.wait_left(u.id, slot, a.cooldown_left)),
        key: if look.passive {
            String::new()
        } else {
            settings.label(Settings::ability_action(slot, abilities.len()))
        },
        cost: if a.level > 0 && a.mana_cost > 0 {
            a.mana_cost.to_string()
        } else {
            String::new()
        },
        pips: (a.level, a.max_level),
        charges: String::new(),
        no_mana: a.level > 0 && mana < a.mana_cost,
        frame: if aiming {
            ui::GOLDEN
        } else if look.toggled {
            Color::srgb(0.35, 0.95, 0.85)
        } else {
            Color::srgb(0.2, 0.22, 0.27)
        },
    })
}

/// The face of an item slot.
fn item_face(
    core: &Core,
    manifest: &Manifest,
    settings: &Settings,
    unit: Option<&UnitView>,
    kit: Option<&Kit>,
    slot: u8,
) -> Option<SlotFace> {
    let item = subject_item(core, unit, kit, slot)?;
    let mana = unit.map_or(0, |u| u.mana);
    let look = item_look(Some(&item), slot, mana);
    let held = core.held_item == Some(slot);
    let rate = u32::from(core.tick_rate().max(1));
    Some(SlotFace {
        seconds: if item.cooldown_left > 0 {
            item.cooldown_left.div_ceil(rate).to_string()
        } else {
            String::new()
        },
        icon: manifest.item_icon(item.id.0),
        look,
        sweep: unit.map_or(0.0, |u| {
            core.derived.wait_left(
                u.id,
                bota_client_core::ITEM_WAIT_BASE + slot,
                item.cooldown_left,
            )
        }),
        key: match item_action(slot) {
            Some(action) if !look.passive => settings.label(action),
            _ => String::new(),
        },
        cost: String::new(),
        pips: (0, 0),
        charges: item
            .charges
            .filter(|c| *c > 0)
            .map(|c| c.to_string())
            .unwrap_or_default(),
        no_mana: mana < item.mana_cost,
        frame: if held {
            ui::GOLDEN
        } else {
            Color::srgb(0.2, 0.22, 0.27)
        },
    })
}

/// The action whose key uses an item slot, if the slot has one.
fn item_action(slot: u8) -> Option<Action> {
    match slot {
        TOWN_PORTAL_SLOT => Some(Action::TownPortal),
        _ if slot < INVENTORY_SLOTS => Some(Action::Item(slot)),
        _ => None,
    }
}

/// Fills every slot box with what is in it.
fn slots(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    settings: Res<Settings>,
    server: Res<AssetServer>,
    mut boxes: Query<
        (
            Entity,
            Option<&AbilityBox>,
            Option<&ItemBox>,
            &mut BorderColor,
            &mut Visibility,
        ),
        Or<(With<AbilityBox>, With<ItemBox>)>,
    >,
    mut parts: Query<
        (
            &Part,
            &PartOf,
            Option<&mut ImageNode>,
            Option<&mut Text>,
            Option<&mut BackgroundColor>,
            Option<&mut BackgroundGradient>,
        ),
        Without<AbilityBox>,
    >,
    mut faces: Local<Vec<(Entity, Option<SlotFace>)>>,
) {
    let core = &session.0;
    let (_, unit, kit) = subject(core);
    faces.clear();
    for (entity, ability, item, mut border, mut visibility) in &mut boxes {
        let face = match (ability, item) {
            (Some(AbilityBox(slot)), _) => {
                ability_face(core, &manifest, &settings, unit, kit, *slot)
            }
            (_, Some(ItemBox(slot))) => item_face(core, &manifest, &settings, unit, kit, *slot),
            _ => None,
        };
        let show = face.is_some() || item.is_some();
        visibility.set_if_neq(ui::visible(show));
        let frame = face
            .as_ref()
            .map_or(Color::srgb(0.16, 0.18, 0.22), |f| f.frame);
        if border.top != frame {
            *border = BorderColor::all(frame);
        }
        faces.push((entity, face));
    }
    for (part, of, image, text, bg, gradient) in &mut parts {
        let Some((_, face)) = faces.iter().find(|(e, _)| *e == of.0) else {
            continue;
        };
        dress(part, face.as_ref(), &server, image, text, bg, gradient);
    }
}

/// Sets one part of a slot box from the slot's face.
fn dress(
    part: &Part,
    face: Option<&SlotFace>,
    server: &AssetServer,
    image: Option<Mut<ImageNode>>,
    text: Option<Mut<Text>>,
    bg: Option<Mut<BackgroundColor>>,
    gradient: Option<Mut<BackgroundGradient>>,
) {
    match (part, face) {
        (Part::Icon, f) => {
            if let Some(mut image) = image {
                let tint = match f {
                    // An ability the cache has no icon for.
                    Some(f) if f.icon.is_none() => Color::NONE,
                    Some(f) if !f.look.learned => Color::srgb(0.35, 0.35, 0.35),
                    Some(_) => Color::WHITE,
                    None => Color::NONE,
                };
                set_image(&mut image, server, f.and_then(|f| f.icon.clone()), tint);
            }
        }
        (Part::Shade, f) => {
            if let Some(mut bg) = bg {
                let c = match f {
                    Some(f) if !f.look.learned => Color::srgba(0.0, 0.0, 0.0, 0.55),
                    Some(f) if f.no_mana && !f.look.passive => Color::srgba(0.05, 0.12, 0.5, 0.45),
                    _ => Color::NONE,
                };
                bg.set_if_neq(BackgroundColor(c));
            }
        }
        (Part::Sweep, f) => {
            if let Some(mut g) = gradient {
                let left = f
                    .filter(|f| f.look.cooldown_left > 0)
                    .map_or(0.0, |f| f.sweep.max(0.02));
                let a = (1.0 - left) * TAU;
                let dark = Color::srgba(0.0, 0.0, 0.0, if left > 0.0 { 0.68 } else { 0.0 });
                // Touched only when the sweep moved: a changed gradient is
                // prepared for drawing again.
                let same = matches!(g.0.first(), Some(Gradient::Conic(conic))
                    if conic.stops[1].angle == Some(a) && conic.stops[2].color == dark);
                if !same && let Some(Gradient::Conic(conic)) = g.0.first_mut() {
                    conic.stops[1].angle = Some(a);
                    conic.stops[2].angle = Some(a);
                    conic.stops[2].color = dark;
                    conic.stops[3].color = dark;
                }
            }
        }
        (Part::Pip(i), f) => {
            if let Some(mut bg) = bg {
                let c = match f.map(|f| f.pips) {
                    Some((lit, max)) if *i < max => {
                        if *i < lit {
                            ui::GOLDEN
                        } else {
                            Color::srgb(0.15, 0.15, 0.17)
                        }
                    }
                    _ => Color::NONE,
                };
                bg.set_if_neq(BackgroundColor(c));
            }
        }
        (label, f) => {
            if let Some(mut text) = text {
                let s = f.map_or(String::new(), |f| match label {
                    Part::Seconds => f.seconds.clone(),
                    Part::Key => f.key.clone(),
                    Part::Cost => f.cost.clone(),
                    Part::Charges => f.charges.clone(),
                    _ => String::new(),
                });
                text.set_if_neq(Text(s));
            }
        }
    }
}

/// The bottom panel's portrait: a render of the unit's model over its
/// background, or Dota's picture of the hero.
fn portrait(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    drawn: Res<crate::portrait::Portrait>,
    mut image: Single<&mut ImageNode, (With<Portrait>, Without<PortraitRender>)>,
    mut render: Single<(&mut ImageNode, &mut Visibility), With<PortraitRender>>,
) {
    if let Some((model, backdrop)) = &drawn.shown {
        if render.0.image != *model {
            render.0.image = model.clone();
        }
        render.1.set_if_neq(Visibility::Inherited);
        // No background from Dota: the panel's own dark one.
        let tint = if backdrop.is_some() {
            Color::WHITE
        } else {
            Color::NONE
        };
        let handle = backdrop.clone().unwrap_or_default();
        if image.image != handle {
            image.image = handle;
        }
        if image.color != tint {
            image.color = tint;
        }
        return;
    }
    render.1.set_if_neq(Visibility::Hidden);
    let core = &session.0;
    let (p, unit, _) = subject(core);
    let hero = p
        .map(|p| p.hero.0)
        .or(unit.and_then(|u| u.hero.map(|h| h.0)));
    let path = hero.and_then(|h| manifest.portrait(h));
    // Nothing in place of a portrait the cache has not, rather than white.
    let tint = if path.is_none() {
        Color::NONE
    } else if p.is_some_and(|p| p.unit.is_none()) {
        Color::srgb(0.3, 0.3, 0.3)
    } else {
        Color::WHITE
    };
    set_image(&mut image, &server, path, tint);
}

/// Shows the cache's image at `path` (none: the default image) in a tint.
fn set_image(image: &mut Mut<ImageNode>, server: &AssetServer, path: Option<String>, tint: Color) {
    let handle: Handle<Image> = path.map(|p| server.load(p)).unwrap_or_default();
    if image.image != handle {
        image.image = handle;
    }
    if image.color != tint {
        image.color = tint;
    }
}

/// The sun by day and the moon by night beside the clock; nothing where the
/// cache has neither.
fn time_of_day(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut image: Single<&mut ImageNode, With<DayNight>>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let name = if bota_core::time_of_day(view.tick).night {
        "hud.night"
    } else {
        "hud.day"
    };
    let path = manifest.file(name);
    let tint = if path.is_some() {
        Color::WHITE
    } else {
        Color::NONE
    };
    set_image(&mut image, &server, path, tint);
}

/// The seat portraits of the top bar.
fn seats(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut cards: Query<(&SeatBox, &mut ImageNode, &mut BorderColor)>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    for (seat, mut image, mut border) in &mut cards {
        let Some(p) = seat_of(view, seat.0) else {
            continue;
        };
        let path = manifest
            .dota_name("hero", p.hero.0)
            .and_then(|name| manifest.file(&format!("portrait.hero.{name}")));
        let tint = if p.unit.is_none() {
            Color::srgb(0.3, 0.3, 0.3)
        } else {
            Color::WHITE
        };
        set_image(&mut image, &server, path, tint);
        let picked = p.unit.is_some() && p.unit == core.selected;
        let edge = if picked {
            Color::WHITE
        } else {
            crate::overlay::team_color(p.team)
        };
        if border.top != edge {
            *border = BorderColor::all(edge);
        }
    }
}

/// A regeneration as a pool bar shows it: "+1.5" a second, nothing when none.
fn regen_text(per_second: bota_proto::Fixed) -> String {
    if per_second > bota_proto::Fixed::ZERO {
        format!("+{:.1}", per_second.to_f32())
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The wire's experience is any number: the lowest one overflowed the
    /// share's arithmetic (a panic in a debug build).
    #[test]
    fn any_experience_gives_a_share_of_the_level() {
        assert_eq!(xp_share(2, i32::MIN), 0.0);
        assert_eq!(xp_share(2, i32::MAX), 1.0);
    }
}
