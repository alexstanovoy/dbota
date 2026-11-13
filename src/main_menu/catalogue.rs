//! The playable catalogue and hero page, from the simulator's data and the installed art/text.
use super::{
    core::{Act, Page},
    kit::{Kit, MenuButton, Tone},
};
use crate::{dota_text::Texts, theme};
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bota_proto::{Attribute, HeroId};

/// The way back to the grid is a button of this height, at the page's left.
const BACK_HEIGHT: f32 = 44.0;

/// An ability's icon, across, and the innate's round mark in its place.
const ABILITY_ICON: f32 = 48.0;
const INNATE_BADGE: f32 = 36.0;
/// The hero's words stand in a column at the page's left, over the hero standing in its middle.
const COLUMN_WIDTH: f32 = 500.0;

/// A hero's card in the grid, which rises under the cursor and with the keyboard.
#[derive(Component)]
pub struct HeroCard;

/// How much larger the card under the cursor is: little enough that it stays within the gaps
/// between the cards (8 px against the card's 47 by 82), so it covers none of its neighbours and
/// the cursor on its old edge never leaves it as it grows.
const RAISED: f32 = 1.09;

pub fn grid(kit: &mut Kit, root: Entity) {
    let band = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            top: px(54),
            left: px(0),
            right: px(0),
            height: px(44),
            padding: UiRect::left(px(180)),
            align_items: AlignItems::Center,
            border: UiRect::bottom(px(1)),
            ..default()
        },
    );
    kit.commands.entity(band).insert((
        BackgroundColor(Color::BLACK.with_alpha(theme::dim(0.35))),
        BorderColor::all(theme::DIVIDER),
    ));
    kit.typed(band, "HEROES", theme::CATALOGUE_HEADING, theme::INK_2);
    let groups = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: px(52),
            top: px(124),
            column_gap: px(38),
            ..default()
        },
    );
    for (attribute, name, icon, columns) in [
        (Some(Attribute::Strength), "STRENGTH", "hud.strength", 6),
        (Some(Attribute::Agility), "AGILITY", "hud.agility", 6),
        (
            Some(Attribute::Intelligence),
            "INTELLIGENCE",
            "hud.intelligence",
            6,
        ),
        (None, "UNIVERSAL", "hud.universal", 4),
    ] {
        hero_group(kit, groups, attribute, name, icon, columns);
    }
}

fn hero_group(
    kit: &mut Kit,
    groups: Entity,
    attribute: Option<Attribute>,
    name: &str,
    icon: &str,
    columns: usize,
) {
    let group = kit.node(
        groups,
        Node {
            width: px(columns as f32 * 47.0 + (columns - 1) as f32 * 8.0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            ..default()
        },
    );
    let heading = kit.node(
        group,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(7),
            ..default()
        },
    );
    kit.art(
        heading,
        icon,
        Node {
            width: px(18),
            height: px(18),
            ..default()
        },
    );
    kit.typed(heading, name, theme::CATALOGUE_HEADING, theme::INK_2);
    let cards = kit.node(
        group,
        Node {
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(8),
            column_gap: px(8),
            min_height: px(82),
            ..default()
        },
    );
    let mut heroes: Vec<_> = bota_core::HEROES
        .iter()
        .filter(|hero| hero.unit.primary == attribute)
        .collect();
    heroes.sort_by_key(|hero| hero.name);
    for hero in heroes {
        hero_card(kit, cards, hero);
    }
}

fn hero_card(kit: &mut Kit, cards: Entity, hero: &bota_core::HeroDef) {
    let card = kit.node(
        cards,
        Node {
            width: px(47),
            height: px(82),
            flex_shrink: 0.0,
            ..default()
        },
    );
    kit.commands.entity(card).insert((
        Button,
        Pickable::default(),
        MenuButton(Act::InspectHero(hero.id)),
        HeroCard,
        super::highlight::Highlight { scale: RAISED },
        Name::new(hero.name),
    ));
    tall(
        kit,
        card,
        hero.id,
        Node {
            width: percent(100),
            height: percent(100),
            ..default()
        },
    );
}

pub fn tall(kit: &mut Kit, parent: Entity, hero: HeroId, node: Node) {
    let frame = kit.node(parent, node);
    if let Some(path) = kit.manifest.portrait(hero.0) {
        kit.commands
            .entity(frame)
            .insert((ImageNode::new(kit.server.load(path)), crate::ui::Cover));
    }
}

pub fn hero(kit: &mut Kit, root: Entity, id: HeroId, texts: &Texts, record: &str, demo: bool) {
    let Some(hero) = bota_core::hero_def(id) else {
        return;
    };
    let body = hero_body(kit, root, id);
    hero_header(kit, body, hero);
    skills(kit, body, hero, texts);
    stats(kit, body, hero);
    hero_actions(kit, body, id, record, demo);
    talents(kit, body, hero, texts);
}

/// The page's navigation line, which has the way back to the grid at its left and the neighbouring
/// heroes at its right, and under it the column holding the hero's words.
fn hero_body(kit: &mut Kit, root: Entity, id: HeroId) -> Entity {
    let page = kit.node(
        root,
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(18),
            ..default()
        },
    );
    kit.commands.entity(page).insert(Name::new("hero page"));
    let line = kit.node(
        page,
        Node {
            width: percent(100),
            align_items: AlignItems::Center,
            column_gap: px(14),
            ..default()
        },
    );
    let back = kit.button(
        line,
        "‹ ALL HEROES",
        Act::Go(Page::Heroes),
        Tone::Secondary,
        BACK_HEIGHT,
    );
    kit.commands
        .entity(back)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.align_self = AlignSelf::FlexStart;
        });
    kit.node(
        line,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    neighbours(kit, line, id);
    kit.node(
        page,
        Node {
            width: px(COLUMN_WIDTH),
            // The original game's page starts its words 0.046 of the screen's width in and 0.23 of
            // its height down.
            margin: UiRect::new(px(48), px(0), px(60), px(0)),
            flex_direction: FlexDirection::Column,
            row_gap: px(14),
            ..default()
        },
    )
}

/// The heroes before and after this one in the catalogue, one press away.
fn neighbours(kit: &mut Kit, line: Entity, id: HeroId) {
    let heroes = bota_core::HEROES;
    let Some(at) = heroes.iter().position(|hero| hero.id == id) else {
        return;
    };
    let before = &heroes[(at + heroes.len() - 1) % heroes.len()];
    let after = &heroes[(at + 1) % heroes.len()];
    // With two heroes the one before is the one after: it is offered once.
    let mut offered = vec![(before, format!("‹ {}", before.name.to_uppercase()))];
    if after.id == before.id {
        offered[0].1 = format!("{} ›", after.name.to_uppercase());
    } else {
        offered.push((after, format!("{} ›", after.name.to_uppercase())));
    }
    for (hero, label) in offered {
        if hero.id != id {
            kit.quiet(
                line,
                &label,
                Act::InspectHero(hero.id),
                theme::LABEL,
                theme::LINK,
            );
        }
    }
}

fn hero_header(kit: &mut Kit, body: Entity, hero: &bota_core::HeroDef) {
    let header = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            margin: UiRect::bottom(px(-3)),
            ..default()
        },
    );
    let attribute = hero_identity(kit, header, hero);
    kit.text(
        header,
        format!(
            "{attribute} · {}",
            if hero.unit.projectile_speed.is_some() {
                "Ranged"
            } else {
                "Melee"
            }
        ),
        15.0,
        theme::INK_3,
    );
}

fn hero_identity(kit: &mut Kit, header: Entity, hero: &bota_core::HeroDef) -> &'static str {
    let title = kit.node(
        header,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(16),
            ..default()
        },
    );
    let (attribute, icon) = match hero.unit.primary {
        Some(Attribute::Strength) => ("Strength", "hud.strength"),
        Some(Attribute::Agility) => ("Agility", "hud.agility"),
        Some(Attribute::Intelligence) => ("Intelligence", "hud.intelligence"),
        None => ("Universal", "hud.universal"),
    };
    kit.art(
        title,
        icon,
        Node {
            width: px(38),
            height: px(38),
            ..default()
        },
    );
    kit.typed(
        title,
        hero.name.to_uppercase(),
        theme::HERO_NAME,
        theme::INK_1,
    );
    attribute
}

/// One row of icons: the innate as the game's round mark, then the abilities. Under the pointer an
/// icon shows the game's tooltip of the ability.
fn skills(kit: &mut Kit, body: Entity, hero: &bota_core::HeroDef, _texts: &Texts) {
    let abilities = kit.node(
        body,
        Node {
            column_gap: px(8),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    let mut shown = std::collections::HashSet::new();
    for ability in hero
        .innate
        .into_iter()
        .chain(hero.abilities.iter().copied())
    {
        let ability = bota_core::learn_group(ability);
        let Some(def) = bota_core::ability_def(ability) else {
            continue;
        };
        if shown.insert(ability) {
            skill(kit, abilities, def, Some(ability) == hero.innate);
        }
    }
}

/// An ability's icon (the innate's round mark, as the game's interface draws it).
fn skill(kit: &mut Kit, abilities: Entity, def: &bota_core::AbilityDef, innate: bool) {
    let ability = def.id;
    let frame = kit
        .commands
        .spawn((
            crate::tips::AbilityTile(ability.0),
            Node {
                width: px(ABILITY_ICON),
                height: px(ABILITY_ICON),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(theme::PANEL_EDGE),
            Name::new(format!("ability {}", def.name.to_lowercase())),
            ChildOf(abilities),
        ))
        .id();
    if innate {
        let badge = crate::hud::innate_badge(
            &mut kit.commands,
            kit.manifest,
            kit.server,
            frame,
            INNATE_BADGE,
        );
        // The tile is what the pointer is over, not the mark in it.
        kit.commands.entity(badge).insert(Pickable::IGNORE);
        kit.commands
            .entity(frame)
            .insert(BorderColor::all(Color::NONE));
    } else if let Some(icon) = kit.manifest.ability_icon(ability.0) {
        kit.commands.spawn((
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            ImageNode::new(kit.server.load(icon)),
            Pickable::IGNORE,
            ChildOf(frame),
        ));
    }
}

/// Under the stats: the green DEMO HERO, which starts Dota's Demo Hero with this hero where one
/// can be started, and the player's record with it.
fn hero_actions(kit: &mut Kit, body: Entity, id: HeroId, record: &str, demo: bool) {
    let bottom = kit.node(
        body,
        Node {
            column_gap: px(14),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    if demo {
        kit.button(bottom, "DEMO HERO", Act::Demo(id.0), Tone::Primary, 40.0);
    }
    kit.text(bottom, record, 13.0, theme::INK_3);
}

/// The numbers: the three attributes, each as its base and its gain by level on one line, beside
/// the combat figures and the health and mana the hero starts with.
fn stats(kit: &mut Kit, body: Entity, hero: &bota_core::HeroDef) {
    let block = kit.node(
        body,
        Node {
            padding: UiRect::vertical(px(12)),
            column_gap: px(22),
            border: UiRect::vertical(px(1)),
            ..default()
        },
    );
    kit.commands
        .entity(block)
        .insert(BorderColor::all(theme::DIVIDER));
    attributes(kit, block, hero);
    combat(kit, block, hero);
}

fn attributes(kit: &mut Kit, root: Entity, hero: &bota_core::HeroDef) {
    let column = kit.node(
        root,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            ..default()
        },
    );
    let unit = hero.unit;
    for (label, icon, value, gain) in [
        (
            "STRENGTH",
            "hud.strength",
            unit.attributes.strength,
            unit.per_level.attributes.strength,
        ),
        (
            "AGILITY",
            "hud.agility",
            unit.attributes.agility,
            unit.per_level.attributes.agility,
        ),
        (
            "INTELLIGENCE",
            "hud.intelligence",
            unit.attributes.intelligence,
            unit.per_level.attributes.intelligence,
        ),
    ] {
        let tile = kit.node(
            column,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            },
        );
        kit.typed(tile, label, theme::CAPTION, theme::INK_4);
        let line = kit.node(
            tile,
            Node {
                align_items: AlignItems::Center,
                column_gap: px(6),
                ..default()
            },
        );
        kit.art(
            line,
            icon,
            Node {
                width: px(16),
                height: px(16),
                ..default()
            },
        );
        // One text in two inks: the gain stands on the value's baseline by being set with it.
        let words = kit.bold(line, value.to_int().to_string(), 18.0, theme::INK_1);
        let font = TextFont {
            font: kit.fonts.face(theme::Face::Regular).clone().into(),
            font_size: FontSize::Px(18.0),
            ..default()
        };
        kit.commands.spawn((
            TextSpan::new(format!(" + {:.1}", gain.to_f32())),
            font,
            TextColor(theme::INK_3),
            ChildOf(words),
        ));
    }
}

/// The figures of a fight in a row (each over its name), and under them the health and mana bars
/// with what they regain in a second.
fn combat(kit: &mut Kit, root: Entity, hero: &bota_core::HeroDef) {
    let unit = hero.unit;
    let damage = bota_core::base_damage_least(unit, 1);
    let attack_time = bota_core::attack_duration(
        unit.attack_time,
        unit.attack_speed + bota_core::agility_attack_speed(unit.attributes),
    );
    let armor = unit.armor as f32 + bota_core::agility_armor(unit.attributes).to_f32();
    let health = unit.max_hp
        + (unit.attributes.strength
            * bota_proto::Fixed::from_int(bota_core::rules::HP_PER_STRENGTH))
        .to_int();
    let mana = unit.max_mana
        + (unit.attributes.intelligence
            * bota_proto::Fixed::from_int(bota_core::rules::MANA_PER_INTELLIGENCE))
        .to_int();
    let per_second = |base: bota_proto::Fixed, per_point: bota_proto::Fixed, points| {
        ((base + points * per_point)
            * bota_proto::Fixed::from_int(bota_core::rules::TICKS_PER_SECOND as i32))
        .to_f32()
    };
    let health_regen = per_second(
        unit.hp_regen,
        bota_core::rules::HP_REGEN_PER_STRENGTH,
        unit.attributes.strength,
    );
    let mana_regen = per_second(
        unit.mana_regen,
        bota_core::rules::MANA_REGEN_PER_INTELLIGENCE,
        unit.attributes.intelligence,
    );
    let column = kit.node(
        root,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            ..default()
        },
    );
    let figures = kit.node(
        column,
        Node {
            column_gap: px(18),
            ..default()
        },
    );
    for (label, value) in [
        (
            "DAMAGE",
            format!("{damage}–{}", damage + unit.damage_spread),
        ),
        ("ARMOR", format!("{armor:.1}")),
        ("MOVE SPEED", unit.move_speed.to_string()),
        (
            "ATTACK TIME",
            format!("{:.1} s", attack_time as f32 / 1000.0),
        ),
        ("ATTACK RANGE", unit.attack_range.to_string()),
    ] {
        let one = kit.node(
            figures,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                ..default()
            },
        );
        let name = kit.typed(one, label, theme::CAPTION, theme::INK_4);
        kit.commands.entity(name).insert(TextLayout::no_wrap());
        kit.bold(one, value, 15.0, theme::INK_1);
    }
    bar(kit, column, "MAX HEALTH", health, health_regen, theme::TICK);
    bar(
        kit,
        column,
        "MAX MANA",
        mana,
        mana_regen,
        (
            Color::srgb_u8(0x2A, 0x6A, 0x95),
            Color::srgb_u8(0x3E, 0x8F, 0xC4),
        ),
    );
}

/// A bar of a pool: what the hero starts with in its middle, what it regains a second at its
/// right.
fn bar(kit: &mut Kit, column: Entity, label: &str, value: i32, regen: f32, ground: (Color, Color)) {
    kit.typed(column, label, theme::CAPTION, theme::INK_4);
    let bar = kit.node(
        column,
        Node {
            width: px(260),
            height: px(22),
            padding: UiRect::horizontal(px(8)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
    );
    kit.commands
        .entity(bar)
        .insert(crate::theme::downward(ground));
    kit.node(bar, Node::default());
    kit.bold(bar, value.to_string(), 15.0, theme::INK_1);
    kit.text(bar, format!("+{:.1}", regen + 0.005), 12.0, theme::INK_1);
}

fn talents(kit: &mut Kit, root: Entity, hero: &bota_core::HeroDef, texts: &Texts) {
    let Some(tree) = hero.talents else {
        return;
    };
    let section = kit.node(
        root,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            margin: UiRect::top(px(10)),
            ..default()
        },
    );
    kit.typed(section, "TALENTS", theme::CAPTION, theme::INK_4);
    let rows = kit.node(
        section,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    let Some(fallback) = crate::faces::talents(hero.id.0) else {
        return;
    };
    for tier in (0..bota_core::rules::TALENT_LEVELS.len()).rev() {
        let row = kit.node(
            rows,
            Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(8),
                ..default()
            },
        );
        for side in 0..2 {
            if side == 1 {
                talent_level(kit, row, tier);
            }
            let index = tier * 2 + side;
            let text = texts
                .get(tree[index].key())
                .map_or(fallback[index].name.as_str(), |entry| entry.name.as_str());
            talent_cell(kit, row, text, side);
        }
    }
}

fn talent_level(kit: &mut Kit, row: Entity, tier: usize) {
    let level = kit.node(
        row,
        Node {
            width: px(35),
            height: px(35),
            flex_shrink: 0.0,
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(18)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    kit.commands
        .entity(level)
        .insert(BorderColor::all(theme::GOLD));
    kit.bold(
        level,
        bota_core::rules::TALENT_LEVELS[tier].to_string(),
        15.0,
        theme::GOLD,
    );
}

fn talent_cell(kit: &mut Kit, row: Entity, text: &str, side: usize) {
    let cell = kit.node(
        row,
        Node {
            width: px(264),
            height: px(35),
            padding: UiRect::all(px(6)),
            align_items: AlignItems::Center,
            justify_content: if side == 0 {
                JustifyContent::FlexEnd
            } else {
                JustifyContent::FlexStart
            },
            ..default()
        },
    );
    kit.commands
        .entity(cell)
        .insert(BackgroundColor(theme::FIELD_SUNKEN));
    let words = kit.text(cell, text, 12.0, theme::INK_2);
    kit.commands
        .entity(words)
        .insert(TextLayout::justify(if side == 0 {
            Justify::Right
        } else {
            Justify::Left
        }));
}
