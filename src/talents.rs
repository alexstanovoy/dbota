//! The talent tree, as Dota draws it: a round button left of the abilities,
//! Dota's tree icon (four rows of two pips where the cache has none), lit
//! while a skill point could go into the tree; over it, while it is hovered
//! or pinned by a click, the tree itself, the level-25 tier on top, the two
//! choices of each tier either side of its level, and under them the
//! Attribute Bonus with a pip for each of its levels, as Dota levels it since
//! the bonus left the ability row; a click on a choice or on the Attribute
//! Bonus learns it. It shows the tree of whichever hero the bottom panel is
//! about, an enemy's too.

use bevy::prelude::*;
use bota_client_core::talent_levels;
use bota_proto::TalentSlot;

use crate::dota_text::Texts;
use crate::faces::talents;
use crate::hud::Spawner;
use crate::session::Session;
use crate::ui;

/// The button left of the abilities.
#[derive(Component)]
pub struct TalentButton;

/// One pip of the button: a talent slot.
#[derive(Component)]
struct Pip(u8);

/// The tree over the button.
#[derive(Component)]
struct TalentPanel;

/// One choice of the tree, by talent slot.
#[derive(Component)]
pub struct TalentCell(pub u8);

/// The text of a choice.
#[derive(Component)]
struct CellText(u8);

/// The level badge of a tier.
#[derive(Component)]
struct TierLevel(u8);

/// The branch from a tier's badge to one of its choices, by talent slot.
#[derive(Component)]
struct Branch(u8);

/// Dota's tree icon on the button, where the cache has it.
#[derive(Component)]
struct TalentArt {
    /// The icon as it is, and lit while a point could go into the tree.
    plain: Handle<Image>,
    lit: Handle<Image>,
}

/// The tiers of the tree, hidden for a hero without one.
#[derive(Component)]
struct TreeBody;

/// The Attribute Bonus row under the tiers: a click spends a skill point on
/// it.
#[derive(Component)]
pub struct AttributeRow;

/// One level pip of the Attribute Bonus row.
#[derive(Component)]
struct AttributePip(u8);

/// The talent button's size, and the tree's width.
const BUTTON: f32 = 60.0;
const PANEL_WIDTH: f32 = 560.0;

fn branch() -> impl Bundle {
    (
        Node {
            width: px(16),
            height: px(4),
            ..default()
        },
        BackgroundColor(Color::srgb(0.22, 0.24, 0.28)),
    )
}

const GOLD: Color = Color::srgb(0.95, 0.75, 0.3);
const OPEN: Color = Color::srgb(0.75, 0.78, 0.84);
const SHUT: Color = Color::srgb(0.38, 0.4, 0.45);

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (toggle, show_button, show, show_attribute, light).run_if(crate::select::match_or_tactical),
    );
}

/// The button, round, as the first of the ability row, and the tree over
/// the row's `column`.
pub fn spawn_button(kit: &mut Spawner, row: Entity, column: Entity) {
    let button = kit
        .commands
        .spawn((
            TalentButton,
            Button,
            Node {
                width: px(BUTTON),
                height: px(BUTTON),
                align_self: AlignSelf::Center,
                margin: UiRect::bottom(px(10)),
                border: UiRect::all(px(2)),
                border_radius: BorderRadius::MAX,
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(3),
                ..default()
            },
            BorderColor::all(Color::srgb(0.2, 0.22, 0.27)),
            BackgroundColor(ui::PANEL_DEEP),
            ChildOf(row),
        ))
        .id();
    let art = (kit.manifest.file("hud.talents")).zip(kit.manifest.file("hud.talents_learnable"));
    if let Some((plain, lit)) = art {
        let plain = kit.server.load(plain);
        kit.commands.spawn((
            TalentArt {
                plain: plain.clone(),
                lit: kit.server.load(lit),
            },
            ImageNode::new(plain),
            Node {
                border_radius: BorderRadius::MAX,
                ..ui::fill()
            },
            Pickable::IGNORE,
            ChildOf(button),
        ));
    } else {
        spawn_drawn_tree(kit.commands, button);
    }
    spawn_panel(kit, column);
}

/// A small tree drawn in the button where the cache has no icon: a trunk
/// and a pip for each talent, lit as they are learned.
fn spawn_drawn_tree(commands: &mut Commands, button: Entity) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            width: px(3),
            top: px(9),
            bottom: px(9),
            ..default()
        },
        BackgroundColor(Color::srgb(0.45, 0.42, 0.36)),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    // The level-25 tier on top, as the tree.
    for tier in (0..4u8).rev() {
        let line = commands
            .spawn((
                Node {
                    column_gap: px(5),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(button),
            ))
            .id();
        for side in 0..2u8 {
            commands.spawn((
                Pip(tier * 2 + side),
                Node {
                    width: px(8),
                    height: px(5),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(SHUT),
                Pickable::IGNORE,
                ChildOf(line),
            ));
        }
    }
}

/// The tree over the ability row: the tiers, then the Attribute Bonus.
fn spawn_panel(kit: &mut Spawner, column: Entity) {
    let commands = &mut *kit.commands;
    let fonts = kit.fonts;
    let panel = commands
        .spawn((
            TalentPanel,
            Interaction::default(),
            crate::hud::Blocker,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: percent(50),
                margin: UiRect {
                    left: px(-PANEL_WIDTH / 2.0),
                    bottom: px(10.0),
                    ..default()
                },
                width: px(PANEL_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                padding: UiRect::all(px(8)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.02, 0.03, 0.05, 0.96)),
            BorderColor::all(ui::EDGE),
            GlobalZIndex(20),
            Visibility::Hidden,
            ChildOf(column),
        ))
        .id();
    let title = kit
        .texts
        .label("DOTA_StatBranch_TooltipTitle", &[])
        .unwrap_or_else(|| "Talent Tree".to_string());
    commands.spawn((
        ui::text(title.to_uppercase(), &fonts.bold, 12.0, ui::INK_DIM),
        Node {
            align_self: AlignSelf::Center,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(panel),
    ));
    spawn_tree_body(commands, fonts, panel);
    spawn_attribute_row(commands, fonts, kit.texts, panel);
}

/// The tiers, of a hero that has a tree, on the trunk their level badges
/// sit on.
fn spawn_tree_body(commands: &mut Commands, fonts: &crate::ui::Fonts, panel: Entity) {
    let body = commands
        .spawn((
            TreeBody,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: px(8),
            bottom: px(8),
            width: px(4),
            margin: UiRect::left(px(-2)),
            ..default()
        },
        BackgroundColor(Color::srgb(0.22, 0.24, 0.28)),
        Pickable::IGNORE,
        ChildOf(body),
    ));
    for tier in (0..4u8).rev() {
        spawn_tier(commands, fonts, body, tier);
    }
}

/// One tier of the tree: its two choices either side of its level.
fn spawn_tier(commands: &mut Commands, fonts: &crate::ui::Fonts, panel: Entity, tier: u8) {
    let line = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    for side in 0..2u8 {
        let slot = tier * 2 + side;
        if side == 1 {
            commands.spawn((Branch(tier * 2), branch(), Pickable::IGNORE, ChildOf(line)));
            commands.spawn((
                TierLevel(tier),
                Node {
                    width: px(34),
                    height: px(34),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(17)),
                    ..default()
                },
                BorderColor::all(ui::EDGE),
                BackgroundColor(ui::PANEL_DEEP),
                children![(
                    ui::text(
                        talent_levels(TalentSlot(2 * tier)).0.to_string(),
                        &fonts.bold,
                        14.0,
                        OPEN
                    ),
                    Pickable::IGNORE,
                )],
                ChildOf(line),
            ));
            commands.spawn((
                Branch(tier * 2 + 1),
                branch(),
                Pickable::IGNORE,
                ChildOf(line),
            ));
        }
        let cell = commands
            .spawn((
                TalentCell(slot),
                Button,
                Node {
                    width: px(220),
                    min_height: px(34),
                    padding: UiRect::axes(px(8), px(4)),
                    justify_content: if side == 0 {
                        JustifyContent::FlexEnd
                    } else {
                        JustifyContent::FlexStart
                    },
                    align_items: AlignItems::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BorderColor::all(ui::EDGE),
                BackgroundColor(ui::PANEL_DEEP),
                ChildOf(line),
            ))
            .id();
        commands.spawn((
            CellText(slot),
            ui::text("", &fonts.regular, 13.0, OPEN),
            TextLayout::justify(if side == 0 {
                Justify::Right
            } else {
                Justify::Left
            }),
            Pickable::IGNORE,
            ChildOf(cell),
        ));
    }
}

/// The Attribute Bonus under the tiers, as Dota's tree has it: its name and
/// a pip for each level it may take.
fn spawn_attribute_row(
    commands: &mut Commands,
    fonts: &crate::ui::Fonts,
    texts: &Texts,
    panel: Entity,
) {
    let name = texts
        .label("DOTA_LevelUpAttributes", &[])
        .unwrap_or_else(|| {
            format!(
                "+{} to all attributes",
                bota_core::rules::ATTRIBUTE_BONUS_POINTS
            )
        });
    let row = commands
        .spawn((
            AttributeRow,
            Button,
            Node {
                margin: UiRect::top(px(6)),
                padding: UiRect::axes(px(12), px(6)),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BorderColor::all(ui::EDGE),
            BackgroundColor(ui::PANEL_DEEP),
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        ui::text(name, &fonts.regular, 13.0, OPEN),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    let pips = commands
        .spawn((
            Node {
                column_gap: px(4),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    for level in 0..bota_core::rules::ATTRIBUTE_BONUS_MAX_LEVEL {
        commands.spawn((
            AttributePip(level),
            Node {
                width: px(14),
                height: px(6),
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            BackgroundColor(SHUT),
            Pickable::IGNORE,
            ChildOf(pips),
        ));
    }
}

/// The Attribute Bonus row's pips lit to its level, its rim glowing while a
/// skill point could go into it; a click on it learns a level.
fn show_attribute(
    mut session: NonSendMut<Session>,
    time: Res<Time>,
    mut row: Query<(&Interaction, &mut BorderColor), With<AttributeRow>>,
    clicked: Query<&Interaction, (Changed<Interaction>, With<AttributeRow>)>,
    mut pips: Query<(&AttributePip, &mut BackgroundColor)>,
) {
    let (seat, _, _) = crate::hud::subject(&session.0);
    let seat = seat.map(|p| p.slot);
    let mine = seat.is_some() && seat == session.0.my_slot;
    let state = seat.and_then(|s| session.0.talents_of(s));
    let level = state.map_or(0, |t| t.attribute_bonus);
    let learnable = mine && state.is_some_and(|t| t.may_learn_attribute_bonus == Some(true));
    let glow = 0.55 + 0.45 * (time.elapsed_secs() * 4.0).sin().abs();
    for (_, mut rim) in &mut row {
        rim.set_if_neq(BorderColor::all(if learnable {
            GOLD.with_alpha(glow)
        } else {
            ui::EDGE
        }));
    }
    for (pip, mut bg) in &mut pips {
        bg.set_if_neq(BackgroundColor(if pip.0 < level {
            GOLD
        } else if pip.0 == level && learnable {
            GOLD.with_alpha(0.35 + 0.4 * glow)
        } else {
            SHUT
        }));
    }
    if learnable && clicked.iter().any(|i| *i == Interaction::Pressed) {
        session.0.learn_attribute_bonus();
    }
}

/// Dota's tree icon, lit while a skill point could go into a talent or the
/// Attribute Bonus.
fn light(session: NonSend<Session>, mut art: Query<(&TalentArt, &mut ImageNode)>) {
    let core = &session.0;
    let seat = crate::hud::subject(core).0.map(|p| p.slot);
    let mine = seat.is_some() && seat == core.my_slot;
    let lit = seat.and_then(|s| core.talents_of(s)).is_some_and(|t| {
        t.may_learn_attribute_bonus == Some(true)
            || (0..8).any(|k| t.may_learn.is_some_and(|m| m.has(TalentSlot(k))))
    });
    for (art, mut image) in &mut art {
        let want = if lit && mine { &art.lit } else { &art.plain };
        if image.image != *want {
            image.image = want.clone();
        }
    }
}

/// A click on the button, or Dota's talent key, pins the tree open or lets
/// it go (`--tip talents` keeps it open, for screenshots); a click on a
/// choice asks for it.
fn toggle(
    mut session: NonSendMut<Session>,
    args: Res<crate::Args>,
    button: Query<&Interaction, (Changed<Interaction>, With<TalentButton>)>,
    cells: Query<(&Interaction, &TalentCell), Changed<Interaction>>,
    mut panels: ResMut<crate::input::Panels>,
    mut panel: Query<&mut Visibility, With<TalentPanel>>,
    hovered: Query<
        &Interaction,
        Or<(
            With<TalentButton>,
            With<TalentPanel>,
            With<TalentCell>,
            With<AttributeRow>,
        )>,
    >,
) {
    if button.iter().any(|i| *i == Interaction::Pressed) {
        panels.talents = !panels.talents;
    }
    for (interaction, cell) in &cells {
        if *interaction == Interaction::Pressed {
            learn(&mut session.0, cell.0);
        }
    }
    // Every hero has the Attribute Bonus to learn, whether or not it has
    // talents.
    let hero = crate::hud::subject_hero(&session.0).is_some();
    let forced = args.tip.as_deref() == Some("talents");
    let open =
        hero && (forced || panels.talents || hovered.iter().any(|i| *i != Interaction::None));
    for mut v in &mut panel {
        v.set_if_neq(ui::visible(open));
    }
}

/// Asks for a talent of one's own hero; the panel of another hero's tree
/// only shows it.
fn learn(core: &mut bota_client_core::App, slot: u8) {
    let mine =
        crate::hud::subject(core).0.map(|p| p.slot) == core.my_slot && core.my_slot.is_some();
    if mine {
        core.learn_talent(slot);
    }
}

/// A pip of the tree icon: gold once learned, breathing while a point could
/// go into it, dim once its tier is open, shut before.
fn pip_color(learned: bool, learnable: bool, open: bool, glow: f32) -> Color {
    if learned {
        GOLD
    } else if learnable {
        GOLD.with_alpha(0.35 + 0.4 * glow)
    } else if open {
        OPEN.with_alpha(0.5)
    } else {
        SHUT
    }
}

/// The talents of the seat the panel is about, as the view tells them.
fn seat_talents(core: &bota_client_core::App) -> Option<bota_proto::TalentView> {
    crate::hud::subject(core)
        .0
        .and_then(|p| core.talents_of(p.slot))
}

/// The breathing glow of what a point could go into, as Dota's.
fn glow(time: &Time) -> f32 {
    0.55 + 0.45 * (time.elapsed_secs() * 4.0).sin().abs()
}

/// The button, for any hero the panel is about, its rim aglow while a
/// point could go into the tree; the tree's body for a hero that has one.
fn show_button(
    session: NonSend<Session>,
    time: Res<Time>,
    mut button: Query<(&mut Visibility, &mut BorderColor), With<TalentButton>>,
    mut body: Query<&mut Visibility, (With<TreeBody>, Without<TalentButton>)>,
) {
    let core = &session.0;
    let hero = crate::hud::subject_hero(core);
    let any = seat_talents(core).is_some_and(|t| {
        t.may_learn_attribute_bonus == Some(true)
            || (0..8).any(|k| t.may_learn.is_some_and(|m| m.has(TalentSlot(k))))
    });
    for mut v in &mut body {
        v.set_if_neq(ui::visible(hero.and_then(|h| talents(h.0)).is_some()));
    }
    for (mut v, mut rim) in &mut button {
        v.set_if_neq(ui::visible(hero.is_some()));
        rim.set_if_neq(BorderColor::all(if any {
            GOLD.with_alpha(glow(&time))
        } else {
            Color::srgb(0.2, 0.22, 0.27)
        }));
    }
}

/// What the tree of the panel's hero shows: its names, which are learned,
/// which a skill point could go into, which tiers its level has opened.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    time: Res<Time>,
    mut pips: Query<(&Pip, &mut BackgroundColor), Without<TalentCell>>,
    mut cells: Query<
        (&TalentCell, &mut BorderColor, &mut BackgroundColor),
        (Without<Pip>, Without<Branch>, Without<TierLevel>),
    >,
    mut branches: Query<(&Branch, &mut BackgroundColor), (Without<Pip>, Without<TalentCell>)>,
    mut badges: Query<(&TierLevel, &mut BorderColor), (Without<TalentCell>, Without<TalentButton>)>,
    mut texts: Query<(&CellText, &mut Text, &mut TextColor)>,
) {
    let core = &session.0;
    let (seat, unit, _) = crate::hud::subject(core);
    let tree = crate::hud::subject_hero(core).and_then(|h| talents(h.0));
    let state = seat_talents(core);
    let learned = |slot: u8| state.is_some_and(|t| t.learned.has(TalentSlot(slot)));
    let learnable =
        |slot: u8| state.is_some_and(|t| t.may_learn.is_some_and(|m| m.has(TalentSlot(slot))));
    let glow = glow(&time);
    let Some(tree) = tree else {
        return;
    };
    // The body's level, the seat's while it has none, and none before the
    // match.
    let level = unit.map(|u| u.level).or(seat.map(|p| p.level)).unwrap_or(0);
    let open = |slot: u8| level >= talent_levels(TalentSlot(slot)).0;
    for (pip, mut bg) in &mut pips {
        let (got, may, opened) = (learned(pip.0), learnable(pip.0), open(pip.0));
        bg.set_if_neq(BackgroundColor(pip_color(got, may, opened, glow)));
    }
    for (branch, mut bg) in &mut branches {
        bg.set_if_neq(BackgroundColor(if learned(branch.0) {
            GOLD
        } else if learnable(branch.0) {
            GOLD.with_alpha(0.3 + 0.5 * glow)
        } else {
            Color::srgb(0.22, 0.24, 0.28)
        }));
    }
    for (badge, mut rim) in &mut badges {
        let lit = learned(badge.0 * 2) || learned(badge.0 * 2 + 1);
        rim.set_if_neq(BorderColor::all(if lit { GOLD } else { ui::EDGE }));
    }
    for (cell, mut rim, mut bg) in &mut cells {
        let (r, b) = if learned(cell.0) {
            (GOLD, Color::srgba(0.45, 0.32, 0.08, 0.9))
        } else if learnable(cell.0) {
            (GOLD.with_alpha(glow), Color::srgba(0.2, 0.17, 0.08, 0.9))
        } else {
            (ui::EDGE, ui::PANEL_DEEP)
        };
        rim.set_if_neq(BorderColor::all(r));
        bg.set_if_neq(BackgroundColor(b));
    }
    for (cell, mut text, mut color) in &mut texts {
        let name = &tree[usize::from(cell.0)].name;
        if text.0 != *name {
            text.0.clone_from(name);
        }
        color.set_if_neq(TextColor(if learned(cell.0) {
            Color::WHITE
        } else if learnable(cell.0) || open(cell.0) {
            OPEN
        } else {
            SHUT
        }));
    }
}
