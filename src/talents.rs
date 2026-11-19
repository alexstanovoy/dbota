//! The talent tree, as Dota draws it: a round button left of the abilities, Dota's tree icon (four
//! rows of two pips where the cache has none), lit while a talent could be taken, a leaf of each
//! tier once its talent is taken and a dash of the Attribute Bonus for each of its levels, with
//! Dota's gold plus plate over it while a point could go into the tree.
//!
//! Over the button, the tree's window (`DOTAStatBranch`): hovered, the tree as a tooltip, titled
//! TALENT TREE; pinned by a click on the button or the plus, or by Dota's talent key, the window
//! to choose in, titled SELECT A TALENT: the level-25 tier on top, the two choices of each tier
//! either side of its level, a tier with a choice open glowing, and under it the Attribute Bonus
//! with a dash for each of its levels. A click on a choice or on the Attribute Bonus learns it, and
//! so do Dota's talent keys while the window is pinned (1 and 2: the left and the right talent of
//! the lowest tier with one open, 3: the Attribute Bonus); the marks of the keys hang under the
//! window. It shows the tree of whichever hero the bottom panel is about, an enemy's too.

use bevy::math::Rot2;
use bevy::prelude::*;
use bevy::text::LetterSpacing;
use bevy::{
    picking::{events::PointerPress, hover::Hovered, pointer::PointerButton},
    ui_widgets::Button,
};
use bota_client_core::{TalentSide, talent_levels};
use bota_proto::{TalentSlot, TalentView, Talents};

use crate::dota_text::Texts;
use crate::faces::{TalentFace, talents};
use crate::hud::Spawner;
use crate::keys::{Action, KeyLabel};
use crate::session::Session;
use crate::theme::Face;
use crate::ui::{self, dota};

/// The button left of the abilities.
#[derive(Component)]
pub struct TalentButton;

/// Dota's gold plate with a plus over the button, while a point could go into the tree.
#[derive(Component)]
struct TalentPlus;

/// One pip of the drawn button: a talent slot.
#[derive(Component)]
struct Pip(u8);

/// The window over the button.
#[derive(Component)]
pub(crate) struct TalentPanel;

/// The window's backdrop, drawn as the pinned window and as the tooltip.
#[derive(Component)]
struct Backdrop {
    pinned: Handle<Image>,
    tooltip: Handle<Image>,
}

/// The window's title.
#[derive(Component)]
struct Title;

/// One choice of the tree, by talent slot.
#[derive(Component)]
#[require(bevy::picking::hover::DirectlyHovered)]
pub struct TalentCell(pub u8);

/// The text of a choice.
#[derive(Component)]
struct CellText(u8);

/// The level badge of a tier, by tier.
#[derive(Component)]
struct TierLevel(u8);

/// The glow of a tier with a choice open: both choices, only the left, only the right.
#[derive(Component)]
struct Glow {
    tier: u8,
    both: Handle<Image>,
    left: Handle<Image>,
    right: Handle<Image>,
}

/// The mark of a talent key under the window.
#[derive(Component)]
struct KeyMark(Mark);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mark {
    Left,
    Right,
    Attribute,
}

/// Dota's tree icon on the button, where the cache has it.
#[derive(Component)]
struct TalentArt {
    /// The icon as it is; lit while a talent could be taken; and aglow with Dota's Attribute
    /// Bonus glow while only a skill point could go into the bonus (Dota's `SkillUpgradable`
    /// without `CanLevelStats`).
    plain: Handle<Image>,
    lit: Handle<Image>,
    attributes: Handle<Image>,
}

/// The leaf of a taken talent on the button, by talent slot.
#[derive(Component)]
struct Leaf(u8);

/// The tiers of the tree, hidden for a hero without one.
#[derive(Component)]
struct TreeBody;

/// The Attribute Bonus row under the tiers: a click spends a skill point on it.
#[derive(Component)]
#[require(bevy::picking::hover::DirectlyHovered)]
pub struct AttributeRow;

/// The glow of the Attribute Bonus row while a point could go into it.
#[derive(Component)]
struct AttributeGlow;

/// The name of the Attribute Bonus row.
#[derive(Component)]
struct AttributeName;

/// One level dash of the Attribute Bonus, in the window's row.
#[derive(Component)]
struct AttributePip(u8);

/// One level dash of the Attribute Bonus, round the button.
#[derive(Component)]
struct ArcPip(u8);

/// What the window shows and does now.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Closed,
    /// Over the button: the tree to look at.
    Tooltip,
    /// Pinned: the window to choose in.
    Pinned,
}

/// The window's mode and what the tree of the hero the panel is about holds, worked out once a
/// frame for what draws it.
#[derive(Resource, Default, Debug, PartialEq)]
pub(crate) struct Tree {
    pub(crate) mode: Mode,
    /// The panel is about a hero at all.
    hero: bool,
    /// Whether the hero is one's own: only then is anything open to take.
    mine: bool,
    /// The hero has a tree of talents (every hero has the Attribute Bonus).
    has_tree: bool,
    learned: Talents,
    /// The talents that could be taken now: the server's word for one's own hero, the hero's level
    /// for another's.
    open: Talents,
    /// Levels of the Attribute Bonus learned.
    attribute: u8,
    /// Whether a skill point could go into the Attribute Bonus now.
    attribute_open: bool,
}

impl Tree {
    /// Whether a talent could be taken now.
    fn is_open(&self, slot: u8) -> bool {
        self.open.has(TalentSlot(slot))
    }

    fn is_learned(&self, slot: u8) -> bool {
        self.learned.has(TalentSlot(slot))
    }

    /// Whether any talent of a tier could be taken now.
    fn tier_open(&self, tier: u8) -> bool {
        self.is_open(2 * tier) || self.is_open(2 * tier + 1)
    }

    /// Whether any talent of a side could be taken now.
    fn side_open(&self, side: u8) -> bool {
        (0..4).any(|tier| self.is_open(2 * tier + side))
    }

    /// Whether a talent could be taken: what lights the button as Dota's `CanLevelStats` does. A
    /// skill point only the Attribute Bonus could take does not.
    fn talent_to_take(&self) -> bool {
        self.mine && self.open.bits != 0
    }

    fn pinned(&self) -> bool {
        self.mode == Mode::Pinned
    }
}

// Dota's own measures of the window, in its pixels (`ui_stat_branch.css`, `dota_hud_stat_branch.css`).
const WINDOW_WIDTH: f32 = 400.0;
/// The title's room: 6 over it, 24 for it, 4 under it.
const TITLE_ROOM: f32 = 34.0;
const ROW_HEIGHT: f32 = 45.0;
/// A tier's pitch: its row and the 6 under it.
const ROW_PITCH: f32 = 51.0;
const TIERS: u8 = 4;
const ROWS_HEIGHT: f32 = TITLE_ROOM + ROW_PITCH * TIERS as f32;
/// The backdrop's picture, and so the window's frame, reaches under the rows by its foot.
const BACKDROP_HEIGHT: f32 = 257.0;
/// The room under the rows for the key marks and the Attribute Bonus.
const LOWER_HEIGHT: f32 = 69.0;
const ATTRIBUTE_BOX: (f32, f32) = (209.0, 48.0);
const LEVEL_BADGE: f32 = 45.0;
/// Clears the HUD’s buff row and level-up plates.
const WINDOW_LIFT: f32 = 84.0;
/// The room of a choice's text: the half, less the padding and the badge's side.
const CHOICE_PADDING: (f32, f32) = (4.0, 20.0);
const MARK_SIZE: f32 = 17.0;

/// The button's size and the room its arc of dashes takes round it, in interface pixels.
const BUTTON: f32 = 60.0;
const ARC: f32 = 80.0;
/// The gold plate over the button, as the abilities' own are.
const PLATE: (f32, f32) = (BUTTON, 35.0);
const PLATE_GAP: f32 = 11.0;

const GOLD: Color = Color::srgb(0.95, 0.75, 0.3);
/// The button's rim while no talent could be taken.
const RIM: Color = Color::srgb(0.2, 0.22, 0.27);
const SHUT: Color = Color::srgb(0.38, 0.4, 0.45);

// Dota's colours of the window's words.
const WORDS: Color = Color::srgb(0.882, 0.882, 0.882);
const WORDS_OPEN: Color = Color::WHITE;
const WORDS_HOVER: Color = Color::srgb(0.906, 0.824, 0.569);
const WORDS_SHUT: Color = Color::srgb(0.404, 0.431, 0.439);
const WORDS_CHOSEN: Color = Color::srgba(0.906, 0.824, 0.569, 0.784);
const LEVEL_WORDS: Color = Color::srgb(0.906, 0.824, 0.569);
const MARK_FILL: Color = Color::srgb(0.129, 0.153, 0.149);
const PIP_SHUT: Color = Color::srgb(0.227, 0.247, 0.267);
const SIDE_SHADE: Color = Color::srgb(0.055, 0.063, 0.067);

pub fn plugin(app: &mut App) {
    app.init_resource::<Tree>().add_observer(click).add_systems(
        Update,
        (
            work_out,
            (
                show_button,
                show_window,
                (show_cells, show_levels, show_marks, show_attribute).run_if(talent_window_open),
                light,
                show_leaves,
                show_arc,
                show_plus,
            ),
        )
            .chain()
            .run_if(crate::select::match_or_tactical),
    );
}

fn talent_window_open(tree: Res<Tree>) -> bool {
    tree.mode != Mode::Closed
}

/// An image of the cache, or nothing where the cache has none.
fn picture(kit: &Spawner, logical: &str) -> Handle<Image> {
    kit.manifest
        .file(logical)
        .map_or_else(Handle::default, |path| kit.server.load(path))
}

/// The button, round, as the first of the ability row, and the tree over the row's `column`.
pub fn spawn_button(kit: &mut Spawner, row: Entity, bar: Entity) {
    let button = kit
        .commands
        .spawn((
            TalentButton,
            Name::new("talent button"),
            Button,
            Hovered::default(),
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
            BorderColor::all(RIM),
            BackgroundColor(ui::PANEL_DEEP),
            ChildOf(row),
        ))
        .id();
    let art = (kit.manifest.file("hud.talents"))
        .zip(kit.manifest.file("hud.talents_learnable"))
        .zip(kit.manifest.file("hud.attributes_learnable"));
    if let Some(((plain, lit), attributes)) = art {
        let plain = kit.server.load(plain);
        kit.commands.spawn((
            TalentArt {
                plain: plain.clone(),
                lit: kit.server.load(lit),
                attributes: kit.server.load(attributes),
            },
            ImageNode::new(plain),
            Node {
                border_radius: BorderRadius::MAX,
                ..ui::fill()
            },
            Pickable::IGNORE,
            ChildOf(button),
        ));
        spawn_leaves(kit, button);
    } else {
        spawn_drawn_tree(kit.commands, button);
    }
    spawn_arc(kit.commands, button);
    spawn_plus(kit, row);
    spawn_panel(kit, bar);
}

/// The leaf of each talent's tier over the icon, shown once the talent is taken: the right
/// choice's leaf as the cache has it, the left's mirrored.
fn spawn_leaves(kit: &mut Spawner, button: Entity) {
    for slot in 0..TalentSlot::COUNT {
        let tier = bota_core::rules::TALENT_LEVELS[usize::from(slot / 2)];
        let mut image = ImageNode::new(picture(kit, &format!("hud.tree_leaf_{tier}")));
        image.flip_x = slot % 2 == 0;
        kit.commands.spawn((
            Leaf(slot),
            image,
            Node {
                border_radius: BorderRadius::MAX,
                ..ui::fill()
            },
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(button),
        ));
    }
}

/// The seven dashes of the Attribute Bonus in an arc under the button, where Dota draws them: in
/// the places and at the slants of its own picture of the arc, whose side is [`ARC`] here.
fn spawn_arc(commands: &mut Commands, button: Entity) {
    // Centre and slant of each dash in the picture, which is 176 across.
    const DASHES: [(f32, f32, f32); 7] = [
        (22.8, 134.5, 55.8),
        (40.5, 152.2, 34.9),
        (62.8, 163.5, 18.3),
        (87.5, 167.5, 0.0),
        (112.2, 163.5, -17.4),
        (134.5, 152.2, -34.2),
        (152.2, 134.5, -55.1),
    ];
    let scale = ARC / 176.0;
    let (length, thickness) = (16.0 * scale, 7.0 * scale);
    let inner = BUTTON - 4.0;
    for (level, (x, y, slant)) in DASHES.into_iter().enumerate() {
        commands.spawn((
            ArcPip(level as u8),
            // The Attribute Bonus arc sits outside the talent button's rim.
            ui::Overhangs,
            Node {
                position_type: PositionType::Absolute,
                left: px((inner - ARC) / 2.0 + x * scale - length / 2.0),
                top: px((inner - ARC) / 2.0 + y * scale - thickness / 2.0),
                width: px(length),
                height: px(thickness),
                border_radius: BorderRadius::all(px(1)),
                ..default()
            },
            UiTransform::from_rotation(Rot2::degrees(slant)),
            BackgroundColor(PIP_SHUT),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(button),
        ));
    }
}

/// Dota's gold plate with a plus over the button, while a point could go into the tree: as high
/// over the row as the abilities' own plates, and not a child of the button, so that a press on it
/// does not reach the button as well.
fn spawn_plus(kit: &mut Spawner, row: Entity) {
    let plate = kit
        .commands
        .spawn((
            TalentPlus,
            Name::new("talent plus"),
            Button,
            Hovered::default(),
            crate::hud::Blocker,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: px(-1),
                width: px(PLATE.0),
                height: px(PLATE.1),
                margin: UiRect::bottom(px(PLATE_GAP)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(2)),
                ..default()
            },
            ui::art(kit.manifest, kit.server, "hud.levelup_button"),
            Visibility::Hidden,
            ChildOf(row),
        ))
        .id();
    match kit.manifest.file("hud.levelup_well") {
        Some(path) => {
            kit.commands.spawn((
                Node {
                    width: px(PLATE.1 - 6.0),
                    height: px(PLATE.1 - 6.0),
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

/// A small tree drawn in the button where the cache has no icon: a trunk and a pip for each
/// talent, lit as they are learned.
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

/// The window over the ability row: the title, the tiers, then the key marks and the Attribute
/// Bonus.
fn spawn_panel(kit: &mut Spawner, bar: Entity) {
    let panel = kit
        .commands
        .spawn((
            TalentPanel,
            Name::new("talent window"),
            Hovered::default(),
            crate::hud::Blocker,
            Node {
                position_type: PositionType::Absolute,
                bottom: percent(100),
                left: px(0),
                margin: UiRect::bottom(dota(WINDOW_LIFT)),
                width: dota(WINDOW_WIDTH),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            GlobalZIndex(20),
            Visibility::Hidden,
            ChildOf(bar),
        ))
        .id();
    kit.commands.spawn((
        Backdrop {
            pinned: picture(kit, "hud.statbranch_backdrop"),
            tooltip: picture(kit, "hud.statbranch_backdrop_tooltip"),
        },
        ImageNode::new(picture(kit, "hud.statbranch_backdrop")),
        Node {
            position_type: PositionType::Absolute,
            width: dota(WINDOW_WIDTH),
            height: dota(BACKDROP_HEIGHT),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(panel),
    ));
    let body = kit
        .commands
        .spawn((
            TreeBody,
            Node {
                width: percent(100),
                height: dota(ROWS_HEIGHT),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    spawn_title(kit, body);
    for tier in (0..TIERS).rev() {
        spawn_tier(kit, body, tier);
    }
    spawn_lower(kit, panel);
}

fn spawn_title(kit: &mut Spawner, body: Entity) {
    kit.commands.spawn((
        Title,
        ui::text("", kit.fonts.face(Face::Regular), 18.0 * ui::DOTA, WORDS),
        LetterSpacing::Px(2.0 * ui::DOTA),
        ui::shadow(),
        Node {
            margin: UiRect::new(px(0), px(0), dota(6.0), dota(4.0)),
            height: dota(24.0),
            align_self: AlignSelf::Center,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(body),
    ));
}

/// One tier of the tree: its glow, its two choices either side of its level.
fn spawn_tier(kit: &mut Spawner, body: Entity, tier: u8) {
    let row = kit
        .commands
        .spawn((
            Node {
                width: percent(100),
                height: dota(ROW_PITCH),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(body),
        ))
        .id();
    kit.commands.spawn((
        Glow {
            tier,
            both: picture(kit, "hud.statbranch_glow"),
            left: picture(kit, "hud.statbranch_glow_left"),
            right: picture(kit, "hud.statbranch_glow_right"),
        },
        ImageNode::new(picture(kit, "hud.statbranch_glow")).with_color(Color::NONE),
        Node {
            position_type: PositionType::Absolute,
            left: percent(1),
            width: percent(98),
            top: dota(3.0),
            height: dota(ROW_HEIGHT),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(row),
    ));
    for side in 0..2u8 {
        spawn_choice(kit, row, 2 * tier + side);
    }
    let badge = kit
        .commands
        .spawn((
            TierLevel(tier),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                margin: UiRect::left(dota(-LEVEL_BADGE / 2.0)),
                width: dota(LEVEL_BADGE),
                height: dota(LEVEL_BADGE),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ui::art(kit.manifest, kit.server, "hud.statbranch_level"),
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    let font = kit.manifest.file("font.numbers").map_or_else(
        || kit.fonts.face(Face::Regular).clone(),
        |path| kit.server.load(path),
    );
    kit.commands.spawn((
        ui::text(
            talent_levels(TalentSlot(2 * tier)).0.to_string(),
            &font,
            20.0 * ui::DOTA,
            LEVEL_WORDS,
        ),
        LetterSpacing::Px(2.0 * ui::DOTA),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(badge),
    ));
}

/// One choice of a tier: the half of the row it fills, shaded towards the badge, and its words.
fn spawn_choice(kit: &mut Spawner, row: Entity, slot: u8) {
    let left = slot.is_multiple_of(2);
    let (outer, inner) = CHOICE_PADDING;
    let shade = [SIDE_SHADE, SIDE_SHADE.with_alpha(0.0)];
    let stops = if left { shade } else { [shade[1], shade[0]] };
    let side = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: if left { px(0) } else { percent(50) },
                width: percent(50),
                height: dota(ROW_HEIGHT),
                padding: if left {
                    UiRect::new(dota(outer), dota(inner), px(0), px(0))
                } else {
                    UiRect::new(dota(inner), dota(outer), px(0), px(0))
                },
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_right(vec![
                ColorStop::new(stops[0], percent(0)),
                ColorStop::new(stops[1], percent(100)),
            ])),
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    let cell = kit
        .commands
        .spawn((
            TalentCell(slot),
            Name::new(format!("talent cell {slot}")),
            Button,
            Hovered::default(),
            Node {
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            ChildOf(side),
        ))
        .id();
    kit.commands.spawn((
        CellText(slot),
        ui::text(
            "",
            kit.fonts.face(Face::Regular),
            15.0 * ui::DOTA,
            WORDS_SHUT,
        ),
        TextLayout::justify(Justify::Center),
        bevy::text::LineHeight::RelativeToFont(1.0),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(cell),
    ));
}

/// Under the tiers: the marks of the keys that learn a talent, and the Attribute Bonus with a
/// dash for each of its levels.
fn spawn_lower(kit: &mut Spawner, panel: Entity) {
    let lower = kit
        .commands
        .spawn((
            Node {
                width: percent(100),
                height: dota(LOWER_HEIGHT),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(panel),
        ))
        .id();
    spawn_mark(kit, lower, Mark::Left);
    spawn_mark(kit, lower, Mark::Right);
    spawn_attribute_row(kit, lower);
    spawn_mark(kit, lower, Mark::Attribute);
}

fn spawn_mark(kit: &mut Spawner, lower: Entity, mark: Mark) {
    let (action, node) = match mark {
        Mark::Left => (
            Action::TalentLeft,
            Node {
                left: px(0),
                border_radius: BorderRadius::bottom(dota(4.0)),
                ..default()
            },
        ),
        Mark::Right => (
            Action::TalentRight,
            Node {
                right: px(0),
                border_radius: BorderRadius::bottom(dota(4.0)),
                ..default()
            },
        ),
        Mark::Attribute => (
            Action::TalentAttribute,
            Node {
                left: percent(50),
                top: px(0),
                margin: UiRect::left(dota(-MARK_SIZE / 2.0)),
                border_radius: BorderRadius::all(dota(4.0)),
                ..default()
            },
        ),
    };
    let plate = kit
        .commands
        .spawn((
            KeyMark(mark),
            Node {
                position_type: PositionType::Absolute,
                min_width: dota(MARK_SIZE),
                min_height: dota(MARK_SIZE),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(px(1)),
                ..node
            },
            BackgroundColor(MARK_FILL),
            BorderColor::all(Color::BLACK),
            Visibility::Hidden,
            Pickable::IGNORE,
            ChildOf(lower),
        ))
        .id();
    kit.commands.spawn((
        KeyLabel(action),
        ui::text(
            "",
            kit.fonts.face(Face::Regular),
            14.0 * ui::DOTA,
            Color::WHITE,
        ),
        ui::shadow(),
        Pickable::IGNORE,
        ChildOf(plate),
    ));
}

/// The Attribute Bonus under the tiers, as Dota's tree has it: its name and a dash for each level
/// it may take.
fn spawn_attribute_row(kit: &mut Spawner, lower: Entity) {
    let name = kit
        .texts
        .label("DOTA_LevelUpAttributes", &[])
        .unwrap_or_else(|| {
            format!(
                "+{} to all attributes",
                bota_core::rules::ATTRIBUTE_BONUS_POINTS
            )
        });
    let row = kit
        .commands
        .spawn((
            AttributeRow,
            Name::new("talent attribute row"),
            Button,
            Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                left: percent(50),
                bottom: px(0),
                margin: UiRect::left(dota(-ATTRIBUTE_BOX.0 / 2.0)),
                width: dota(ATTRIBUTE_BOX.0),
                height: dota(ATTRIBUTE_BOX.1),
                ..default()
            },
            ui::art(kit.manifest, kit.server, "hud.statbranch_attribute"),
            ChildOf(lower),
        ))
        .id();
    kit.commands.spawn((
        AttributeGlow,
        ImageNode::new(picture(kit, "hud.statbranch_attribute_glow")).with_color(Color::NONE),
        ui::fill(),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    kit.commands.spawn((
        AttributeName,
        ui::text(name, kit.fonts.face(Face::Regular), 15.0 * ui::DOTA, WORDS),
        ui::shadow(),
        Node {
            position_type: PositionType::Absolute,
            top: dota(10.0),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        },
        TextLayout::justify(Justify::Center),
        Pickable::IGNORE,
        ChildOf(row),
    ));
    let pips = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: dota(6.0),
                width: percent(100),
                justify_content: JustifyContent::Center,
                column_gap: dota(4.4),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(row),
        ))
        .id();
    for level in 0..bota_core::rules::ATTRIBUTE_BONUS_MAX_LEVEL {
        kit.commands.spawn((
            AttributePip(level),
            Node {
                width: dota(7.5),
                height: dota(3.0),
                ..default()
            },
            BackgroundColor(PIP_SHUT),
            Pickable::IGNORE,
            ChildOf(pips),
        ));
    }
}

/// Works out the window's mode and what the tree of the panel's hero holds: a click on the button
/// or the plus, or Dota's talent key, pins the window open or lets it go (`--tip talents` keeps it
/// open, for screenshots); the cursor over the button shows the tree as a tooltip.
fn work_out(
    session: NonSend<Session>,
    args: Res<crate::Args>,
    panels: Res<crate::input::Panels>,
    mut tree: ResMut<Tree>,
    mut keys: ResMut<crate::keys::TalentKeys>,
    button: Query<&Hovered, With<TalentButton>>,
) {
    let core = &session.0;
    let hero = crate::hud::subject_hero(core);
    let (seat, unit, _) = crate::hud::subject(core);
    let mine = hero.is_some() && seat.map(|p| p.slot) == core.my_slot && core.my_slot.is_some();
    let told = seat.and_then(|p| core.talents_of(p.slot));
    let level = unit.map(|u| u.level).or(seat.map(|p| p.level)).unwrap_or(0);
    let learned = told.map_or_else(Talents::default, |t| t.learned);
    let forced = args.tip.as_deref() == Some("talents");
    // Every hero has the Attribute Bonus to learn, whether or not it has talents.
    let mode = if hero.is_none() {
        Mode::Closed
    } else if forced || panels.talents {
        Mode::Pinned
    } else if button.iter().any(|hovered| hovered.get()) {
        Mode::Tooltip
    } else {
        Mode::Closed
    };
    let has_tree = mode != Mode::Closed && hero.is_some_and(|h| bota_client_core::has_talents(h.0));
    let now = Tree {
        mode,
        hero: hero.is_some(),
        mine,
        has_tree,
        learned,
        open: open_talents(told.as_ref(), mine, has_tree, level, learned),
        attribute: told.map_or(0, |t| t.attribute_bonus),
        attribute_open: mine && told.is_some_and(|t| t.may_learn_attribute_bonus == Some(true)),
    };
    if *tree != now {
        *tree = now;
    }
    let on = mode == Mode::Pinned && mine;
    if keys.0 != on {
        keys.0 = on;
    }
}

/// The talents that could be taken now: one's own hero's, as its view tells it; another's, as
/// its level opens them.
fn open_talents(
    told: Option<&TalentView>,
    mine: bool,
    has_tree: bool,
    level: u8,
    learned: Talents,
) -> Talents {
    if mine {
        return told.and_then(|t| t.may_learn).unwrap_or_default();
    }
    if !has_tree {
        return Talents::default();
    }
    (0..TalentSlot::COUNT)
        .map(TalentSlot)
        .filter(|slot| !learned.has(*slot) && level >= bota_core::talent_floor(*slot, learned))
        .fold(Talents::default(), Talents::with)
}

/// The breathing of what a point could go into, as Dota's `TalentSelect` animation: opacity from
/// one down to half and back every 2.02 seconds.
fn pulse(time: &Time) -> f32 {
    let phase = (time.elapsed_secs() % 2.02) / 2.02;
    1.0 - 0.5 * (1.0 - (2.0 * phase - 1.0).abs())
}

/// The button, for any hero the panel is about, its rim aglow while one's own hero could take a
/// talent.
fn show_button(
    tree: Res<Tree>,
    time: Res<Time>,
    mut button: Query<(&mut Visibility, &mut BorderColor), With<TalentButton>>,
) {
    let glow = 0.55 + 0.45 * (time.elapsed_secs() * 4.0).sin().abs();
    for (mut v, mut rim) in &mut button {
        v.set_if_neq(ui::visible(tree.hero));
        rim.set_if_neq(BorderColor::all(if tree.talent_to_take() {
            GOLD.with_alpha(glow)
        } else {
            RIM
        }));
    }
}

/// The window, its title and backdrop, and the tiers of a hero that has a tree.
fn show_window(
    tree: Res<Tree>,
    texts: Res<Texts>,
    mut panel: Query<&mut Visibility, With<TalentPanel>>,
    mut body: Query<&mut Visibility, (With<TreeBody>, Without<TalentPanel>)>,
    mut backdrop: Query<(&Backdrop, &mut ImageNode)>,
    mut title: Query<&mut Text, With<Title>>,
    mut previous_mode: Local<Mode>,
) {
    let open = tree.mode != Mode::Closed;
    let changed_mode = *previous_mode != tree.mode;
    *previous_mode = tree.mode;
    if !open && !changed_mode {
        return;
    }
    for mut v in &mut panel {
        v.set_if_neq(ui::visible(open));
    }
    if !open {
        return;
    }
    for mut v in &mut body {
        v.set_if_neq(ui::visible(tree.has_tree));
    }
    for (art, mut image) in &mut backdrop {
        let want = if tree.pinned() {
            &art.pinned
        } else {
            &art.tooltip
        };
        if image.image != *want && *want != Handle::default() {
            image.image = want.clone();
        }
    }
    if !changed_mode && !texts.is_changed() {
        return;
    }
    let key = if tree.pinned() {
        "DOTA_StatBranch_Title"
    } else {
        "DOTA_StatBranch_TooltipTitle"
    };
    let words = texts
        .label(key, &[])
        .unwrap_or_else(|| {
            (if tree.pinned() {
                "Select a talent"
            } else {
                "Talent Tree"
            })
            .to_string()
        })
        .to_uppercase();
    for mut text in &mut title {
        if text.0 != words {
            text.0.clone_from(&words);
        }
    }
}

/// What the choices of the tree say, and in what colour: taken in gold, open in white (gold under
/// the cursor), the rest dimmed, the other talent of a taken tier among them.
fn show_cells(
    tree: Res<Tree>,
    session: NonSend<Session>,
    texts: Res<Texts>,
    mut faces: Local<Option<(u16, [TalentFace; TalentSlot::COUNT as usize])>>,
    cells: Query<(&TalentCell, &Hovered)>,
    mut words: Query<(&CellText, &mut Text, &mut TextColor)>,
    mut hovered: Local<[bool; 8]>,
) {
    let Some(hero) = crate::hud::subject_hero(&session.0) else {
        return;
    };
    if texts.is_changed() || faces.as_ref().is_none_or(|(held, _)| *held != hero.0) {
        *faces = talents(hero.0, &texts).map(|tree| (hero.0, tree));
    }
    let Some((_, names)) = faces.as_ref() else {
        return;
    };
    for (cell, over) in &cells {
        hovered[usize::from(cell.0)] = over.get();
    }
    for (cell, mut text, mut color) in &mut words {
        let slot = usize::from(cell.0);
        let name = &names[slot].name;
        if text.0 != *name {
            text.0.clone_from(name);
        }
        let ink = if tree.is_learned(cell.0) {
            WORDS_CHOSEN
        } else if tree.pinned() && tree.is_open(cell.0) {
            if hovered[slot] {
                WORDS_HOVER
            } else {
                WORDS_OPEN
            }
        } else if tree.is_open(cell.0) {
            WORDS_OPEN
        } else {
            WORDS_SHUT
        };
        color.set_if_neq(TextColor(ink));
    }
}

/// Each tier's glow while one has a choice open, and its level badge in white then.
fn show_levels(
    tree: Res<Tree>,
    time: Res<Time>,
    mut glows: Query<(&Glow, &mut ImageNode)>,
    badges: Query<(&TierLevel, &Children)>,
    mut words: Query<&mut TextColor>,
) {
    let pulse = pulse(&time);
    for (glow, mut image) in &mut glows {
        let (left, right) = (tree.is_open(2 * glow.tier), tree.is_open(2 * glow.tier + 1));
        let shown = tree.pinned() && tree.mine;
        let art = match (left, right) {
            (true, true) => &glow.both,
            (true, false) => &glow.left,
            _ => &glow.right,
        };
        if image.image != *art {
            image.image = art.clone();
        }
        let tint = if shown && (left || right) {
            Color::WHITE.with_alpha(pulse)
        } else {
            Color::NONE
        };
        if image.color != tint {
            image.color = tint;
        }
    }
    for (badge, children) in &badges {
        let ink = if tree.pinned() && tree.tier_open(badge.0) {
            WORDS_OPEN
        } else {
            LEVEL_WORDS
        };
        for child in children {
            if let Ok(mut color) = words.get_mut(*child) {
                color.set_if_neq(TextColor(ink));
            }
        }
    }
}

/// The marks of the keys, under the window while a choice of their side, or the Attribute Bonus,
/// is open to the key.
fn show_marks(tree: Res<Tree>, mut marks: Query<(&KeyMark, &mut Visibility)>) {
    for (mark, mut v) in &mut marks {
        let on = tree.pinned()
            && tree.mine
            && match mark.0 {
                Mark::Left => tree.side_open(0),
                Mark::Right => tree.side_open(1),
                Mark::Attribute => tree.attribute_open,
            };
        v.set_if_neq(ui::visible(on));
    }
}

/// The Attribute Bonus row's dashes lit to its level, its glow while a point could go into it,
/// its words gold once it has every level.
fn show_attribute(
    tree: Res<Tree>,
    time: Res<Time>,
    row: Query<&Hovered, With<AttributeRow>>,
    mut glow: Query<&mut ImageNode, With<AttributeGlow>>,
    mut name: Query<&mut TextColor, With<AttributeName>>,
    mut pips: Query<(&AttributePip, &mut BackgroundColor)>,
) {
    let learnable = tree.pinned() && tree.attribute_open;
    let over = row.iter().any(|hovered| hovered.get());
    // `BonusAttributeSelect`: from half to a little over full and back.
    let phase = (time.elapsed_secs() % 2.02) / 2.02;
    let breath = 0.5 + 0.65 * (1.0 - (2.0 * phase - 1.0).abs());
    for mut image in &mut glow {
        let tint = if learnable {
            Color::WHITE.with_alpha(breath.min(1.0))
        } else {
            Color::NONE
        };
        if image.color != tint {
            image.color = tint;
        }
    }
    let complete = tree.attribute >= bota_core::rules::ATTRIBUTE_BONUS_MAX_LEVEL;
    for mut color in &mut name {
        color.set_if_neq(TextColor(if complete {
            WORDS_CHOSEN
        } else if learnable && over {
            WORDS_HOVER
        } else {
            WORDS
        }));
    }
    for (pip, mut bg) in &mut pips {
        bg.set_if_neq(BackgroundColor(if pip.0 < tree.attribute {
            LEVEL_WORDS
        } else {
            PIP_SHUT
        }));
    }
}

/// Dota's tree icon of one's own hero: lit while a talent could be taken, aglow with the
/// Attribute Bonus's glow while only the bonus could take a skill point, plain otherwise.
fn light(tree: Res<Tree>, mut art: Query<(&TalentArt, &mut ImageNode)>) {
    let attribute = tree.mine && tree.attribute_open;
    for (art, mut image) in &mut art {
        let want = if tree.talent_to_take() {
            &art.lit
        } else if attribute {
            &art.attributes
        } else {
            &art.plain
        };
        if image.image != *want {
            image.image = want.clone();
        }
    }
}

/// The leaf of each talent taken, over the icon; the pips of the drawn tree where the cache has no
/// icon.
fn show_leaves(
    tree: Res<Tree>,
    time: Res<Time>,
    mut leaves: Query<(&Leaf, &mut Visibility)>,
    mut pips: Query<(&Pip, &mut BackgroundColor), Without<ArcPip>>,
) {
    for (leaf, mut v) in &mut leaves {
        v.set_if_neq(ui::visible(tree.is_learned(leaf.0)));
    }
    let glow = 0.55 + 0.45 * (time.elapsed_secs() * 4.0).sin().abs();
    for (pip, mut bg) in &mut pips {
        bg.set_if_neq(BackgroundColor(if tree.is_learned(pip.0) {
            GOLD
        } else if tree.pinned() && tree.is_open(pip.0) || tree.is_open(pip.0) && tree.mine {
            GOLD.with_alpha(0.35 + 0.4 * glow)
        } else {
            SHUT
        }));
    }
}

/// The Attribute Bonus's dashes under the button, lit to its level.
fn show_arc(tree: Res<Tree>, mut arc: Query<(&ArcPip, &mut BackgroundColor, &mut Visibility)>) {
    for (pip, mut bg, mut v) in &mut arc {
        v.set_if_neq(ui::visible(tree.hero));
        bg.set_if_neq(BackgroundColor(if pip.0 < tree.attribute {
            LEVEL_WORDS
        } else {
            PIP_SHUT
        }));
    }
}

/// Dota's plus plate over the button while a point could go into the tree.
fn show_plus(tree: Res<Tree>, mut plate: Query<&mut Visibility, With<TalentPlus>>) {
    let on = tree.mine && (tree.talent_to_take() || tree.attribute_open);
    for mut v in &mut plate {
        v.set_if_neq(ui::visible(on));
    }
}

#[allow(clippy::too_many_arguments)]
fn click(
    event: On<PointerPress>,
    known: Res<crate::session::MatchKnown>,
    tactical: Res<crate::select::TacticalPhase>,
    tree: Res<Tree>,
    mut session: NonSendMut<Session>,
    mut panels: ResMut<crate::input::Panels>,
    button: Query<(), Or<(With<TalentButton>, With<TalentPlus>)>>,
    cells: Query<&TalentCell>,
    attribute: Query<(), With<AttributeRow>>,
) {
    if event.button != PointerButton::Primary || !crate::select::match_or_tactical(known, tactical)
    {
        return;
    }
    if button.contains(event.entity) {
        panels.talents = !panels.talents;
        return;
    }
    // The tooltip is for looking at; only the pinned window chooses.
    if !tree.pinned() || !tree.mine {
        return;
    }
    if let Ok(cell) = cells.get(event.entity) {
        session.0.learn_talent(cell.0);
    }
    if attribute.contains(event.entity) {
        session.0.learn_attribute_bonus();
    }
}

/// What Dota's talent keys do: the left or the right talent of the lowest tier with one open, or a
/// skill point on the Attribute Bonus.
pub(crate) fn learn_by_key(core: &mut bota_client_core::App, fired: &crate::keys::Fired) {
    if fired.pressed(Action::TalentLeft) {
        core.learn_talent_of_side(TalentSide::Left);
    }
    if fired.pressed(Action::TalentRight) {
        core.learn_talent_of_side(TalentSide::Right);
    }
    if fired.pressed(Action::TalentAttribute) {
        core.learn_attribute_bonus();
    }
}

/// What the tree button shows, as the end-to-end tests read it.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeShown {
    Plain,
    /// Lit: a talent could be taken.
    Talent,
    /// Aglow with the Attribute Bonus's glow.
    Attributes,
}

/// The tree icon the button draws and whether its rim glows.
#[cfg(test)]
pub(crate) fn tree_shown(world: &mut World) -> (Option<TreeShown>, bool) {
    let shown = world
        .query::<(&TalentArt, &ImageNode)>()
        .iter(world)
        .next()
        .map(|(art, image)| {
            if image.image == art.lit {
                TreeShown::Talent
            } else if image.image == art.attributes {
                TreeShown::Attributes
            } else {
                TreeShown::Plain
            }
        });
    let aglow = world
        .query_filtered::<&BorderColor, With<TalentButton>>()
        .iter(world)
        .any(|rim| rim.top != RIM);
    (shown, aglow)
}

/// What a choice's words look like, as the end-to-end tests read them.
#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Look {
    /// Taken: gold.
    Chosen,
    /// Open to take: white, gold under the cursor.
    Open,
    /// Not open: dimmed.
    Shut,
}

/// The window as drawn, for the end-to-end tests: its mode and title, the tiers glowing, the
/// marks of the keys showing, and each choice's look.
#[cfg(test)]
#[derive(Debug, PartialEq)]
pub(crate) struct Shown {
    pub(crate) mode: Mode,
    pub(crate) title: String,
    pub(crate) glowing: Vec<u8>,
    /// The marks of the left key, the right key and the Attribute Bonus's.
    pub(crate) marks: [bool; 3],
    pub(crate) looks: [Look; 8],
    pub(crate) texts: [String; 8],
    pub(crate) choice_font: bevy::text::FontSource,
    /// The plus plate over the button shows.
    pub(crate) plus: bool,
}

#[cfg(test)]
pub(crate) fn shown(world: &mut World) -> Shown {
    let mode = world.resource::<Tree>().mode;
    let title = world
        .query_filtered::<&Text, With<Title>>()
        .single(world)
        .map(|text| text.0.clone())
        .unwrap_or_default();
    let mut glowing: Vec<u8> = world
        .query::<(&Glow, &ImageNode)>()
        .iter(world)
        .filter(|(_, image)| image.color.alpha() > 0.0)
        .map(|(glow, _)| glow.tier)
        .collect();
    glowing.sort_unstable();
    let mut marks = [false; 3];
    for (mark, seen) in world
        .query::<(&KeyMark, &InheritedVisibility)>()
        .iter(world)
    {
        marks[match mark.0 {
            Mark::Left => 0,
            Mark::Right => 1,
            Mark::Attribute => 2,
        }] = seen.get();
    }
    let mut looks = [Look::Shut; 8];
    let mut texts: [String; 8] = Default::default();
    let mut choice_font = bevy::text::FontSource::default();
    for (cell, text, color, font) in world
        .query::<(&CellText, &Text, &TextColor, &TextFont)>()
        .iter(world)
    {
        let slot = usize::from(cell.0);
        if slot == 0 {
            choice_font = font.font.clone();
        }
        texts[slot].clone_from(&text.0);
        looks[slot] = if color.0 == WORDS_CHOSEN {
            Look::Chosen
        } else if color.0 == WORDS_OPEN || color.0 == WORDS_HOVER {
            Look::Open
        } else {
            Look::Shut
        };
    }
    let plus = world
        .query_filtered::<&InheritedVisibility, With<TalentPlus>>()
        .single(world)
        .is_ok_and(|seen| seen.get());
    Shown {
        mode,
        title,
        glowing,
        marks,
        looks,
        texts,
        choice_font,
        plus,
    }
}

/// The choices whose words overrun their box: the words, the box and the words' size, in physical
/// pixels.
#[cfg(test)]
pub(crate) fn overflowing(world: &mut World) -> Vec<(String, Vec2, Vec2)> {
    let mut out = Vec::new();
    let texts: Vec<(Entity, String, Entity)> = world
        .query::<(Entity, &CellText, &Text, &ChildOf)>()
        .iter(world)
        .map(|(entity, _, text, parent)| (entity, text.0.clone(), parent.parent()))
        .collect();
    for (entity, words, parent) in texts {
        let (Some(text), Some(cell)) = (
            world.get::<ComputedNode>(entity),
            world.get::<ComputedNode>(parent),
        ) else {
            continue;
        };
        let (own, room) = (text.size(), cell.size());
        if own.x > room.x + 0.5 || own.y > room.y + 0.5 {
            out.push((words, room, own));
        }
    }
    out
}
