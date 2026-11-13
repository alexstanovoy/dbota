//! The hover popup, laid out as Dota lays out its own: the name over the
//! price, the stats an item adds, each section under its bar, the cooldown
//! and mana, the lore at the foot.

use bevy::ecs::schedule::common_conditions::not;
use bevy::picking::hover::{DirectlyHovered, Hovered};
use bevy::{prelude::*, window::PrimaryWindow};
use bota_core::item_def;
use bota_proto::TalentSlot;

use crate::dota_text::Texts;
use crate::hud::{AbilityBox, ItemBox};
use crate::manifest::Manifest;
use crate::session::Session;
use crate::shop::ShopCell;
use crate::theme::Face;
use crate::tip_content::{Content, Cost, Kind, Lookup, Run, Section, Tone};
use crate::ui::help::{Side, placement::position};
use crate::ui::{self, Fonts};

/// The popup box.
#[derive(Component)]
struct Tip;

/// An ability's icon on the hero page of the menu: under the cursor, the popup the game shows
/// for the ability (its first level, as the strategy phase shows it).
#[derive(Component)]
#[require(Hovered, DirectlyHovered)]
pub struct AbilityTile(pub u16);

/// The talent tree's mark on a hero's row of skills (the pick panel, the strategy screen): under
/// the cursor, the popup that lists the hero's talents by the level that opens each.
#[derive(Component)]
#[require(Hovered, DirectlyHovered)]
pub struct TalentTile(pub u16);

/// What the popup holds below its edge; rebuilt when the content changes.
#[derive(Component)]
struct TipBody;

/// What the popup shows now, if anything.
#[derive(Resource, Default)]
struct Shown(Option<Content>);

/// Where the thing the popup is about stands, in the window's pixels, when the popup is to stand
/// beside it rather than by the cursor: the menu's ability tiles name theirs.
#[derive(Resource, Default)]
struct ShownAnchor(Option<Rect>);

/// Width of the popup in interface pixels: Dota's 330 of text blocks (`#AbilityDescriptionContainer`) and a 2 pixel border either side, so its blocks and bars span it whole.
const WIDTH: f32 = 334.0;
/// Where the popup stands in the interface's layers: over the menu's page and its overlay.
pub const TIP_Z: i32 = 72;
/// Space between the cursor and the popup.
const GAP: f32 = 16.0;

const NAME: Color = Color::srgb(0.88, 0.88, 0.88);
const BONUS_LABEL: Color = Color::srgb(0.455, 0.502, 0.565);
const STAT_LABEL: Color = Color::srgb(0.349, 0.431, 0.537);
const NOTE: Color = Color::srgb(0.5, 0.5, 0.5);
const LORE: Color = Color::srgb(0.24, 0.306, 0.357);
const LORE_BG: Color = Color::srgb(0.043, 0.071, 0.082);
const FAINT: Color = Color::srgb(0.302, 0.333, 0.376);

/// How the runs of a block are coloured.
#[derive(Clone, Copy)]
struct Palette {
    body: Color,
    value: Color,
    dim: Color,
    label: Color,
    header: Color,
    ground: Option<Color>,
}

const ABILITY: Palette = Palette {
    body: Color::srgb(0.604, 0.69, 0.804),
    value: NAME,
    dim: FAINT,
    label: STAT_LABEL,
    header: Color::srgb(0.569, 0.627, 0.706),
    ground: None,
};

const LINES: Palette = Palette {
    label: BONUS_LABEL,
    ..ABILITY
};

fn section_palette(kind: Kind) -> Palette {
    let hex = |h: u32| Color::srgb_u8((h >> 16) as u8, (h >> 8) as u8, h as u8);
    match kind {
        Kind::Passive => Palette {
            body: hex(0x626d7b),
            value: hex(0x7e8c9d),
            dim: hex(0x474a61),
            label: hex(0x626d7b),
            header: hex(0x91a0b4),
            ground: Some(hex(0x1c262f)),
        },
        Kind::Active => Palette {
            body: hex(0x7a80a7),
            value: hex(0x9f9fcf),
            dim: hex(0x474a61),
            label: hex(0x7a80a7),
            header: hex(0xb0b0ff),
            ground: Some(hex(0x1a2234)),
        },
        Kind::Use => Palette {
            body: hex(0x7b8a72),
            value: hex(0x95c07a),
            dim: hex(0x474a61),
            label: hex(0x7b8a72),
            header: hex(0x95c07a),
            ground: Some(hex(0x182623)),
        },
    }
}

fn tone_color(tone: Tone, palette: &Palette) -> Color {
    match tone {
        Tone::Body => palette.body,
        Tone::Value => palette.value,
        Tone::Dim => palette.dim,
        Tone::Label => palette.label,
        Tone::Strength => Color::srgb_u8(0xfb, 0x57, 0x3c),
        Tone::Agility => Color::srgb_u8(0x1b, 0xfc, 0x20),
        Tone::Intelligence => Color::srgb_u8(0x2e, 0xee, 0xff),
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Shown>()
        .init_resource::<ShownAnchor>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (
                pick.run_if(crate::select::match_or_tactical),
                pick_pregame.run_if(not(crate::select::match_or_tactical)),
                rebuild,
                place,
            )
                .chain(),
        );
}

fn spawn(mut commands: Commands) {
    let tip = commands
        .spawn((
            Tip,
            Node {
                position_type: PositionType::Absolute,
                width: px(WIDTH),
                border: UiRect::all(px(2)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgb(0.07, 0.09, 0.11)),
            BorderColor::all(Color::BLACK),
            // Over the menu's page (60) and its overlay (70).
            GlobalZIndex(TIP_Z),
            Pickable::IGNORE,
            Visibility::Hidden,
        ))
        .id();
    commands.spawn((
        TipBody,
        Node {
            flex_direction: FlexDirection::Column,
            width: percent(100),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(tip),
    ));
}

/// What the cursor is over, as a popup.
fn pick(
    session: NonSend<Session>,
    texts: Res<Texts>,
    manifest: Res<Manifest>,
    args: Res<crate::Args>,
    abilities: Query<(&DirectlyHovered, &AbilityBox)>,
    items: Query<(&DirectlyHovered, &ItemBox)>,
    bag: Res<crate::bag::TacticalBag>,
    tiles: Query<(&DirectlyHovered, &AbilityTile)>,
    talent_tiles: Query<(&DirectlyHovered, &TalentTile)>,
    cells: Query<(&DirectlyHovered, &ShopCell)>,
    buffs: Query<(&DirectlyHovered, &crate::buffs::BuffChip)>,
    talent_cells: Query<(&DirectlyHovered, &crate::talents::TalentCell)>,
    attribute: Query<&DirectlyHovered, With<crate::talents::AttributeRow>>,
    innate: Query<&DirectlyHovered, With<crate::hud::InnateButton>>,
    mut shown: ResMut<Shown>,
    mut anchor: ResMut<ShownAnchor>,
) {
    // A popup of the game stands by the cursor.
    if anchor.0.is_some() {
        anchor.0 = None;
    }
    let core = &session.0;
    let rate = u32::from(core.tick_rate().max(1));
    // The numbers of the unit the tooltip is about: what shortens its cooldowns and costs (an Arcane Rune) shortens what it shows.
    let rates = core
        .commanded()
        .and_then(|id| core.view()?.unit(id))
        .map_or_else(Default::default, crate::tip_numbers::Rates::of_view);
    let look = Lookup {
        texts: &texts,
        manifest: &manifest,
        rates,
    };
    let buff = buffs
        .iter()
        .find(|(i, _)| i.get())
        .and_then(|(_, b)| b.shown);
    let content = if let Some(spec) = args.tip.as_deref() {
        forced_tip(&look, spec)
    } else if attribute.iter().any(|i| i.get()) {
        Some(note_tip(attribute_tip(&texts, core)))
    } else if innate.iter().any(|i| i.get()) {
        innate_tip(&look, core)
    } else if let Some(shown) = buff {
        Some(buff_tip(&look, shown.id, shown.ticks_left, rate))
    } else if let Some(cell) = hovered(&talent_cells) {
        talent_tip(core, cell.0).map(note_tip)
    } else if let Some(slot) = hovered(&abilities) {
        ability_tip(&look, core, slot.0, rate)
    } else if let Some(ability) = hovered(&tiles) {
        entering_tip(&look, ability.0)
    } else if let Some(hero) = hovered(&talent_tiles) {
        talents_tip(hero.0)
    } else if let Some(slot) = hovered(&items) {
        item_tip(&look, core, &bag, slot.0, rate)
    } else if let Some(cell) = hovered(&cells) {
        shop_tip(&look, core, cell.0)
    } else {
        None
    };
    if shown.0 != content {
        shown.0 = content;
    }
}

/// What the cursor is over on the pick screen, before any hero is one's own, and on a hero's page
/// of the menu: an ability or the talent tree of the hero shown. The popup stands beside the tile
/// (see [`ShownAnchor`]) and stays up exactly while the cursor is over one, so it leaves alone
/// what the menu's other pages show.
fn pick_pregame(
    texts: Res<Texts>,
    manifest: Res<Manifest>,
    tiles: Query<(
        &DirectlyHovered,
        &AbilityTile,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    talent_tiles: Query<(
        &DirectlyHovered,
        &TalentTile,
        &ComputedNode,
        &UiGlobalTransform,
    )>,
    mut shown: ResMut<Shown>,
    mut anchor: ResMut<ShownAnchor>,
    mut was_up: Local<bool>,
) {
    let look = Lookup {
        texts: &texts,
        manifest: &manifest,
        rates: Default::default(),
    };
    let over = tiles.iter().find(|(hover, ..)| hover.get());
    let over_talents = talent_tiles.iter().find(|(hover, ..)| hover.get());
    let content = over
        .and_then(|(_, tile, ..)| entering_tip(&look, tile.0))
        .or_else(|| over_talents.and_then(|(_, tile, ..)| talents_tip(tile.0)));
    let rect = over
        .map(|(.., node, at)| Rect::from_center_size(at.translation, node.size()))
        .or_else(|| {
            over_talents.map(|(.., node, at)| Rect::from_center_size(at.translation, node.size()))
        });
    if content.is_some() || *was_up {
        *was_up = content.is_some();
        if shown.0 != content {
            shown.0 = content;
        }
        if anchor.0 != rect {
            anchor.0 = rect;
        }
    }
}

/// A hero's talents, a line for each level that opens a pair.
fn talents_tip(hero: u16) -> Option<Content> {
    let tree = crate::faces::talents(hero)?;
    let mut lines = vec!["Talent Tree".to_string()];
    for (tier, level) in bota_core::rules::TALENT_LEVELS.iter().enumerate().rev() {
        let (first, second) = (tree.get(2 * tier)?, tree.get(2 * tier + 1)?);
        lines.push(format!("Level {level}: {} / {}", first.name, second.name));
    }
    Some(note_tip(lines.join("\n")))
}

/// An ability of the hero one enters battle as, in the strategy phase: Dota's
/// tooltip of it at its first level.
fn entering_tip(look: &Lookup, id: u16) -> Option<Content> {
    let max = bota_core::ability_def(bota_proto::AbilityId(id))?.max_level;
    look.ability(id, 1, max)
}

/// `--tip item:ID`, `--tip shop:ID` or `--tip ability:ID:LEVEL`: a popup that
/// stays, for screenshots.
fn forced_tip(look: &Lookup, spec: &str) -> Option<Content> {
    let mut parts = spec.split(':');
    let kind = parts.next()?;
    let id: u16 = parts.next()?.parse().ok()?;
    match kind {
        "item" => look.item(id, None),
        "shop" => look.item(id, item_def(bota_proto::ItemId(id)).map(|d| d.cost)),
        // A buff with ten seconds left, at Dota's 30 ticks a second.
        "buff" => Some(buff_tip(look, id, Some(300), 30)),
        _ => {
            let level = parts.next().and_then(|l| l.parse().ok()).unwrap_or(1);
            look.ability(id, level, 4)
        }
    }
}

/// What of a query's boxes the cursor is over, if any.
fn hovered<'a, T: Component>(boxes: &'a Query<(&DirectlyHovered, &T)>) -> Option<&'a T> {
    boxes.iter().find(|(i, _)| i.get()).map(|(_, b)| b)
}

/// A popup of the client's own words: a title and lines under it.
fn note_tip(text: String) -> Content {
    let mut lines = text.lines();
    let title = lines.next().unwrap_or_default().to_string();
    let rest: Vec<&str> = lines.collect();
    Content {
        title,
        sections: if rest.is_empty() {
            Vec::new()
        } else {
            vec![Section {
                kind: Kind::Passive,
                header: None,
                body: vec![Run {
                    text: rest.join("\n"),
                    tone: Tone::Body,
                    bold: false,
                    color: None,
                }],
            }]
        },
        ..Content::default()
    }
}

/// The Attribute Bonus of the talent tree: Dota's name and description,
/// the simulator's level and points.
fn attribute_tip(texts: &Texts, core: &bota_client_core::App) -> String {
    let level = crate::hud::subject(core)
        .0
        .and_then(|p| core.talents_of(p.slot))
        .map_or(0, |t| t.attribute_bonus);
    let name = texts
        .label("DOTA_Tooltip_ability_attribute_bonus", &[])
        .unwrap_or_else(|| "Attribute Bonus".to_string());
    let about = texts
        .label("DOTA_Tooltip_ability_attribute_bonus_Description", &[])
        .unwrap_or_else(|| "Improves every attribute with each level.".to_string());
    format!(
        "{name}  ({level}/{})\n{about}\n+{} to all attributes per level.",
        bota_core::rules::ATTRIBUTE_BONUS_MAX_LEVEL,
        bota_core::rules::ATTRIBUTE_BONUS_POINTS,
    )
}

/// The innate ability of the panel's hero, Dota's tooltip of it.
fn innate_tip(look: &Lookup, core: &bota_client_core::App) -> Option<Content> {
    let id = crate::hud::subject_hero(core)
        .and_then(bota_core::hero_def)?
        .innate?;
    look.ability(id.0, 1, 1)
}

/// A buff's or a debuff's tooltip, as Dota's: its name as written, what it
/// does, and after one blank line the time it has left.
fn buff_tip(look: &Lookup, id: u16, ticks_left: Option<u32>, rate: u32) -> Content {
    let mut body = crate::buff_words::description(look, id);
    if let Some(t) = ticks_left {
        body.push_str(&format!("\n\n{:.1} s left", t as f32 / rate as f32));
    }
    let mut content = note_tip(format!(
        "{}\n{}",
        crate::buff_words::name(look.texts, look.manifest, id),
        body.trim_start_matches('\n')
    ));
    content.as_written = true;
    content
}

fn talent_tip(core: &bota_client_core::App, slot: u8) -> Option<String> {
    crate::hud::subject_hero(core)
        .and_then(|h| crate::faces::talents(h.0))
        .map(|tree| {
            let face = &tree[usize::from(slot)];
            let (level, second) = bota_client_core::talent_levels(TalentSlot(slot));
            format!(
                "{}\n{}\nOpens at level {level}; the other of its tier at {second}.",
                face.name, face.blurb
            )
        })
}

/// An ability of the unit the bottom panel is about, the one it shows.
fn ability_tip(
    look: &Lookup,
    core: &bota_client_core::App,
    slot: u8,
    rate: u32,
) -> Option<Content> {
    let (_, unit, kit) = crate::hud::subject(core);
    let a = crate::hud::subject_abilities(unit, kit).get(usize::from(slot))?;
    let mut content = look.ability(a.id.0, a.level, a.max_level)?;
    if a.cooldown_left > 0 {
        content
            .hints
            .push(format!("Ready in {} s", a.cooldown_left.div_ceil(rate)));
    }
    if a.learnable == Some(true) {
        content.hints.push("Ctrl + its key: learn".to_string());
    }
    Some(content)
}

fn item_tip(
    look: &Lookup,
    core: &bota_client_core::App,
    bag: &crate::bag::TacticalBag,
    slot: u8,
    rate: u32,
) -> Option<Content> {
    let (_, unit, kit) = crate::hud::subject(core);
    let i = &crate::hud::subject_item(core, bag, unit, kit, slot)?;
    let mut content = look.item(i.id.0, None)?;
    if i.cooldown_left > 0 {
        content
            .hints
            .push(format!("Ready in {} s", i.cooldown_left.div_ceil(rate)));
    }
    // The state it is in, in Dota's words (the attribute Power Treads are set to, the rune a
    // Bottle keeps), and the seconds before a kept rune takes effect by itself.
    if let Some(line) =
        crate::item_look::state_text(i, look.manifest).and_then(|key| look.texts.label(key, &[]))
    {
        content.hints.push(match i.rune {
            Some(rune) => format!("{line} ({} s)", rune.ticks_left.div_ceil(rate)),
            None => line,
        });
    }
    if slot >= bota_client_core::INVENTORY_SLOTS && slot != bota_client_core::TOWN_PORTAL_SLOT {
        content
            .hints
            .push("Drag it into the inventory to use it.".to_string());
    }
    Some(content)
}

fn shop_tip(look: &Lookup, core: &bota_client_core::App, id: u16) -> Option<Content> {
    let (now, whole) = core.price_of(id);
    let mut content = look.item(id, Some(now))?;
    if now < whole {
        content
            .hints
            .push(format!("{} gold of it is already in hand.", whole - now));
    }
    if let Some(parts) = look.parts_line(id) {
        content.hints.push(parts);
    }
    Some(content)
}

/// Draws the popup anew when what it holds changes.
fn rebuild(
    mut commands: Commands,
    shown: Res<Shown>,
    body: Single<Entity, With<TipBody>>,
    mut tip: Single<&mut Visibility, With<Tip>>,
    fonts: Res<Fonts>,
    server: Res<AssetServer>,
    manifest: Res<Manifest>,
) {
    if !shown.is_changed() {
        return;
    }
    commands.entity(*body).despawn_children();
    let Some(content) = &shown.0 else {
        tip.set_if_neq(Visibility::Hidden);
        return;
    };
    let mut draw = Draw {
        commands: &mut commands,
        fonts: &fonts,
        server: &server,
        manifest: &manifest,
        parent: *body,
    };
    draw.content(content);
}

/// Puts a popup that is about something beside it, clear of it, and hides the frame in which a new
/// size has not been laid out yet.
fn place_beside(
    anchor: Rect,
    window: &Window,
    tip: (
        &mut Node,
        &mut Mut<Visibility>,
        &ComputedNode,
        &UiGlobalTransform,
    ),
) {
    let (node, visibility, computed, at) = tip;
    let unit = computed.inverse_scale_factor;
    let size = computed.size();
    let extent = Vec2::new(
        window.physical_width() as f32,
        window.physical_height() as f32,
    );
    let margin = Vec2::splat(8.0 / unit.max(1e-3));
    let bounds = Rect::from_corners(margin, extent - margin);
    let gap = 12.0 / unit.max(1e-3);
    // The shared help widget's rule: clear of the whole anchor, the preferred side then the others, within the window.
    let Some(corner) = position(anchor, size, anchor.center(), bounds, gap, Side::Right)
        .filter(|_| size.min_element() > 0.0)
    else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    node.left = px(corner.x * unit);
    node.top = px(corner.y * unit);
    // Layout applies the new corner next frame: the box that is drawn now may still cover the anchor.
    let drawn = Rect::from_center_size(at.translation, size);
    let apart = (anchor.min - drawn.max)
        .max(drawn.min - anchor.max)
        .max_element();
    visibility.set_if_neq(if apart >= 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
}

/// Puts the popup beside what it is about, else by the cursor, above it where there is room.
fn place(
    shown: Res<Shown>,
    anchor: Res<ShownAnchor>,
    args: Res<crate::Args>,
    window: Single<&Window, With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut tip: Single<
        (
            &mut Node,
            &mut Visibility,
            &ComputedNode,
            &UiGlobalTransform,
        ),
        With<Tip>,
    >,
) {
    let (node, visibility, computed, at) = &mut *tip;
    if shown.0.is_none() {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    }
    if let Some(anchor) = anchor.0 {
        place_beside(anchor, &window, (node, visibility, computed, at));
        return;
    }
    let cursor = window.cursor_position().or_else(|| {
        args.tip
            .is_some()
            .then(|| Vec2::new(60.0, window.height() * 0.5))
    });
    let Some(cursor) = cursor else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let scale = ui_scale.0.max(0.1);
    let (width, height) = (window.width() / scale, window.height() / scale);
    let tall = computed.size().y * computed.inverse_scale_factor;
    let x = (cursor.x / scale + GAP).min(width - WIDTH - 10.0);
    let y = (cursor.y / scale - GAP - tall).clamp(4.0, (height - tall - 4.0).max(4.0));
    node.left = px(x.max(4.0));
    node.top = px(y);
    visibility.set_if_neq(Visibility::Inherited);
}

/// Spawns the parts of a popup.
struct Draw<'a, 'w, 's> {
    commands: &'a mut Commands<'w, 's>,
    fonts: &'a Fonts,
    server: &'a AssetServer,
    manifest: &'a Manifest,
    parent: Entity,
}

impl Draw<'_, '_, '_> {
    fn content(&mut self, c: &Content) {
        self.header(c);
        if !c.bonuses.is_empty() {
            self.lines(&c.bonuses, LINES, 8.0, 2.0);
        }
        for section in &c.sections {
            self.section(section, c.is_item);
        }
        if !c.stats.is_empty() {
            self.lines(&c.stats, ABILITY, 8.0, 2.0);
        }
        if !c.costs.is_empty() {
            self.costs(&c.costs);
        }
        for note in &c.notes {
            self.block(
                note,
                Palette {
                    body: NOTE,
                    ..ABILITY
                },
                13.0,
                8.0,
                None,
            );
        }
        if !c.lore.is_empty() {
            let run = Run {
                text: c.lore.clone(),
                tone: Tone::Body,
                bold: false,
                color: None,
            };
            self.block(
                &[run],
                Palette {
                    body: LORE,
                    ..ABILITY
                },
                13.0,
                10.0,
                Some(LORE_BG),
            );
        }
        for hint in &c.hints {
            let run = Run {
                text: hint.clone(),
                tone: Tone::Body,
                bold: false,
                color: None,
            };
            self.block(
                &[run],
                Palette {
                    body: ui::INK_DIM,
                    ..ABILITY
                },
                12.0,
                8.0,
                None,
            );
        }
    }

    fn header(&mut self, c: &Content) {
        let row = self
            .commands
            .spawn((
                Node {
                    padding: UiRect::all(px(6)),
                    column_gap: px(8),
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(self.parent),
            ))
            .id();
        if let Some(path) = &c.icon {
            let (w, h) = if c.is_item {
                (60.0, 44.0)
            } else {
                (44.0, 44.0)
            };
            self.commands.spawn((
                ImageNode::new(self.server.load(path.clone())),
                Node {
                    width: px(w),
                    height: px(h),
                    flex_shrink: 0.0,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row),
            ));
        }
        let column = self
            .commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    flex_grow: 1.0,
                    row_gap: px(2),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(row),
            ))
            .id();
        let top = self
            .commands
            .spawn((
                Node {
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(8),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(column),
            ))
            .id();
        self.commands.spawn((
            ui::text(
                if c.as_written {
                    c.title.clone()
                } else {
                    c.title.to_uppercase()
                },
                self.fonts.face(Face::Bold),
                17.0,
                NAME,
            ),
            Pickable::IGNORE,
            ChildOf(top),
        ));
        if let Some(level) = &c.level {
            self.commands.spawn((
                ui::text(
                    level.clone(),
                    self.fonts.face(Face::Regular),
                    14.0,
                    Color::WHITE,
                ),
                Pickable::IGNORE,
                ChildOf(top),
            ));
        }
        if let Some(price) = &c.price {
            let cost = self
                .commands
                .spawn((
                    Node {
                        column_gap: px(4),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                    ChildOf(column),
                ))
                .id();
            if let Some(path) = self.manifest.file("hud.gold") {
                self.commands.spawn((
                    ImageNode::new(self.server.load(path)),
                    Node {
                        width: px(16),
                        height: px(16),
                        ..default()
                    },
                    Pickable::IGNORE,
                    ChildOf(cost),
                ));
            }
            self.commands.spawn((
                ui::text(price.clone(), self.fonts.face(Face::Bold), 15.0, ui::GOLDEN),
                Pickable::IGNORE,
                ChildOf(cost),
            ));
        }
    }

    fn section(&mut self, section: &Section, is_item: bool) {
        let palette = if is_item {
            section_palette(section.kind)
        } else {
            ABILITY
        };
        if let Some(header) = &section.header {
            self.bar(header, section.kind, palette, is_item);
        }
        if !section.body.is_empty() {
            self.block(&section.body, palette, 14.0, 8.0, palette.ground);
        }
    }

    /// The heading of a section: over Dota's bar for an item.
    fn bar(&mut self, text: &str, kind: Kind, palette: Palette, is_item: bool) {
        let logical = match kind {
            Kind::Passive => "tip.passive",
            Kind::Active => "tip.active",
            Kind::Use => "tip.use",
        };
        let mut bar = self.commands.spawn((
            Node {
                width: percent(100),
                height: px(26),
                padding: UiRect::new(px(8), px(8), px(4), px(0)),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(self.parent),
        ));
        // Dota's header label wears the section's ground like its body, the bar's art over it.
        bar.insert(BackgroundColor(palette.ground.unwrap_or(Color::NONE)));
        if let Some(path) = is_item.then(|| self.manifest.file(logical)).flatten() {
            bar.insert(ImageNode::new(self.server.load(path)));
        }
        let bar = bar.id();
        self.commands.spawn((
            ui::text(
                text.to_string(),
                self.fonts.face(Face::Bold),
                14.0,
                palette.header,
            ),
            Pickable::IGNORE,
            ChildOf(bar),
        ));
    }

    fn lines(&mut self, lines: &[Vec<Run>], palette: Palette, pad: f32, gap: f32) {
        let column = self
            .commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(gap),
                    padding: UiRect::new(px(pad), px(pad), px(2), px(6)),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(self.parent),
            ))
            .id();
        for line in lines {
            self.rich(line, palette, 15.0, column);
        }
    }

    fn costs(&mut self, costs: &[Cost]) {
        let row = self
            .commands
            .spawn((
                Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(14),
                    padding: UiRect::new(px(8), px(8), px(2), px(6)),
                    align_items: AlignItems::Center,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(self.parent),
            ))
            .id();
        for cost in costs {
            let cell = self
                .commands
                .spawn((
                    Node {
                        column_gap: px(4),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    Pickable::IGNORE,
                    ChildOf(row),
                ))
                .id();
            match &cost.icon {
                Some(path) => {
                    self.commands.spawn((
                        ImageNode::new(self.server.load(path.clone())),
                        Node {
                            width: px(18),
                            height: px(18),
                            ..default()
                        },
                        Pickable::IGNORE,
                        ChildOf(cell),
                    ));
                }
                None => {
                    self.commands.spawn((
                        ui::text(cost.word, self.fonts.face(Face::Regular), 14.0, STAT_LABEL),
                        Pickable::IGNORE,
                        ChildOf(cell),
                    ));
                }
            }
            self.rich(&cost.figures, ABILITY, 15.0, cell);
        }
    }

    /// A block of text on its own ground.
    fn block(
        &mut self,
        runs: &[Run],
        palette: Palette,
        size: f32,
        pad: f32,
        ground: Option<Color>,
    ) {
        let mut block = self.commands.spawn((
            Node {
                padding: UiRect::new(px(pad), px(pad), px(5), px(7)),
                width: percent(100),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(self.parent),
        ));
        if let Some(color) = ground {
            block.insert(BackgroundColor(color));
        }
        let block = block.id();
        self.rich(runs, palette, size, block);
    }

    /// Text made of runs of several colours.
    fn rich(&mut self, runs: &[Run], palette: Palette, size: f32, parent: Entity) {
        let text = self
            .commands
            .spawn((
                Text::default(),
                TextFont {
                    font: self.fonts.face(Face::Regular).clone().into(),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(palette.body),
                Node {
                    flex_shrink: 1.0,
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(parent),
            ))
            .id();
        for run in runs {
            let font = if run.bold {
                self.fonts.face(Face::Bold)
            } else {
                self.fonts.face(Face::Regular)
            };
            self.commands.spawn((
                TextSpan::new(run.text.clone()),
                TextFont {
                    font: font.clone().into(),
                    font_size: FontSize::Px(size),
                    ..default()
                },
                TextColor(run.color.unwrap_or_else(|| tone_color(run.tone, &palette))),
                ChildOf(text),
            ));
        }
    }
}
