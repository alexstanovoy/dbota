//! The playable catalogue and hero page, from the simulator's data and the installed art/text.
#[cfg(not(target_arch = "wasm32"))]
use super::kit::Tone;
use super::{
    core::{Act, Page},
    kit::{Kit, MenuButton},
};
use crate::{dota_text::Texts, theme};
use bevy::prelude::*;
use bevy::ui_widgets::Button;
use bota_proto::{Attribute, HeroId};

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

pub fn hero(kit: &mut Kit, root: Entity, id: HeroId, texts: &Texts, record: &str) {
    let Some(hero) = bota_core::hero_def(id) else {
        return;
    };
    let body = hero_body(kit, root, id);
    hero_header(kit, body, hero);
    attributes(kit, body, hero);
    combat(kit, body, hero);
    skills(kit, body, hero, texts);
    talents(kit, body, hero, texts);
    hero_actions(kit, body, id, record);
}

fn hero_body(kit: &mut Kit, root: Entity, id: HeroId) -> Entity {
    let page = kit.node(
        root,
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    kit.quiet(
        page,
        "‹ ALL HEROES",
        Act::Go(Page::Heroes),
        theme::LABEL,
        theme::LINK,
    );
    let columns = kit.node(
        page,
        Node {
            width: percent(100),
            margin: UiRect::top(px(8)),
            padding: UiRect::right(px(36)),
            column_gap: px(80),
            justify_content: JustifyContent::FlexEnd,
            ..default()
        },
    );
    tall(
        kit,
        columns,
        id,
        Node {
            width: px(420),
            height: px(558),
            margin: UiRect::top(px(34)),
            ..default()
        },
    );
    kit.node(
        columns,
        Node {
            width: px(580),
            flex_direction: FlexDirection::Column,
            row_gap: px(12),
            ..default()
        },
    )
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

fn skills(kit: &mut Kit, body: Entity, hero: &bota_core::HeroDef, texts: &Texts) {
    let skills = kit.node(
        body,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(8),
            ..default()
        },
    );
    kit.typed(skills, "ABILITIES", theme::CAPTION, theme::INK_4);
    let abilities = kit.node(
        skills,
        Node {
            column_gap: px(10),
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
        if !shown.insert(ability) {
            continue;
        }
        skill(kit, abilities, def, Some(ability) == hero.innate, texts);
    }
}

fn skill(
    kit: &mut Kit,
    abilities: Entity,
    def: &bota_core::AbilityDef,
    innate: bool,
    texts: &Texts,
) {
    let ability = def.id;
    let column = kit.node(
        abilities,
        Node {
            width: px(66),
            flex_direction: FlexDirection::Column,
            row_gap: px(5),
            ..default()
        },
    );
    if let Some(icon) = kit.manifest.ability_icon(ability.0) {
        let frame = kit.node(
            column,
            Node {
                width: px(66),
                height: px(66),
                border: UiRect::all(px(1)),
                ..default()
            },
        );
        kit.commands
            .entity(frame)
            .insert(BorderColor::all(if innate {
                theme::GOLD
            } else {
                theme::PANEL_EDGE
            }));
        kit.commands.spawn((
            Node {
                width: percent(100),
                height: percent(100),
                ..default()
            },
            ImageNode::new(kit.server.load(icon)),
            ChildOf(frame),
        ));
    }
    let name = texts
        .get(def.key)
        .map_or(def.name, |entry| entry.name.as_str());
    kit.text(column, name, 11.0, theme::INK_3);
    if innate {
        kit.text(column, "INNATE", 10.0, theme::GOLD);
    }
}

fn hero_actions(kit: &mut Kit, body: Entity, _id: HeroId, record: &str) {
    let bottom = kit.node(
        body,
        Node {
            column_gap: px(14),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    #[cfg(not(target_arch = "wasm32"))]
    {
        let demo = kit.button(bottom, "DEMO", Act::Demo(_id.0), Tone::Primary, 52.0);
        kit.commands
            .entity(demo)
            .entry::<Node>()
            .and_modify(|mut node| node.width = px(153));
    }
    kit.text(bottom, record, 14.0, theme::INK_3);
}

fn attributes(kit: &mut Kit, root: Entity, hero: &bota_core::HeroDef) {
    let row = kit.node(
        root,
        Node {
            width: percent(100),
            column_gap: px(8),
            ..default()
        },
    );
    let unit = hero.unit;
    for (label, value, gain) in [
        (
            "STRENGTH",
            unit.attributes.strength,
            unit.per_level.attributes.strength,
        ),
        (
            "AGILITY",
            unit.attributes.agility,
            unit.per_level.attributes.agility,
        ),
        (
            "INTELLIGENCE",
            unit.attributes.intelligence,
            unit.per_level.attributes.intelligence,
        ),
    ] {
        let tile = kit.node(
            row,
            Node {
                flex_grow: 1.0,
                border: UiRect::bottom(px(2)),
                padding: UiRect::all(px(12)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        );
        kit.commands.entity(tile).insert((
            BackgroundColor(theme::FIELD_SUNKEN),
            BorderColor::all(match label {
                "STRENGTH" => theme::LOSS,
                "AGILITY" => theme::RADIANT_TEXT,
                _ => theme::SELECT,
            }),
        ));
        kit.typed(tile, label, theme::CAPTION, theme::INK_4);
        let values = kit.node(
            tile,
            Node {
                align_items: AlignItems::Baseline,
                column_gap: px(4),
                ..default()
            },
        );
        kit.bold(values, value.to_int().to_string(), 22.0, theme::INK_1);
        kit.text(values, format!("+{:.1}", gain.to_f32()), 12.0, theme::INK_3);
    }
}

fn combat(kit: &mut Kit, root: Entity, hero: &bota_core::HeroDef) {
    let unit = hero.unit;
    let damage = bota_core::base_damage_least(unit, 1);
    let health = unit.max_hp
        + (unit.attributes.strength
            * bota_proto::Fixed::from_int(bota_core::rules::HP_PER_STRENGTH))
        .to_int();
    let mana = unit.max_mana
        + (unit.attributes.intelligence
            * bota_proto::Fixed::from_int(bota_core::rules::MANA_PER_INTELLIGENCE))
        .to_int();
    let armor = unit.armor as f32 + bota_core::agility_armor(unit.attributes).to_f32();
    let attack_time = bota_core::attack_duration(
        unit.attack_time,
        unit.attack_speed + bota_core::agility_attack_speed(unit.attributes),
    );
    let row = kit.node(
        root,
        Node {
            width: percent(100),
            padding: UiRect::all(px(12)),
            ..default()
        },
    );
    kit.commands
        .entity(row)
        .insert(BackgroundColor(theme::FIELD_SUNKEN));
    for pairs in [
        [
            (
                "DAMAGE",
                format!("{damage}–{}", damage + unit.damage_spread),
            ),
            ("ARMOR", format!("{armor:.1}")),
        ],
        [
            ("ATTACK RANGE", unit.attack_range.to_string()),
            ("MOVE SPEED", unit.move_speed.to_string()),
        ],
        [
            (
                "ATTACK TIME",
                format!("{:.1} s", attack_time as f32 / 1000.0),
            ),
            ("HEALTH / MANA", format!("{health} / {mana}")),
        ],
    ] {
        let column = kit.node(
            row,
            Node {
                width: percent(33.333),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
        );
        for (label, value) in pairs {
            kit.typed(column, label, theme::CAPTION, theme::INK_4);
            kit.bold(column, value, 15.0, theme::INK_1);
        }
    }
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
            margin: UiRect::top(px(-12)),
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
