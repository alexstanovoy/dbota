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
use bota_proto::{AbilityView, Attribute, ItemView, Kit, PlayerView, Team, UnitKind, UnitView};

use crate::config::{Action, Settings, key_name};
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The size of an ability box.
const ABILITY: f32 = 58.0;
/// The size of an item box.
const ITEM: (f32, f32) = (60.0, 44.0);
/// The height of a backpack box, and of the gap under the inventory rows that
/// the town portal slot keeps clear of.
const BACKPACK_ROW: f32 = 26.0;

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
    Plus,
    Charges,
}

/// The parent box of a part, to find a part's slot.
#[derive(Component, Clone, Copy)]
struct PartOf(Entity);

/// A text the HUD keeps up to date, by what it says.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Readout {
    Clock,
    Score(Team),
    PanelName,
    PanelLevel,
    Damage,
    DamageBonus,
    Stats,
    Attribute(u8),
    AttributeBonus(u8),
    Hp,
    Mana,
    HpRegen,
    ManaRegen,
    LastHits,
    Kda,
    Feed,
    Notice,
    Hint,
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

/// The portrait image of the bottom panel.
#[derive(Component)]
struct Portrait;

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
            ((xp - from) as f32 / (to - from).max(1) as f32).clamp(0.0, 1.0)
        }
        _ => 1.0,
    }
}

fn xp_ring(share: f32) -> ConicGradient {
    let a = share.clamp(0.0, 1.0) * TAU;
    let gold = Color::srgb(0.95, 0.75, 0.3);
    let empty = Color::srgba(0.1, 0.1, 0.12, 0.9);
    ConicGradient::new(
        UiPosition::CENTER,
        vec![
            AngularColorStop::new(gold, 0.0),
            AngularColorStop::new(gold, a),
            AngularColorStop::new(empty, a),
            AngularColorStop::new(empty, TAU),
        ],
    )
}

/// Fills the level ring with the panel hero's experience.
fn xp(session: NonSend<Session>, mut ring: Single<&mut BackgroundGradient, With<XpRing>>) {
    let core = &session.0;
    let share = match subject(core) {
        (Some(p), _, _) => xp_share(p.level, p.xp),
        _ => 0.0,
    };
    **ring = BackgroundGradient::from(xp_ring(share));
}

/// The whole HUD, hidden in the lobby.
#[derive(Component)]
pub struct HudRoot;

/// A part of the HUD that swallows clicks meant for the world.
#[derive(Component)]
pub struct Blocker;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn_hud).add_systems(
        Update,
        (readouts, gauges, slots, portrait, seats, stash, xp)
            .run_if(|k: Res<crate::session::MatchKnown>| k.0),
    );
}

fn spawn_hud(mut commands: Commands, fonts: Res<Fonts>) {
    let root = commands
        .spawn((
            HudRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .id();
    spawn_bottom(&mut commands, &fonts, root);
    spawn_top(&mut commands, &fonts, root);
    spawn_corner(&mut commands, &fonts, root);
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

/// The bottom panel, centred along the window's foot.
fn spawn_bottom(commands: &mut Commands, fonts: &Fonts, root: Entity) {
    let bottom = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    let bar = commands
        .spawn((
            panel(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::FlexEnd,
                padding: UiRect::all(px(8)),
                column_gap: px(10),
                border: UiRect::top(px(2)).with_left(px(2)).with_right(px(2)),
                border_radius: BorderRadius::top(px(10)),
                ..default()
            }),
            ChildOf(bottom),
        ))
        .id();
    let portrait = commands
        .spawn((
            Portrait,
            Node {
                width: px(124),
                height: px(140),
                border: UiRect::all(px(2)),
                ..default()
            },
            BorderColor::all(ui::EDGE),
            BackgroundColor(ui::PANEL_DEEP),
            ImageNode::default(),
            ChildOf(bar),
        ))
        .id();
    // The level in a ring that fills with the experience to the next.
    let ring = commands
        .spawn((
            XpRing,
            Node {
                position_type: PositionType::Absolute,
                right: px(4),
                bottom: px(4),
                width: px(34),
                height: px(34),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundGradient::from(xp_ring(0.0)),
            ChildOf(portrait),
        ))
        .id();
    let disc = commands
        .spawn((
            Node {
                width: px(26),
                height: px(26),
                border_radius: BorderRadius::MAX,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.025, 0.035, 0.95)),
            ChildOf(ring),
        ))
        .id();
    commands.spawn((
        Readout::PanelLevel,
        ui::text("", &fonts.bold, 15.0, ui::GOLDEN),
        ChildOf(disc),
    ));
    spawn_numbers(commands, fonts, bar);
    spawn_middle(commands, fonts, bar);
    spawn_items(commands, fonts, bar);
}

/// The name and the stat lines.
fn spawn_numbers(commands: &mut Commands, fonts: &Fonts, bar: Entity) {
    let column = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                width: px(118),
                row_gap: px(4),
                ..default()
            },
            ChildOf(bar),
        ))
        .id();
    commands.spawn((
        Readout::PanelName,
        ui::text("", &fonts.bold, 15.0, ui::INK),
        TextLayout::no_wrap(),
        ChildOf(column),
    ));
    // Damage as Dota shows it: the range a swing draws from, the bonus
    // on top in green.
    let damage = commands
        .spawn((
            Node {
                column_gap: px(4),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    commands.spawn((
        Readout::Damage,
        ui::text("", &fonts.regular, 13.0, ui::INK_DIM),
        TextLayout::no_wrap(),
        ChildOf(damage),
    ));
    commands.spawn((
        Readout::DamageBonus,
        ui::text("", &fonts.bold, 13.0, ui::HP_MINE),
        TextLayout::no_wrap(),
        ChildOf(damage),
    ));
    commands.spawn((
        Readout::Stats,
        ui::text("", &fonts.regular, 13.0, ui::INK_DIM),
        ChildOf(column),
    ));
    // Strength, agility, intelligence: the level's own white, what items,
    // souls and buffs add green, as Dota shows them.
    let attributes = commands
        .spawn((
            Node {
                column_gap: px(3),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    for k in 0..3u8 {
        commands.spawn((
            Readout::Attribute(k),
            ui::text("", &fonts.bold, 13.0, ui::INK),
            TextLayout::no_wrap(),
            ChildOf(attributes),
        ));
        commands.spawn((
            Readout::AttributeBonus(k),
            ui::text("", &fonts.bold, 13.0, ui::HP_MINE),
            TextLayout::no_wrap(),
            Node {
                margin: UiRect::right(px(6)),
                ..default()
            },
            ChildOf(attributes),
        ));
    }
}

/// The ability row with the health and mana bars under it.
fn spawn_middle(commands: &mut Commands, fonts: &Fonts, bar: Entity) {
    let column = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(6),
                ..default()
            },
            ChildOf(bar),
        ))
        .id();
    commands.spawn((
        Readout::Hint,
        ui::text("", &fonts.bold, 13.0, ui::GOLDEN),
        ChildOf(column),
    ));
    crate::buffs::spawn_row(commands, fonts, column);
    let row = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                column_gap: px(6),
                ..default()
            },
            ChildOf(column),
        ))
        .id();

    crate::talents::spawn_button(commands, fonts, row, column);
    for slot in 0..6u8 {
        spawn_slot_box(
            commands,
            fonts,
            row,
            (ABILITY, ABILITY),
            AbilityBox(slot),
            true,
        );
    }
    for (gauge, readout, regen, height, color) in [
        (
            Gauge::Hp,
            Readout::Hp,
            Readout::HpRegen,
            20.0,
            ui::HP_FRIEND,
        ),
        (
            Gauge::Mana,
            Readout::Mana,
            Readout::ManaRegen,
            15.0,
            ui::MANA,
        ),
    ] {
        let well = commands
            .spawn((
                Node {
                    // Under the abilities, from the first to the ultimate,
                    // past the talent button left of them.
                    width: px(6.0 * ABILITY + 5.0 * 6.0),
                    margin: UiRect::left(px(crate::talents::BUTTON + 6.0)),
                    height: px(height),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                ChildOf(column),
            ))
            .id();
        commands.spawn((
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
        commands.spawn((
            readout,
            ui::text("", &fonts.bold, height - 5.0, ui::INK),
            ui::shadow(),
            ChildOf(well),
        ));
        commands.spawn((
            regen,
            Node {
                position_type: PositionType::Absolute,
                right: px(6),
                ..default()
            },
            ui::text("", &fonts.bold, height - 7.0, ui::INK),
            ui::shadow(),
            ChildOf(well),
        ));
    }
}

/// The inventory, the backpack below it and the stash above it.
fn spawn_items(commands: &mut Commands, fonts: &Fonts, bar: Entity) {
    let grid = commands
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
            commands,
            fonts,
            grid,
            size,
            ItemBox(slot),
            slot < INVENTORY_SLOTS,
        );
    }
    spawn_town_portal(commands, fonts, bar);
    let stash = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                right: px(8),
                display: Display::Grid,
                grid_template_columns: RepeatedGridTrack::px(3, ITEM.0),
                column_gap: px(5),
                row_gap: px(5),
                padding: UiRect::all(px(6)),
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
            Interaction::default(),
            Blocker,
            StashStrip,
            ChildOf(bar),
        ))
        .id();
    for slot in BAG_SLOTS..BAG_SLOTS + 6 {
        spawn_slot_box(commands, fonts, stash, (ITEM.0, 30.0), ItemBox(slot), false);
    }
}

/// Dota's own slot for scrolls, a small square at the right of the inventory
/// rows.
fn spawn_town_portal(commands: &mut Commands, fonts: &Fonts, bar: Entity) {
    let column = commands
        .spawn((
            Node {
                height: px(2.0 * ITEM.1 + 5.0),
                margin: UiRect::bottom(px(BACKPACK_ROW + 5.0)),
                ..default()
            },
            ChildOf(bar),
        ))
        .id();
    spawn_slot_box(
        commands,
        fonts,
        column,
        (ITEM.1, ITEM.1),
        ItemBox(TOWN_PORTAL_SLOT),
        true,
    );
}

/// The stash strip, shown while it holds anything or at home.
#[derive(Component)]
pub struct StashStrip;

/// One clickable slot box with its icon, its wait and its labels.
fn spawn_slot_box(
    commands: &mut Commands,
    fonts: &Fonts,
    parent: Entity,
    (w, h): (f32, f32),
    tag: impl Component,
    keyed: bool,
) {
    let slot = commands
        .spawn((
            tag,
            Button,
            Node {
                width: px(w),
                height: px(h),
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(Color::srgb(0.2, 0.22, 0.27)),
            BackgroundColor(ui::PANEL_DEEP),
            ChildOf(parent),
        ))
        .id();
    let full = || Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        ..default()
    };
    commands.spawn((
        Part::Icon,
        PartOf(slot),
        full(),
        ImageNode::default(),
        Pickable::IGNORE,
        ChildOf(slot),
    ));
    commands.spawn((
        Part::Shade,
        PartOf(slot),
        full(),
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
        ChildOf(slot),
    ));
    commands.spawn((
        Part::Sweep,
        PartOf(slot),
        full(),
        BackgroundGradient::from(sweep(0.0)),
        Pickable::IGNORE,
        ChildOf(slot),
    ));
    let label = |commands: &mut Commands, part: Part, node: Node, size: f32, color: Color| {
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
    let at = |left: Option<f32>, top: Option<f32>, right: Option<f32>, bottom: Option<f32>| Node {
        position_type: PositionType::Absolute,
        left: left.map_or(Val::Auto, px),
        top: top.map_or(Val::Auto, px),
        right: right.map_or(Val::Auto, px),
        bottom: bottom.map_or(Val::Auto, px),
        ..default()
    };
    let centre = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
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
    if keyed {
        label(
            commands,
            Part::Key,
            at(Some(3.0), Some(1.0), None, None),
            12.0,
            ui::INK,
        );
    }
    label(
        commands,
        Part::Cost,
        at(None, None, Some(3.0), Some(2.0)),
        12.0,
        Color::srgb(0.55, 0.75, 1.0),
    );
    label(
        commands,
        Part::Charges,
        at(None, None, Some(3.0), Some(1.0)),
        13.0,
        ui::INK,
    );
    label(
        commands,
        Part::Plus,
        at(None, Some(1.0), Some(3.0), None),
        18.0,
        ui::GOLDEN,
    );
    for pip in 0..4u8 {
        commands.spawn((
            Part::Pip(pip),
            PartOf(slot),
            Node {
                position_type: PositionType::Absolute,
                left: px(4.0 + f32::from(pip) * 10.0),
                bottom: px(2),
                width: px(8),
                height: px(4),
                ..default()
            },
            BackgroundColor(Color::NONE),
            Pickable::IGNORE,
            ChildOf(slot),
        ));
    }
}

/// A conic gradient darkening the part of a box still waiting, clockwise
/// from twelve o'clock: `left` is the share still to run.
fn sweep(left: f32) -> ConicGradient {
    let a = (1.0 - left.clamp(0.0, 1.0)) * TAU;
    let dark = Color::srgba(0.0, 0.0, 0.0, if left > 0.0 { 0.68 } else { 0.0 });
    ConicGradient::new(
        UiPosition::CENTER,
        vec![
            AngularColorStop::new(Color::NONE, 0.0),
            AngularColorStop::new(Color::NONE, a),
            AngularColorStop::new(dark, a),
            AngularColorStop::new(dark, TAU),
        ],
    )
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
                align_items: AlignItems::FlexStart,
                column_gap: px(6),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(root),
        ))
        .id();
    spawn_seat(commands, fonts, top, 0);
    let clock = commands
        .spawn((
            panel(Node {
                width: px(150),
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
    commands.spawn((
        Readout::Clock,
        ui::text("0:00", &fonts.bold, 22.0, ui::INK),
        ChildOf(clock),
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
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
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
            panel(Node {
                position_type: PositionType::Absolute,
                left: px(8),
                top: px(8),
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

/// Sets a text when it changed.
fn put(text: &mut Text, s: String) {
    if text.0 != s {
        text.0 = s;
    }
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
    let Some(p) = core
        .panel_slot()
        .and_then(|s| view.players.iter().find(|p| p.slot == s))
    else {
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
    settings: Res<Settings>,
    diagnostics: Res<bevy::diagnostic::DiagnosticsStore>,
    mut texts: Query<(&Readout, &mut Text)>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let rate = u32::from(core.tick_rate().max(1));
    let (p, unit, _) = subject(core);
    let mine = core
        .panel_slot()
        .and_then(|s| view.players.iter().find(|p| p.slot == s));
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
            Readout::PanelName => p
                .and_then(|p| bota_client_core::hero(p.hero.0))
                .map(|hero| hero.name.to_string())
                .or(unit.map(|u| kind_name(u.kind).to_string()))
                .unwrap_or_default(),
            Readout::PanelLevel => unit
                .filter(|u| u.level > 0)
                .map(|u| u.level.to_string())
                .unwrap_or_default(),
            Readout::Damage => unit
                .map(|u| {
                    let d = bota_client_core::damage_range(u);
                    if d.min == d.max {
                        format!("DMG {}", d.min)
                    } else {
                        format!("DMG {}\u{2013}{}", d.min, d.max)
                    }
                })
                .unwrap_or_default(),
            Readout::DamageBonus => unit
                .map(|u| bota_client_core::damage_range(u).bonus)
                .filter(|bonus| *bonus > 0)
                .map(|bonus| format!("+{bonus}"))
                .unwrap_or_default(),
            Readout::Stats => unit.map(stat_lines).unwrap_or_default(),
            Readout::Attribute(k) => unit.map(|u| attribute_text(u, k).0).unwrap_or_default(),
            Readout::AttributeBonus(k) => unit.map(|u| attribute_text(u, k).1).unwrap_or_default(),
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
            Readout::Hint => hint(core, unit, &settings),
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
                .map(|p| (p.respawn_left / rate + 1).to_string())
                .unwrap_or_default(),
        };
        put(&mut text, s);
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

fn stat_lines(u: &UnitView) -> String {
    format!(
        "ARM {:.1}\nMS {:.0}\nATK {:.2}/s\nRNG {:.0}",
        u.armor.to_f32(),
        u.move_speed.to_f32(),
        1000.0 / u.attack_time.max(1) as f32,
        u.attack_range.to_f32()
    )
}

/// One attribute of the panel: its letter and the level's own points, and
/// the green bonus on top, if any.
fn attribute_text(u: &UnitView, k: u8) -> (String, String) {
    let a = &u.attributes;
    if a.strength.to_f32() == 0.0 && a.agility.to_f32() == 0.0 {
        return (String::new(), String::new());
    }
    let (letter, which, total) = match k {
        0 => ("S", Attribute::Strength, a.strength),
        1 => ("A", Attribute::Agility, a.agility),
        _ => ("I", Attribute::Intelligence, a.intelligence),
    };
    let total = total.to_f32().floor() as i32;
    let mark = if u.primary == Some(which) { "*" } else { "" };
    match bota_client_core::base_attributes(u) {
        Some(base) => {
            let base = base[usize::from(k)].min(total);
            let bonus = total - base;
            (
                format!("{letter}{mark} {base}"),
                if bonus > 0 {
                    format!("+{bonus}")
                } else {
                    String::new()
                },
            )
        }
        None => (format!("{letter}{mark} {total}"), String::new()),
    }
}

/// The word of the moment in the middle of the screen.
fn notice(core: &Core, rate: u32) -> String {
    if core.lost {
        return "CONNECTION LOST".to_string();
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
        .and_then(|s| core.view()?.players.iter().find(|p| p.slot == s))
        .filter(|p| p.unit.is_none());
    if let Some(p) = dead {
        return format!("Respawn in {}", p.respawn_left / rate + 1);
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

/// The line over the ability row: unspent points and how to spend them.
fn hint(core: &Core, unit: Option<&UnitView>, settings: &Settings) -> String {
    let Some(u) = unit.filter(|u| Some(u.id) == core.my_hero()) else {
        return String::new();
    };
    if u.abilities.iter().any(|a| a.can_level) {
        let key = key_name(settings.key(Action::Ability(0)));
        format!("+ ability point  (Ctrl+{key}...)")
    } else {
        String::new()
    }
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
                .and_then(|id| view.units.iter().find(|u| u.id == id))
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
            if bg.0 != color {
                bg.0 = color;
            }
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
    plus: bool,
    charges: String,
    no_mana: bool,
    frame: Color,
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
    let abilities: &[AbilityView] = kit
        .map(|k| k.abilities.as_slice())
        .or(unit.map(|u| u.abilities.as_slice()))?;
    let a = abilities.get(usize::from(slot))?;
    let mana = if kit.is_some() {
        0
    } else {
        unit.map_or(0, |u| u.mana)
    };
    let look = ability_look(Some(a), mana);
    let own = unit.is_some_and(|u| core.drives(u.id))
        || (kit.is_some() && core.panel_slot() == core.my_slot);
    let aiming = own && core.aiming == Some(bota_client_core::Slot::Ability(slot));
    let rate = u32::from(core.tick_rate().max(1));
    Some(SlotFace {
        seconds: if a.cooldown_left > 0 {
            (a.cooldown_left / rate + 1).to_string()
        } else {
            String::new()
        },
        icon: manifest.ability_icon(a.id.0),
        look,
        sweep: unit.map_or(0.0, |u| core.derived.wait_left(u.id, slot, a.cooldown_left)),
        key: if look.passive {
            String::new()
        } else {
            key_name(settings.ability_key(slot, abilities.len())).to_string()
        },
        cost: if a.level > 0 && a.mana_cost > 0 {
            a.mana_cost.to_string()
        } else {
            String::new()
        },
        pips: (a.level, a.max_level),
        plus: own && a.can_level,
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
    let item: ItemView = if slot == TOWN_PORTAL_SLOT {
        match kit {
            Some(kit) => kit.town_portal?,
            None => unit?.town_portal?,
        }
    } else if slot >= BAG_SLOTS {
        let view = core.view()?;
        let p = view.players.iter().find(|p| Some(p.slot) == core.my_slot)?;
        (*p.stash.as_ref()?.get(usize::from(slot - BAG_SLOTS))?)?
    } else {
        let items = kit
            .map(|k| k.items.as_slice())
            .or(unit.map(|u| u.items.as_slice()))?;
        (*items.get(usize::from(slot))?)?
    };
    let mana = unit.map_or(0, |u| u.mana);
    let look = item_look(Some(&item), slot, mana);
    let held = core.held_item == Some(slot);
    let rate = u32::from(core.tick_rate().max(1));
    Some(SlotFace {
        seconds: if item.cooldown_left > 0 {
            (item.cooldown_left / rate + 1).to_string()
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
            Some(action) if !look.passive => key_name(settings.key(action)).to_string(),
            _ => String::new(),
        },
        cost: String::new(),
        pips: (0, 0),
        plus: false,
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
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
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
                let want: Option<Handle<Image>> =
                    f.and_then(|f| f.icon.clone()).map(|p| server.load(p));
                let handle = want.unwrap_or_default();
                if image.image != handle {
                    image.image = handle;
                }
                let tint = match f {
                    Some(f) if !f.look.learned => Color::srgb(0.35, 0.35, 0.35),
                    Some(_) => Color::WHITE,
                    None => Color::NONE,
                };
                if image.color != tint {
                    image.color = tint;
                }
            }
        }
        (Part::Shade, f) => {
            if let Some(mut bg) = bg {
                let c = match f {
                    Some(f) if !f.look.learned => Color::srgba(0.0, 0.0, 0.0, 0.55),
                    Some(f) if f.no_mana && !f.look.passive => Color::srgba(0.05, 0.12, 0.5, 0.45),
                    _ => Color::NONE,
                };
                if bg.0 != c {
                    bg.0 = c;
                }
            }
        }
        (Part::Sweep, f) => {
            if let Some(mut g) = gradient {
                let left = f
                    .filter(|f| f.look.cooldown_left > 0)
                    .map_or(0.0, |f| f.sweep.max(0.02));
                let a = (1.0 - left) * TAU;
                if let Some(Gradient::Conic(conic)) = g.0.first_mut() {
                    let dark = Color::srgba(0.0, 0.0, 0.0, if left > 0.0 { 0.68 } else { 0.0 });
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
                if bg.0 != c {
                    bg.0 = c;
                }
            }
        }
        (label, f) => {
            if let Some(mut text) = text {
                let s = f.map_or(String::new(), |f| match label {
                    Part::Seconds => f.seconds.clone(),
                    Part::Key => f.key.clone(),
                    Part::Cost => f.cost.clone(),
                    Part::Charges => f.charges.clone(),
                    Part::Plus if f.plus => "+".to_string(),
                    _ => String::new(),
                });
                put(&mut text, s);
            }
        }
    }
}

/// The bottom panel's portrait: the hero's Dota portrait.
fn portrait(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    mut image: Single<&mut ImageNode, With<Portrait>>,
) {
    let core = &session.0;
    let (p, unit, _) = subject(core);
    let hero = p
        .map(|p| p.hero.0)
        .or(unit.and_then(|u| u.hero.map(|h| h.0)));
    let path = hero
        .and_then(|h| manifest.dota_name("hero", h))
        .and_then(|name| {
            manifest
                .file(&format!("portrait.hero.{name}.tall"))
                .or(manifest.file(&format!("portrait.hero.{name}")))
        });
    let path_missing = path.is_none();
    let handle: Handle<Image> = path.map(|p| server.load(p)).unwrap_or_default();
    if image.image != handle {
        image.image = handle;
    }
    // Nothing in place of a portrait the cache has not, rather than white.
    let tint = if path_missing {
        Color::NONE
    } else if p.is_some_and(|p| p.unit.is_none()) {
        Color::srgb(0.3, 0.3, 0.3)
    } else {
        Color::WHITE
    };
    if image.color != tint {
        image.color = tint;
    }
}

/// What the panel calls a unit that is not a hero.
fn kind_name(kind: UnitKind) -> &'static str {
    match kind {
        UnitKind::Hero => "Hero",
        UnitKind::CreepMelee => "Melee Creep",
        UnitKind::CreepFlagbearer => "Flagbearer Creep",
        UnitKind::CreepRanged => "Ranged Creep",
        UnitKind::CreepSiege => "Siege Creep",
        UnitKind::CreepNeutral => "Neutral Creep",
        UnitKind::Tower => "Tower",
        UnitKind::Ancient => "Ancient",
        UnitKind::Barracks => "Barracks",
        UnitKind::Fountain => "Fountain",
        UnitKind::Ward => "Ward",
        UnitKind::Courier => "Courier",
        UnitKind::Filler => "Filler",
    }
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
        let handle: Handle<Image> = path.map(|p| server.load(p)).unwrap_or_default();
        if image.image != handle {
            image.image = handle;
        }
        let tint = if p.unit.is_none() {
            Color::srgb(0.3, 0.3, 0.3)
        } else {
            Color::WHITE
        };
        if image.color != tint {
            image.color = tint;
        }
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

/// Shows the stash as Dota does: at the home shop, or while an item is held
/// for moving. Away from home what waits there is the courier's to fetch;
/// the courier's own bag shows in the panel when the courier is selected.
fn stash(session: NonSend<Session>, mut strip: Single<&mut Visibility, With<StashStrip>>) {
    let core = &session.0;
    let show = core.at_home_shop() || core.held_item.is_some();
    let want = if show {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if **strip != want {
        **strip = want;
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
