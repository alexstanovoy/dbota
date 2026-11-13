//! A hero as the game's pick panel shows it: the name under the primary attribute's mark, the tall
//! portrait, each attribute as its base and its gain by level on one baseline, the figures of a
//! fight, the row of the hero's talents, innate and abilities (each with its popup under the
//! pointer) and the health and mana bars with what they regain a second.
//!
//! The numbers come from [`Facts`], computed from the rules' own data, so what the panel says is
//! what the match will play with. What the game's panel also holds and bota has no data for (the
//! complexity, a hero's description, similar heroes) is not drawn.

use bevy::prelude::*;
use bota_core::HeroDef;
use bota_proto::Attribute;

use super::reference as r;
use super::{Kit, NAME_FACE, PICK, Shot, Spot, WORD_FACE};
use crate::theme::{self, Face};
use crate::tips::{AbilityTile, TalentTile};
use crate::ui::Cover;

/// A hero's figures as the panel shows them (the install's hero data: Attack Rate is the base
/// attack time, `BaseAttackSpeed` 100 plus one point per agility is the attack speed, damage is
/// the average of the minimum and maximum with the primary attribute's points added).
pub struct Facts {
    pub primary: Option<Attribute>,
    /// Each attribute: its mark, its name, what the hero starts with and what a level adds.
    pub attributes: [(&'static str, &'static str, i32, f32); 3],
    /// The average of an attack's damage at level one, attribute bonus in.
    pub damage: i32,
    pub armor: f32,
    pub move_speed: i32,
    /// Seconds between two attacks at the base attack speed (Dota's "attack rate").
    pub attack_time: f32,
    /// Attack speed: the base and what agility adds.
    pub attack_speed: i32,
    pub attack_range: i32,
    pub ranged: bool,
    pub health: (i32, f32),
    pub mana: (i32, f32),
}

impl Facts {
    pub fn of(hero: &HeroDef) -> Facts {
        let unit = hero.unit;
        let attributes = unit.attributes;
        let growth = unit.per_level.attributes;
        let least = bota_core::base_damage_least(unit, 1);
        let per_second = |base: bota_proto::Fixed, per_point: bota_proto::Fixed, points| {
            ((base + points * per_point)
                * bota_proto::Fixed::from_int(bota_core::rules::TICKS_PER_SECOND as i32))
            .to_f32()
        };
        Facts {
            primary: unit.primary,
            attributes: [
                (
                    "hud.strength",
                    "STRENGTH",
                    attributes.strength.to_int(),
                    growth.strength.to_f32(),
                ),
                (
                    "hud.agility",
                    "AGILITY",
                    attributes.agility.to_int(),
                    growth.agility.to_f32(),
                ),
                (
                    "hud.intelligence",
                    "INTELLIGENCE",
                    attributes.intelligence.to_int(),
                    growth.intelligence.to_f32(),
                ),
            ],
            damage: least + unit.damage_spread / 2,
            armor: unit.armor as f32 + bota_core::agility_armor(attributes).to_f32(),
            move_speed: unit.move_speed,
            attack_time: unit.attack_time as f32 / 1000.0,
            attack_speed: unit.attack_speed + bota_core::agility_attack_speed(attributes),
            attack_range: unit.attack_range,
            ranged: unit.projectile_speed.is_some(),
            health: (
                unit.max_hp
                    + (attributes.strength
                        * bota_proto::Fixed::from_int(bota_core::rules::HP_PER_STRENGTH))
                    .to_int(),
                per_second(
                    unit.hp_regen,
                    bota_core::rules::HP_REGEN_PER_STRENGTH,
                    attributes.strength,
                ),
            ),
            mana: (
                unit.max_mana
                    + (attributes.intelligence
                        * bota_proto::Fixed::from_int(bota_core::rules::MANA_PER_INTELLIGENCE))
                    .to_int(),
                per_second(
                    unit.mana_regen,
                    bota_core::rules::MANA_REGEN_PER_INTELLIGENCE,
                    attributes.intelligence,
                ),
            ),
        }
    }

    /// The mark of the hero's primary attribute; a hero with none (universal) has no mark.
    pub fn primary_mark(&self) -> Option<&'static str> {
        let at = match self.primary? {
            Attribute::Strength => 0,
            Attribute::Agility => 1,
            Attribute::Intelligence => 2,
        };
        Some(self.attributes[at].0)
    }
}

/// The sizes of a row of a hero's skills, in the screenshot's pixels: the round marks (talents,
/// innate), the abilities' square, the gap between the marks, after them and between abilities.
#[derive(Clone, Copy)]
pub struct SkillSizes {
    /// What the render tests call the first ability's tile.
    pub spot: &'static str,
    pub mark: f32,
    pub icon: f32,
    pub marks_gap: f32,
    pub after_marks: f32,
    pub icon_gap: f32,
}

/// The hero panel of the pick screen.
pub const PANEL_SKILLS: SkillSizes = SkillSizes {
    spot: "pick panel skill 0",
    mark: r::PANEL_MARK,
    icon: r::PANEL_SKILL,
    marks_gap: r::PANEL_MARKS_GAP,
    after_marks: r::PANEL_MARKS_GAP,
    icon_gap: r::PANEL_SKILL_PITCH - r::PANEL_SKILL,
};

/// The strategy screen's hero details.
pub const DETAIL_SKILLS: SkillSizes = SkillSizes {
    spot: "strategy ability 0",
    mark: r::ABILITY_MARK,
    icon: r::ABILITIES[1],
    marks_gap: r::MARKS_GAP,
    after_marks: r::ABILITIES_GAP,
    icon_gap: r::ABILITIES[2],
};

/// The row of a hero's talent tree, innate and abilities, each a tile that opens its popup.
pub fn skills(
    commands: &mut Commands,
    kit: &Kit,
    shot: Shot,
    sizes: SkillSizes,
    hero: &HeroDef,
    parent: Entity,
) -> Entity {
    let row = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    let mark = shot.size(sizes.mark);
    let mut after = 0.0;
    if hero.talents.is_some() {
        let tile = commands
            .spawn((
                TalentTile(hero.id.0),
                super::Lift(1.08),
                Name::new("hero talents"),
                round(mark),
                BackgroundColor(crate::ui::PANEL_DEEP),
                BorderColor::all(Color::srgb(0.32, 0.33, 0.36)),
                ChildOf(row),
            ))
            .id();
        commands.spawn((
            Node {
                width: percent(70),
                height: percent(70),
                ..default()
            },
            kit.art("hud.talents"),
            Pickable::IGNORE,
            ChildOf(tile),
        ));
        after = sizes.marks_gap;
    }
    if let Some(innate) = hero.innate {
        let frame = commands
            .spawn((
                AbilityTile(innate.0),
                super::Lift(1.08),
                Name::new("hero innate"),
                Node {
                    width: px(mark),
                    height: px(mark),
                    margin: UiRect::left(shot.val(after)),
                    ..default()
                },
                ChildOf(row),
            ))
            .id();
        let badge = crate::hud::innate_badge(commands, kit.manifest, kit.server, frame, mark);
        commands.entity(badge).insert(Pickable::IGNORE);
        after = sizes.after_marks;
    } else if after > 0.0 {
        after = sizes.after_marks;
    }
    for (at, id) in hero.abilities.iter().enumerate() {
        let tile = commands
            .spawn((
                AbilityTile(id.0),
                Spot(if at == 0 { sizes.spot } else { "" }),
                super::Lift(1.08),
                Name::new(format!("hero ability {at}")),
                Node {
                    width: shot.val(sizes.icon),
                    height: shot.val(sizes.icon),
                    margin: UiRect::left(shot.val(if at == 0 { after } else { sizes.icon_gap })),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BorderColor::all(theme::PANEL_EDGE),
                ChildOf(row),
            ))
            .id();
        commands.spawn((
            ui_fill(),
            kit.image(kit.manifest.ability_icon(id.0)),
            Pickable::IGNORE,
            ChildOf(tile),
        ));
    }
    row
}

fn ui_fill() -> Node {
    crate::ui::fill()
}

fn round(side: f32) -> Node {
    Node {
        width: px(side),
        height: px(side),
        border: UiRect::all(px(2)),
        border_radius: BorderRadius::MAX,
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        flex_shrink: 0.0,
        ..default()
    }
}

/// Marks the panel's box: its contents are built again when another hero is shown.
#[derive(Component)]
pub struct HeroPanel;

/// The panel's empty box, in the column of the pick screen's right side (which stands at the
/// panel's place and is as wide as it).
pub fn shell(commands: &mut Commands, root: Entity) -> Entity {
    let shot = PICK;
    commands
        .spawn((
            HeroPanel,
            Name::new("pick panel"),
            Spot("pick panel"),
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                height: shot.val(r::PANEL_HEIGHT),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(0x0B, 0x10, 0x16, 0xF2)),
            BorderColor::all(Color::srgb_u8(0x2A, 0x33, 0x3D)),
            ChildOf(root),
        ))
        .id()
}

/// Fills the panel with a hero.
pub fn fill(commands: &mut Commands, kit: &Kit, panel: Entity, hero: &HeroDef) {
    let shot = PICK;
    let facts = Facts::of(hero);
    title(commands, kit, panel, hero, &facts);
    let body = commands
        .spawn((
            Node {
                margin: UiRect::left(shot.val(r::PANEL_INSET)),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        Name::new("pick panel portrait"),
        Spot("pick panel portrait"),
        Cover,
        kit.image(kit.manifest.portrait(hero.id.0)),
        Node {
            width: shot.val(r::PANEL_PORTRAIT[0]),
            height: shot.val(r::PANEL_PORTRAIT[1]),
            flex_shrink: 0.0,
            ..default()
        },
        ChildOf(body),
    ));
    let figures = commands
        .spawn((
            Node {
                width: shot.val(r::PANEL_ATTRIBUTES[0]),
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(body),
        ))
        .id();
    attributes(commands, kit, figures, &facts);
    stats(commands, kit, figures, &facts);
    let skills_row = skills(commands, kit, shot, PANEL_SKILLS, hero, panel);
    commands
        .entity(skills_row)
        .entry::<Node>()
        .and_modify(move |mut node| {
            node.margin = UiRect::new(
                shot.val(r::PANEL_INSET),
                Val::ZERO,
                shot.val(r::PANEL_SKILLS_GAP),
                Val::ZERO,
            );
        });
    let bars = commands
        .spawn((
            Node {
                margin: UiRect::new(
                    shot.val(r::PANEL_INSET),
                    Val::ZERO,
                    shot.val(r::PANEL_BARS_GAP),
                    Val::ZERO,
                ),
                flex_direction: FlexDirection::Column,
                row_gap: shot.val(r::PANEL_BAR_PITCH - r::PANEL_BAR[1]),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    let green = (
        Color::srgb_u8(0x4C, 0xA8, 0x3A),
        Color::srgb_u8(0x1F, 0x62, 0x14),
    );
    let blue = (
        Color::srgb_u8(0x3F, 0x7D, 0xC9),
        Color::srgb_u8(0x1D, 0x4B, 0x88),
    );
    pool_bar(commands, kit, bars, "health", facts.health, green);
    pool_bar(commands, kit, bars, "mana", facts.mana, blue);
}

/// The attribute's mark and the hero's name.
fn title(commands: &mut Commands, kit: &Kit, panel: Entity, hero: &HeroDef, facts: &Facts) {
    let shot = PICK;
    let row = commands
        .spawn((
            Node {
                height: shot.val(r::PANEL_TITLE_HEIGHT),
                padding: UiRect::left(shot.val(r::PANEL_INSET + 2.0)),
                align_items: AlignItems::Center,
                column_gap: shot.val(12.0),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    if let Some(mark) = facts.primary_mark() {
        commands.spawn((
            Node {
                width: shot.val(46.0),
                height: shot.val(46.0),
                ..default()
            },
            kit.art(mark),
            ChildOf(row),
        ));
    }
    commands.spawn((
        shot.text(
            kit,
            hero.name.to_uppercase(),
            (NAME_FACE, 52.0, 1.0),
            theme::INK_1,
        ),
        ChildOf(row),
    ));
}

/// The three attributes, each with its mark, its name and `base + gain` on one line.
fn attributes(commands: &mut Commands, kit: &Kit, column: Entity, facts: &Facts) {
    let shot = PICK;
    let block = commands
        .spawn((
            Name::new("pick panel attributes"),
            Spot("pick panel attributes"),
            Node {
                width: percent(100),
                height: shot.val(r::PANEL_ATTRIBUTES[1]),
                padding: UiRect::new(shot.val(30.0), Val::ZERO, shot.val(5.0), Val::ZERO),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgba_u8(0x07, 0x09, 0x0C, 0xF0)),
            ChildOf(column),
        ))
        .id();
    for (mark, label, base, gain) in facts.attributes {
        let row = commands
            .spawn((
                Node {
                    height: shot.val(59.0),
                    align_items: AlignItems::Center,
                    column_gap: shot.val(14.0),
                    ..default()
                },
                ChildOf(block),
            ))
            .id();
        commands.spawn((
            Node {
                width: shot.val(40.0),
                height: shot.val(40.0),
                ..default()
            },
            kit.art(mark),
            ChildOf(row),
        ));
        let words = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                ChildOf(row),
            ))
            .id();
        commands.spawn((
            shot.text(kit, label, (WORD_FACE, 16.0, 2.0), theme::INK_5),
            ChildOf(words),
        ));
        // One text in two inks: the gain stands on the value's baseline by being set with it.
        let value = commands
            .spawn((
                shot.text(kit, base.to_string(), (Face::Bold, 24.0, 0.0), theme::INK_1),
                ChildOf(words),
            ))
            .id();
        commands.spawn((
            TextSpan::new(format!(" + {gain:.1}")),
            TextFont {
                font: kit.fonts.face(Face::Regular).clone().into(),
                font_size: FontSize::Px(shot.size(24.0)),
                ..default()
            },
            TextColor(theme::INK_3),
            ChildOf(value),
        ));
    }
}

/// Six figures in two columns, and how the hero attacks.
fn stats(commands: &mut Commands, kit: &Kit, column: Entity, facts: &Facts) {
    let shot = PICK;
    let block = commands
        .spawn((
            Name::new("pick panel stats"),
            Node {
                width: percent(100),
                height: shot.val(r::PANEL_STATS[1]),
                padding: UiRect::new(shot.val(34.0), Val::ZERO, shot.val(8.0), Val::ZERO),
                flex_wrap: FlexWrap::Wrap,
                align_content: AlignContent::FlexStart,
                column_gap: shot.val(14.0),
                ..default()
            },
            BackgroundColor(Color::srgba_u8(0x02, 0x03, 0x04, 0xF0)),
            ChildOf(column),
        ))
        .id();
    // The original's six, in its order: down the first column damage, armor and move speed, down
    // the second the attack rate, the attack speed and the attack range.
    let figures = [
        ("hud.damage", facts.damage.to_string()),
        ("hud.armor", format!("{:.1}", facts.armor)),
        ("hud.speed", facts.move_speed.to_string()),
        ("hud.attack_speed2", format!("{:.1}", facts.attack_time)),
        ("hud.attack_speed3", facts.attack_speed.to_string()),
        ("hud.attack_range", facts.attack_range.to_string()),
    ];
    // Two columns of three: the figures go down the first, then down the second.
    for at in [0, 3, 1, 4, 2, 5] {
        let (mark, text) = &figures[at];
        let cell = commands
            .spawn((
                Node {
                    width: shot.val(100.0),
                    height: shot.val(30.0),
                    align_items: AlignItems::Center,
                    column_gap: shot.val(8.0),
                    ..default()
                },
                ChildOf(block),
            ))
            .id();
        commands.spawn((
            Node {
                width: shot.val(22.0),
                height: shot.val(22.0),
                ..default()
            },
            kit.art(mark),
            ChildOf(cell),
        ));
        commands.spawn((
            shot.text(kit, text.clone(), (Face::Semibold, 21.0, 0.0), theme::INK_1),
            ChildOf(cell),
        ));
    }
    let attack = if facts.ranged { "RANGED" } else { "MELEE" };
    commands.spawn((
        shot.text(
            kit,
            attack,
            (Face::Regular, 25.0, 2.0),
            Color::srgb_u8(0x6E, 0x93, 0xD8),
        ),
        Node {
            margin: UiRect::top(shot.val(26.0)),
            ..default()
        },
        ChildOf(block),
    ));
}

/// A pool's bar: what the hero starts the match with in the middle, what it regains a second at
/// the right.
fn pool_bar(
    commands: &mut Commands,
    kit: &Kit,
    column: Entity,
    pool: &str,
    (value, regain): (i32, f32),
    ground: (Color, Color),
) {
    let shot = PICK;
    let bar = commands
        .spawn((
            Name::new(format!("pick panel {pool} bar")),
            Spot(if pool == "health" {
                "pick panel health bar"
            } else {
                "pick panel mana bar"
            }),
            Node {
                width: shot.val(r::PANEL_BAR[0]),
                height: shot.val(r::PANEL_BAR[1]),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(shot.val(3.0)),
                ..default()
            },
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.6)),
            theme::downward(ground),
            ChildOf(column),
        ))
        .id();
    commands.spawn((
        shot.text(
            kit,
            format!("{value} / {value}"),
            (Face::Bold, 28.0, 0.0),
            theme::INK_1,
        ),
        TextShadow {
            offset: Vec2::new(0.0, 1.0),
            color: Color::BLACK,
        },
        ChildOf(bar),
    ));
    commands.spawn((
        shot.text(
            kit,
            format!("+{regain:.1}"),
            (Face::Semibold, 20.0, 0.0),
            Color::srgb_u8(0xB5, 0xE8, 0xA8),
        ),
        Node {
            position_type: PositionType::Absolute,
            right: shot.val(8.0),
            ..default()
        },
        ChildOf(bar),
    ));
}
