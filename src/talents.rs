//! The talent tree, as Dota draws it: a button left of the abilities with
//! four rows of two pips, gold where a talent is learned and glowing while a
//! skill point could go into one; over it, while it is hovered or pinned by a
//! click, the tree itself, the level-25 tier on top, the two choices of each
//! tier either side of its level; a click on a choice learns it. It shows
//! the tree of whichever hero the bottom panel is about, an enemy's too.

use bevy::prelude::*;
use bota_client_core::{TALENT_LEVELS, talents};
use bota_proto::TalentSlot;

use crate::session::Session;
use crate::ui::{self, Fonts};

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
struct TierLevel;

const GOLD: Color = Color::srgb(0.95, 0.75, 0.3);
const OPEN: Color = Color::srgb(0.75, 0.78, 0.84);
const SHUT: Color = Color::srgb(0.38, 0.4, 0.45);

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (toggle, show).run_if(|k: Res<crate::session::MatchKnown>| k.0),
    );
}

/// The button, of the ability slots' height, as the first of their row.
pub fn spawn_button(commands: &mut Commands, fonts: &Fonts, row: Entity, height: f32) {
    let button = commands
        .spawn((
            TalentButton,
            Button,
            Node {
                width: px(height * 0.55),
                height: px(height),
                border: UiRect::all(px(1)),
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::SpaceEvenly,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::srgb(0.2, 0.22, 0.27)),
            BackgroundColor(ui::PANEL_DEEP),
            ChildOf(row),
        ))
        .id();
    // The level-25 tier on top, as the tree.
    for tier in (0..4u8).rev() {
        let line = commands
            .spawn((
                Node {
                    column_gap: px(3),
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
                    width: px(10),
                    height: px(6),
                    border_radius: BorderRadius::all(px(2)),
                    ..default()
                },
                BackgroundColor(SHUT),
                Pickable::IGNORE,
                ChildOf(line),
            ));
        }
    }
    let panel = commands
        .spawn((
            TalentPanel,
            Interaction::default(),
            crate::hud::Blocker,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: px(0),
                margin: UiRect::bottom(px(46)),
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
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        ui::text("TALENTS", &fonts.bold, 12.0, ui::INK_DIM),
        Pickable::IGNORE,
        ChildOf(panel),
    ));
    for tier in (0..4u8).rev() {
        let line = commands
            .spawn((
                Node {
                    column_gap: px(6),
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(panel),
            ))
            .id();
        for side in 0..2u8 {
            let slot = tier * 2 + side;
            if side == 1 {
                commands.spawn((
                    TierLevel,
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
                            TALENT_LEVELS[usize::from(tier)].to_string(),
                            &fonts.bold,
                            14.0,
                            OPEN
                        ),
                        Pickable::IGNORE,
                    )],
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
}

/// A click on the button pins the tree open or lets it go; a click on a
/// choice asks for it.
fn toggle(
    mut session: NonSendMut<Session>,
    button: Query<&Interaction, (Changed<Interaction>, With<TalentButton>)>,
    cells: Query<(&Interaction, &TalentCell), Changed<Interaction>>,
    mut pinned: Local<bool>,
    mut panel: Query<&mut Visibility, With<TalentPanel>>,
    hovered: Query<&Interaction, Or<(With<TalentButton>, With<TalentPanel>, With<TalentCell>)>>,
) {
    if button.iter().any(|i| *i == Interaction::Pressed) {
        *pinned = !*pinned;
    }
    for (interaction, cell) in &cells {
        if *interaction == Interaction::Pressed {
            learn(&mut session.0, cell.0);
        }
    }
    let open = *pinned || hovered.iter().any(|i| *i != Interaction::None);
    for mut v in &mut panel {
        let want = if open {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
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

/// What the tree of the panel's hero shows: its names, which are learned,
/// which a skill point could go into, which tiers its level has opened.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    time: Res<Time>,
    mut pips: Query<(&Pip, &mut BackgroundColor), Without<TalentCell>>,
    mut cells: Query<(&TalentCell, &mut BorderColor, &mut BackgroundColor), Without<Pip>>,
    mut texts: Query<(&CellText, &mut Text, &mut TextColor)>,
    mut button: Query<
        (&mut Visibility, &mut BorderColor),
        (
            With<TalentButton>,
            Without<TalentPanel>,
            Without<TalentCell>,
        ),
    >,
) {
    let core = &session.0;
    let (seat, unit, _) = crate::hud::subject(core);
    let tree = unit.and_then(|u| u.hero).and_then(|h| talents(h.0));
    let state = seat.and_then(|p| core.talents_of(p.slot));
    let learned = |slot: u8| state.is_some_and(|t| t.learned.has(TalentSlot(slot)));
    let learnable = |slot: u8| state.is_some_and(|t| t.learnable.has(TalentSlot(slot)));
    // The glow of what a point could go into, breathing as Dota's does.
    let glow = 0.55 + 0.45 * (time.elapsed_secs() * 4.0).sin().abs();
    let any = (0..8).any(learnable);
    for (mut v, mut rim) in &mut button {
        let want = if tree.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *v != want {
            *v = want;
        }
        *rim = BorderColor::all(if any {
            GOLD.with_alpha(glow)
        } else {
            Color::srgb(0.2, 0.22, 0.27)
        });
    }
    let (Some(tree), Some(unit)) = (tree, unit) else {
        return;
    };
    let open = |slot: u8| unit.level >= TALENT_LEVELS[usize::from(slot / 2)];
    for (pip, mut bg) in &mut pips {
        let c = if learned(pip.0) {
            GOLD
        } else if learnable(pip.0) {
            GOLD.with_alpha(0.35 + 0.4 * glow)
        } else if open(pip.0) {
            OPEN.with_alpha(0.5)
        } else {
            SHUT
        };
        if bg.0 != c {
            bg.0 = c;
        }
    }
    for (cell, mut rim, mut bg) in &mut cells {
        let (r, b) = if learned(cell.0) {
            (GOLD, Color::srgba(0.45, 0.32, 0.08, 0.9))
        } else if learnable(cell.0) {
            (GOLD.with_alpha(glow), Color::srgba(0.2, 0.17, 0.08, 0.9))
        } else {
            (ui::EDGE, ui::PANEL_DEEP)
        };
        *rim = BorderColor::all(r);
        if bg.0 != b {
            bg.0 = b;
        }
    }
    for (cell, mut text, mut color) in &mut texts {
        let name = tree[usize::from(cell.0)].name;
        if text.0 != name {
            text.0 = name.to_string();
        }
        color.0 = if learned(cell.0) {
            Color::WHITE
        } else if learnable(cell.0) || open(cell.0) {
            OPEN
        } else {
            SHUT
        };
    }
}
