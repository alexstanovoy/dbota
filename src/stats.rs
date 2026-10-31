//! The numbers over the portrait, as Dota's stats region shows them: damage,
//! attack speed, armor, magic resistance and move speed down its right side,
//! each by its icon, the bonus on top in green; strength, agility and
//! intelligence along its foot. Hovering them opens Dota's stats tooltip
//! (`tooltip_unit_damage_armor`): the attack and defense columns with the
//! base and the bonus of each, the armor's physical resistance, the status
//! resistance and evasion, and what each attribute gives. The labels are
//! Dota's own where the cache has them; every number is the simulator's.

use bevy::prelude::*;
use bota_core::rules;
use bota_proto::{Attribute, Fixed, UnitView};

use crate::dota_text::{Texts, plain};
use crate::hud::Blocker;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// The region over the portrait the tooltip opens for.
#[derive(Component)]
struct StatsZone;

/// The tooltip.
#[derive(Component)]
struct StatsTip;

/// The attributes along the region's foot, shown for what has attributes.
#[derive(Component)]
struct Foot;

/// One stat of the region, by its row.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Region {
    Damage,
    AttackSpeed,
    Armor,
    MagicResist,
    MoveSpeed,
}

/// A text the region or the tooltip keeps up to date.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Line {
    /// A region row's white base (`false`) or green bonus (`true`).
    Region(Region, bool),
    /// A text of an attribute at the region's foot.
    Attribute(u8, Shown),
    /// A text of the tooltip.
    Tip(Tip),
}

/// What of an attribute the foot shows: its whole, as Dota shows it, or,
/// while Alt is held, its base and its bonus (`.AltPressed #StrengthLabel`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Shown {
    Sum,
    Base,
    Bonus,
}

/// The texts of the tooltip.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tip {
    Header(bool),
    Name(Row),
    Base(Row),
    Bonus(Row),
    AttributeBase(u8),
    AttributeBonus(u8),
    Gain(u8),
    Primary(u8),
    Details(u8),
}

/// The rows of the tooltip's two columns: attack, then defense.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    AttackSpeed,
    Damage,
    AttackRange,
    MoveSpeed,
    ManaRegen,
    Armor,
    PhysicalResist,
    MagicResist,
    StatusResist,
    Evasion,
    HealthRegen,
}

const ATTACK_ROWS: [Row; 5] = [
    Row::AttackSpeed,
    Row::Damage,
    Row::AttackRange,
    Row::MoveSpeed,
    Row::ManaRegen,
];
const DEFENSE_ROWS: [Row; 6] = [
    Row::Armor,
    Row::PhysicalResist,
    Row::MagicResist,
    Row::StatusResist,
    Row::Evasion,
    Row::HealthRegen,
];

/// The region's sizes as `dota_hud_stats_region` has them, in Dota's pixels of
/// a 1080-line screen turned into interface pixels: the labels' 14 pixel
/// font, the icons' 14, a row of the label with its 3 above and 2 below, and a
/// hair between a number and its icon.
const REGION_FONT: f32 = 14.0 * ui::DOTA;
const REGION_ICON: f32 = 14.0 * ui::DOTA;
const REGION_ROW: f32 = 19.0 * ui::DOTA;
const REGION_GAP: f32 = 2.0 * ui::DOTA;

/// The region's rows, top down, with their icons.
const REGION: [(Region, &str); 5] = [
    (Region::Damage, "hud.damage"),
    (Region::AttackSpeed, "hud.attack_speed"),
    (Region::Armor, "hud.armor"),
    (Region::MagicResist, "hud.magic_resist"),
    (Region::MoveSpeed, "hud.speed"),
];

/// The attributes' icons, strength first.
const ATTRIBUTES: [&str; 3] = ["hud.strength", "hud.agility", "hud.intelligence"];

/// Dota's green of a bonus and red of a loss.
const BONUS: Color = Color::srgb(0.38, 0.95, 0.4);
const LOSS: Color = Color::srgb(0.95, 0.3, 0.25);
/// The tooltip's label grey and its header.
const LABEL: Color = Color::srgb(0.55, 0.6, 0.66);
const HEADER: Color = Color::srgb(0.8, 0.82, 0.85);
/// Width of the tooltip in interface pixels.
const TIP_WIDTH: f32 = 540.0;
/// The attributes' strip's height, their icons' size and their grey
/// (`#stragiint`, `.Reborn #StrengthIcon`, `.Reborn .StatLabel`).
const FOOT_HEIGHT: f32 = 20.0 * ui::DOTA;
const ATTRIBUTE_ICON: f32 = 13.0 * ui::DOTA;
const ATTRIBUTE_INK: Color = Color::srgb(0.8, 0.8, 0.8);

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (region, tooltip).run_if(crate::select::match_or_tactical),
    );
}

/// The region over the right side and the foot of `portrait`, and the
/// tooltip above it.
pub fn spawn(
    commands: &mut Commands,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
    portrait: Entity,
) {
    // Dark towards the right and the foot, as Dota shades its portrait
    // under the numbers.
    commands.spawn((
        ui::fill(),
        BackgroundGradient::from(LinearGradient::to_right(vec![
            ColorStop::new(Color::NONE, percent(45)),
            ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.7), percent(100)),
        ])),
        Pickable::IGNORE,
        ChildOf(portrait),
    ));
    commands.spawn((
        ui::fill(),
        BackgroundGradient::from(LinearGradient::to_bottom(vec![
            ColorStop::new(Color::NONE, percent(70)),
            ColorStop::new(Color::srgba(0.0, 0.0, 0.0, 0.75), percent(100)),
        ])),
        Pickable::IGNORE,
        ChildOf(portrait),
    ));
    let zone = commands
        .spawn((
            StatsZone,
            Interaction::default(),
            Blocker,
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::new(px(6), px(6), px(6), px(4)),
                ..ui::fill()
            },
            ChildOf(portrait),
        ))
        .id();
    spawn_column(commands, fonts, manifest, server, zone);
    spawn_foot(commands, fonts, manifest, server, zone);
    spawn_tip(commands, fonts, manifest, server, portrait);
}

/// The region's rows down the right side: value, bonus, icon.
fn spawn_column(
    commands: &mut Commands,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
    zone: Entity,
) {
    let column = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::FlexEnd,
                align_self: AlignSelf::FlexEnd,
                row_gap: px(2.0 * ui::DOTA),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(zone),
        ))
        .id();
    for (region, icon) in REGION {
        let row = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(REGION_GAP),
                    height: px(REGION_ROW),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(column),
            ))
            .id();
        for bonus in [false, true] {
            commands.spawn((
                Line::Region(region, bonus),
                ui::text(
                    "",
                    &fonts.bold,
                    REGION_FONT,
                    if bonus { BONUS } else { ui::INK },
                ),
                ui::shadow(),
                TextLayout::no_wrap(),
                Pickable::IGNORE,
                ChildOf(row),
            ));
        }
        commands.spawn((
            Node {
                width: px(REGION_ICON),
                height: px(REGION_ICON),
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            ui::art(manifest, server, icon),
            Pickable::IGNORE,
            ChildOf(row),
        ));
    }
}

/// Strength, agility and intelligence along the foot, as Dota's
/// `DOTAHUDStrAgiInt` lays them out: right-aligned on a dark strip, each its
/// whole and its icon; while Alt is held, its base and its bonus instead.
fn spawn_foot(
    commands: &mut Commands,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
    zone: Entity,
) {
    let foot = commands
        .spawn((
            Foot,
            Node {
                align_self: AlignSelf::FlexEnd,
                height: px(FOOT_HEIGHT),
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.67)),
            ui::Shrink::default(),
            Pickable::IGNORE,
            ChildOf(zone),
        ))
        .id();
    for (k, icon) in (0u8..).zip(ATTRIBUTES) {
        let cell = commands
            .spawn((
                Node {
                    align_items: AlignItems::Center,
                    flex_shrink: 0.0,
                    margin: UiRect::right(px(1)),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(foot),
            ))
            .id();
        for (shown, margin, ink) in [
            (Shown::Sum, 5.5, ATTRIBUTE_INK),
            (Shown::Base, 0.0, ATTRIBUTE_INK),
            (Shown::Bonus, 1.0, BONUS),
        ] {
            commands.spawn((
                Line::Attribute(k, shown),
                ui::text("", &fonts.bold, attribute_size(shown), ink),
                ui::OwnSize(attribute_size(shown)),
                ui::shadow(),
                TextLayout::no_wrap(),
                Node {
                    margin: UiRect::left(px(margin)),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(cell),
            ));
        }
        commands.spawn((
            Node {
                width: px(ATTRIBUTE_ICON),
                height: px(ATTRIBUTE_ICON),
                margin: UiRect::left(px(3)),
                flex_shrink: 0.0,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            ui::art(manifest, server, icon),
            Pickable::IGNORE,
            ChildOf(cell),
        ));
    }
}

/// An attribute text's size: Dota's 18 for the whole and 12 for the split,
/// in the panel's measures.
fn attribute_size(shown: Shown) -> f32 {
    let dota = if shown == Shown::Sum { 18.0 } else { 12.0 };
    dota * ui::DOTA
}

/// The tooltip above the portrait: the attack and defense columns, then a
/// row for each attribute.
fn spawn_tip(
    commands: &mut Commands,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
    portrait: Entity,
) {
    let tip = commands
        .spawn((
            StatsTip,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: px(0),
                margin: UiRect::bottom(px(40)),
                width: px(TIP_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                padding: UiRect::all(px(14)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.07, 0.09, 0.11, 0.98)),
            BorderColor::all(Color::BLACK),
            GlobalZIndex(30),
            Pickable::IGNORE,
            Visibility::Hidden,
            ChildOf(portrait),
        ))
        .id();
    let columns = commands
        .spawn((
            Node {
                column_gap: px(24),
                ..default()
            },
            ChildOf(tip),
        ))
        .id();
    for (defense, rows) in [(false, &ATTACK_ROWS[..]), (true, &DEFENSE_ROWS[..])] {
        let column = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    flex_basis: px(0),
                    row_gap: px(3),
                    ..default()
                },
                ChildOf(columns),
            ))
            .id();
        tip_text(commands, fonts, Tip::Header(defense), 15.0, HEADER, column);
        for row in rows {
            spawn_tip_row(commands, fonts, *row, column);
        }
    }
    for k in 0..3u8 {
        spawn_tip_attribute(commands, fonts, manifest, server, k, tip);
    }
}

/// One `label: base +bonus` row of a tooltip column.
fn spawn_tip_row(commands: &mut Commands, fonts: &Fonts, row: Row, column: Entity) {
    let line = commands
        .spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                column_gap: px(6),
                ..default()
            },
            ChildOf(column),
        ))
        .id();
    tip_text(commands, fonts, Tip::Name(row), 14.0, LABEL, line);
    let values = commands
        .spawn((
            Node {
                column_gap: px(4),
                ..default()
            },
            ChildOf(line),
        ))
        .id();
    tip_text(commands, fonts, Tip::Base(row), 14.0, ui::INK, values);
    tip_text(commands, fonts, Tip::Bonus(row), 14.0, BONUS, values);
}

/// An attribute's row of the tooltip: its icon, then its values with the
/// gain per level, the primary attribute's damage, and what it gives.
fn spawn_tip_attribute(
    commands: &mut Commands,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
    k: u8,
    tip: Entity,
) {
    let icon = ATTRIBUTES[usize::from(k)];
    let row = commands
        .spawn((
            Node {
                column_gap: px(10),
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(tip),
        ))
        .id();
    commands.spawn((
        Node {
            width: px(36),
            height: px(36),
            flex_shrink: 0.0,
            border_radius: BorderRadius::MAX,
            ..default()
        },
        ui::art(manifest, server, icon),
        ChildOf(row),
    ));
    let details = commands
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ChildOf(row),
        ))
        .id();
    let values = commands
        .spawn((
            Node {
                column_gap: px(4),
                ..default()
            },
            ChildOf(details),
        ))
        .id();
    tip_text(
        commands,
        fonts,
        Tip::AttributeBase(k),
        15.0,
        ui::INK,
        values,
    );
    tip_text(commands, fonts, Tip::AttributeBonus(k), 15.0, BONUS, values);
    tip_text(commands, fonts, Tip::Gain(k), 14.0, LABEL, values);
    tip_text(commands, fonts, Tip::Primary(k), 14.0, ui::GOLDEN, details);
    tip_text(commands, fonts, Tip::Details(k), 14.0, LABEL, details);
}

fn tip_text(
    commands: &mut Commands,
    fonts: &Fonts,
    tip: Tip,
    size: f32,
    color: Color,
    parent: Entity,
) {
    commands.spawn((
        Line::Tip(tip),
        ui::text("", &fonts.bold, size, color),
        ChildOf(parent),
    ));
}

/// What one attribute of a hero is: the level's own whole points, what
/// items, souls, buffs and talents add, the whole (unfloored), the gain per
/// level, and whether it pays attack damage.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Attr {
    base: i32,
    bonus: i32,
    total: f32,
    gain: f32,
    primary: bool,
}

/// The white and the green of each stat, as the simulator's client core
/// splits them the way Dota does ([`bota_client_core::stat_split`]): white is
/// the hero's own at its level with every attribute point it holds, green
/// what items, buffs, auras and talents add. Anything but a hero shows its
/// whole numbers white.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Breakdown {
    damage: (i32, i32, i32),
    attack_speed: (i32, i32),
    attack_range: (i32, i32),
    move_speed: (i32, i32),
    armor: (f32, f32),
    /// Percent, as the next two.
    magic_resist: (f32, f32),
    status_resist: f32,
    evasion: f32,
    hp_regen: (f32, f32),
    mana_regen: (f32, f32),
    attributes: Option<[Attr; 3]>,
}

impl Breakdown {
    fn of(u: &UnitView) -> Breakdown {
        let d = bota_client_core::damage_range(u);
        let whole = |f: Fixed| f.to_f32().round() as i32;
        let Some(split) = bota_client_core::stat_split(u) else {
            return Breakdown {
                damage: (d.min, d.max, d.bonus),
                attack_speed: (u.attack_speed, 0),
                attack_range: (whole(u.attack_range), 0),
                move_speed: (whole(u.move_speed), 0),
                armor: (u.armor.to_f32(), 0.0),
                magic_resist: (u.magic_resist.to_f32() * 100.0, 0.0),
                status_resist: u.status_resist.to_f32() * 100.0,
                evasion: u.evasion.to_f32() * 100.0,
                hp_regen: (u.hp_regen.to_f32(), 0.0),
                mana_regen: (u.mana_regen.to_f32(), 0.0),
                attributes: None,
            };
        };
        let ints = |s: bota_client_core::Split<i32>| (s.base, s.bonus);
        let parts = |s: bota_client_core::Split<Fixed>| (s.base.to_f32(), s.bonus.to_f32());
        Breakdown {
            damage: (d.min, d.max, d.bonus),
            attack_speed: ints(split.attack_speed),
            attack_range: ints(split.attack_range),
            move_speed: ints(split.move_speed),
            armor: parts(split.armor),
            magic_resist: parts(split.magic_resist),
            status_resist: split.status_resist.base.to_f32() + split.status_resist.bonus.to_f32(),
            evasion: split.evasion.base.to_f32() + split.evasion.bonus.to_f32(),
            hp_regen: parts(split.hp_regen),
            mana_regen: parts(split.mana_regen),
            attributes: attributes(u, &split),
        }
    }
}

/// A hero's three attributes, or none for a unit that has none.
fn attributes(u: &UnitView, split: &bota_client_core::StatSplit) -> Option<[Attr; 3]> {
    let def = u.hero.and_then(bota_core::hero_def)?.unit;
    let all = [
        Attribute::Strength,
        Attribute::Agility,
        Attribute::Intelligence,
    ];
    let any = all.iter().any(|a| u.attributes.of(*a) > Fixed::ZERO);
    any.then(|| {
        all.map(|a| {
            let at = a as usize;
            Attr {
                base: split.attributes[at].base,
                bonus: split.attributes[at].bonus,
                total: u.attributes.of(a).to_f32(),
                gain: def.per_level.attributes.of(a).to_f32(),
                primary: u.primary == Some(a),
            }
        })
    })
}

/// A whole bonus as Dota writes it beside its base: `+5`, `-2`, or nothing.
fn signed(n: i32) -> String {
    match n {
        0 => String::new(),
        n if n > 0 => format!("+{n}"),
        n => n.to_string(),
    }
}

/// A part bonus to a tenth: `+1.5`, `-0.3`, or nothing when it rounds to none.
fn signed_tenths(n: f32) -> String {
    let tenths = (n * 10.0).round() as i32;
    match tenths {
        0 => String::new(),
        t if t > 0 => format!("+{:.1}", n),
        _ => format!("{:.1}", n),
    }
}

/// The share of physical damage an armor turns away, percent, by the
/// simulator's own mitigation.
fn physical_resist(armor: Fixed) -> f32 {
    const PROBE: i32 = 1_000_000;
    let through = bota_core::mitigate(PROBE, bota_proto::DamageKind::Physical, armor, 0);
    100.0 * (1.0 - through as f32 / PROBE as f32)
}

/// The region's texts, the tooltip's shown while the region is hovered.
fn region(
    session: NonSend<Session>,
    fired: Res<crate::keys::Fired>,
    mut lines: Query<(&Line, &mut Text, &mut TextColor, &mut Node)>,
    mut foot: Single<&mut Visibility, With<Foot>>,
) {
    let unit = crate::hud::subject(&session.0).1;
    let b = unit.map(Breakdown::of);
    let split = fired.held(crate::keys::Action::ShowRanges);
    foot.set_if_neq(ui::visible(b.is_some_and(|b| b.attributes.is_some())));
    for (line, mut text, mut color, mut node) in &mut lines {
        let (s, loss) = match (*line, unit, b) {
            (Line::Region(r, bonus), Some(_), Some(b)) => region_text(r, bonus, &b),
            (Line::Attribute(k, shown), _, Some(b)) => {
                let display = if (shown == Shown::Sum) != split {
                    Display::Flex
                } else {
                    Display::None
                };
                if node.display != display {
                    node.display = display;
                }
                b.attributes.map_or((String::new(), false), |a| {
                    attribute_text(a[usize::from(k)], shown)
                })
            }
            (Line::Tip(_), ..) => continue,
            _ => (String::new(), false),
        };
        // An empty bonus takes no place in the row, so no gap opens before the icon.
        if let Line::Region(..) = line {
            let display = if s.is_empty() {
                Display::None
            } else {
                Display::Flex
            };
            if node.display != display {
                node.display = display;
            }
        }
        if let Line::Region(_, true) | Line::Attribute(_, Shown::Bonus) = line {
            color.set_if_neq(TextColor(if loss { LOSS } else { BONUS }));
        }
        text.set_if_neq(Text(s));
    }
}

/// An attribute's text: its whole, its base, or its bonus and whether it
/// is a loss.
fn attribute_text(a: Attr, shown: Shown) -> (String, bool) {
    match shown {
        Shown::Sum => ((a.base + a.bonus).to_string(), false),
        Shown::Base => (a.base.to_string(), false),
        Shown::Bonus => (signed(a.bonus), a.bonus < 0),
    }
}

/// One row of the region: the base (or the whole), or the bonus and whether
/// it is a loss.
fn region_text(r: Region, bonus: bool, b: &Breakdown) -> (String, bool) {
    let (base, extra) = match r {
        Region::Damage => (((b.damage.0 + b.damage.1) / 2).to_string(), b.damage.2),
        Region::AttackSpeed => (b.attack_speed.0.to_string(), b.attack_speed.1),
        Region::Armor => (
            format!("{:.0}", b.armor.0),
            (b.armor.0 + b.armor.1).round() as i32 - b.armor.0.round() as i32,
        ),
        Region::MagicResist => (
            format!("{:.0}%", b.magic_resist.0),
            (b.magic_resist.0 + b.magic_resist.1).round() as i32 - b.magic_resist.0.round() as i32,
        ),
        Region::MoveSpeed => (b.move_speed.0.to_string(), b.move_speed.1),
    };
    if bonus {
        let percent = if r == Region::MagicResist && extra != 0 {
            "%"
        } else {
            ""
        };
        (format!("{}{percent}", signed(extra)), extra < 0)
    } else {
        (base, false)
    }
}

/// Shows the tooltip while the region is hovered (or `--tip stats` keeps it
/// open), with the subject's numbers.
fn tooltip(
    session: NonSend<Session>,
    texts: Res<Texts>,
    args: Res<crate::Args>,
    zone: Query<&Interaction, With<StatsZone>>,
    mut tip: Single<&mut Visibility, With<StatsTip>>,
    mut lines: Query<(&Line, &mut Text, &mut TextColor)>,
) {
    let forced = args.tip.as_deref() == Some("stats");
    let hovered = forced || zone.iter().any(|i| *i != Interaction::None);
    let unit = crate::hud::subject(&session.0).1.filter(|_| hovered);
    tip.set_if_neq(ui::visible(unit.is_some()));
    let Some(u) = unit else {
        return;
    };
    let b = Breakdown::of(u);
    for (line, mut text, mut color) in &mut lines {
        let Line::Tip(t) = *line else {
            continue;
        };
        let s = tip_line(t, u, &b, &texts);
        // A bonus below nothing is a loss, red as Dota writes it.
        if let Tip::Bonus(_) | Tip::AttributeBonus(_) = t {
            let tint = if s.starts_with('-') { LOSS } else { BONUS };
            color.set_if_neq(TextColor(tint));
        }
        text.set_if_neq(Text(s));
    }
}

/// One text of the tooltip.
fn tip_line(t: Tip, u: &UnitView, b: &Breakdown, texts: &Texts) -> String {
    let label = |key: &str, own: &str| texts.label(key, &[]).unwrap_or_else(|| own.to_string());
    match t {
        Tip::Header(false) => label("DOTA_HUD_Attack", "ATTACK"),
        Tip::Header(true) => label("DOTA_HUD_Defense", "DEFENSE"),
        Tip::Name(row) => {
            let (key, own) = row_name(row);
            label(key, own)
        }
        Tip::Base(row) => row_values(row, u, b).0,
        Tip::Bonus(row) => row_values(row, u, b).1,
        Tip::AttributeBase(k) => b
            .attributes
            .map_or(String::new(), |a| a[usize::from(k)].base.to_string()),
        Tip::AttributeBonus(k) => b
            .attributes
            .map_or(String::new(), |a| signed(a[usize::from(k)].bonus)),
        Tip::Gain(k) => attribute_line(k, b, texts, AttributeLine::Gain),
        Tip::Primary(k) => attribute_line(k, b, texts, AttributeLine::Primary),
        Tip::Details(k) => attribute_line(k, b, texts, AttributeLine::Details),
    }
}

/// A row's label key and the client's own words for it.
fn row_name(row: Row) -> (&'static str, &'static str) {
    match row {
        Row::AttackSpeed => ("DOTA_HUD_AttackSpeed", "Attack speed:"),
        Row::Damage => ("DOTA_HUD_Damage", "Damage:"),
        Row::AttackRange => ("DOTA_HUD_AttackRange", "Attack range:"),
        Row::MoveSpeed => ("DOTA_HUD_MoveSpeed", "Move speed:"),
        Row::ManaRegen => ("DOTA_HUD_ManaRegenDetail", "Mana regen:"),
        Row::Armor => ("DOTA_HUD_Armor", "Armor:"),
        Row::PhysicalResist => ("DOTA_HUD_PhysicalResist", "Physical resistance:"),
        Row::MagicResist => ("DOTA_HUD_MagicResist", "Magic resistance:"),
        Row::StatusResist => ("DOTA_HUD_StatusResist", "Status resistance:"),
        Row::Evasion => ("DOTA_HUD_Evasion", "Evasion:"),
        Row::HealthRegen => ("DOTA_HUD_HealthRegenDetail", "Health regen:"),
    }
}

/// A row's base and bonus.
fn row_values(row: Row, u: &UnitView, b: &Breakdown) -> (String, String) {
    let tenths = |(base, bonus): (f32, f32)| (format!("{base:.1}"), signed_tenths(bonus));
    match row {
        Row::AttackSpeed => (
            format!(
                "{} ({:.2}s)",
                b.attack_speed.0,
                u.attack_time as f32 / 1000.0
            ),
            signed(b.attack_speed.1),
        ),
        Row::Damage => (
            format!("{} - {}", b.damage.0, b.damage.1),
            signed(b.damage.2),
        ),
        Row::AttackRange => (b.attack_range.0.to_string(), signed(b.attack_range.1)),
        Row::MoveSpeed => (b.move_speed.0.to_string(), signed(b.move_speed.1)),
        Row::ManaRegen => tenths(b.mana_regen),
        Row::Armor => tenths(b.armor),
        Row::PhysicalResist => (format!("{:.0}%", physical_resist(u.armor)), String::new()),
        Row::MagicResist => (
            format!("{:.0}%", b.magic_resist.0),
            match (b.magic_resist.0 + b.magic_resist.1).round() as i32
                - b.magic_resist.0.round() as i32
            {
                0 => String::new(),
                n => format!("{}%", signed(n)),
            },
        ),
        Row::StatusResist => (format!("{:.0}%", b.status_resist), String::new()),
        Row::Evasion => (format!("{:.0}%", b.evasion), String::new()),
        Row::HealthRegen => tenths(b.hp_regen),
    }
}

/// Which of an attribute's lines.
#[derive(Clone, Copy)]
enum AttributeLine {
    Gain,
    Primary,
    Details,
}

/// An attribute's gain per level, its damage if primary, or what it gives:
/// Dota's words with the simulator's numbers, or the client's own.
fn attribute_line(k: u8, b: &Breakdown, texts: &Texts, which: AttributeLine) -> String {
    let Some(a) = b.attributes.map(|a| a[usize::from(k)]) else {
        return String::new();
    };
    let name = ["strength", "agility", "intelligence"][usize::from(k)];
    let title = ["Strength", "Agility", "Intelligence"][usize::from(k)];
    match which {
        AttributeLine::Gain => {
            let gain = plain(f64::from(a.gain));
            texts
                .label(
                    &format!("DOTA_HUD_{title}Gain"),
                    &[(&format!("{name}_per_level"), gain.clone())],
                )
                .unwrap_or_else(|| format!(" (+{gain} per level)"))
        }
        AttributeLine::Primary if a.primary => {
            let damage = (a.total.floor() as i32 * rules::DAMAGE_PER_PRIMARY).to_string();
            texts
                .label(
                    &format!("DOTA_HUD_PrimaryAttribute{title}"),
                    &[("primary_attribute_damage", damage.clone())],
                )
                .unwrap_or_else(|| format!("= {damage} damage (primary attribute)"))
        }
        AttributeLine::Primary => String::new(),
        AttributeLine::Details => {
            let values = gives(k, a.total);
            let pairs: Vec<(&str, String)> = values.iter().map(|(k, v)| (*k, v.clone())).collect();
            texts
                .label(&format!("DOTA_HUD_{title}Details"), &pairs)
                .unwrap_or_else(|| own_gives(k, &values))
        }
    }
}

/// What `total` points of attribute `k` give, under Dota's variable names.
fn gives(k: u8, total: f32) -> Vec<(&'static str, String)> {
    let per_second = |f: Fixed| f.to_f32() * rules::TICKS_PER_SECOND as f32;
    let tenth = |v: f32| format!("{v:.1}");
    match k {
        0 => vec![
            (
                "strength_hp",
                ((total * rules::HP_PER_STRENGTH as f32).floor() as i32).to_string(),
            ),
            (
                "strength_hp_regen",
                tenth(total * per_second(rules::HP_REGEN_PER_STRENGTH)),
            ),
        ],
        1 => vec![
            (
                "agility_armor",
                tenth(total * rules::ARMOR_PER_AGILITY.to_f32()),
            ),
            (
                "agility_attack_speed",
                ((total * rules::ATTACK_SPEED_PER_AGILITY as f32).floor() as i32).to_string(),
            ),
        ],
        _ => vec![
            (
                "intelligence_mana",
                ((total * rules::MANA_PER_INTELLIGENCE as f32).floor() as i32).to_string(),
            ),
            (
                "intelligence_mana_regen",
                tenth(total * per_second(rules::MANA_REGEN_PER_INTELLIGENCE)),
            ),
            (
                "intelligence_magic_resist",
                tenth(total * rules::MAGIC_RESIST_BP_PER_INTELLIGENCE as f32 / 100.0),
            ),
        ],
    }
}

/// The client's own words for what an attribute gives.
fn own_gives(k: u8, values: &[(&str, String)]) -> String {
    let v = |at: usize| values.get(at).map_or("", |(_, v)| v.as_str());
    match k {
        0 => format!("= {} health and {} health regen", v(0), v(1)),
        1 => format!("= {} armor and {} attack speed", v(0), v(1)),
        _ => format!(
            "= {} mana, {} mana regen and {}% base magic resistance",
            v(0),
            v(1),
            v(2)
        ),
    }
}
