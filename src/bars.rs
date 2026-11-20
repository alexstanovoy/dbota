//! Health bars over units, Dota's way: screen-space bars that keep their
//! size, the health just lost trailing off, a small mark every 250 health
//! and a big one every 1000 (`dota_health_per_vertical_marker`), level and
//! name; health counted in hits as pips, grouped for enemy Fragile units
//! when viewed by a melee hero.
//! Also the numbers and words that rise off units.

use std::collections::HashMap;

use bevy::prelude::*;
use bota_client_core::{Floater, FloaterKind};
use bota_core::{HitCount, QUARTERS_A_PIP, Striker};
use bota_proto::{Team, UnitKind, UnitView, WorldView};

use crate::config::Settings;
use crate::map::Ground;
use crate::session::Session;
use crate::ui::{self, Fonts};
use crate::units::{Dying, Unit};

/// Floating texts shown at once at most.
const FLOATERS: usize = 48;
/// The health of one's own hero on the Dire side.
const HP_DIRE_OWN: Color = Color::srgb(1.0, 0.3, 0.24);

/// Dota's health markers: a small one every this much health
/// (`dota_health_per_vertical_marker`)...
pub const TICK_HP: i32 = 250;
/// ...and a big one every this much.
pub const BIG_TICK_HP: i32 = 1000;
/// Marks a bar holds at most.
const MAX_TICKS: usize = 48;
/// Interface pixels the small marks keep apart at least: closer (a
/// building's thousands), only the big ones are drawn.
const TICK_SPACING_MIN: f32 = 4.0;
/// Blocks of health counted in hits a bar holds at most.
const MAX_SEGMENTS: usize = 24;
/// Interface pixels between two blocks.
const SEGMENT_GAP: f32 = 1.5;
/// The marks' colours: Dota's dark lines over the health, the big ones
/// darker (`dota_health_marker_minor_alpha`, `..._major_alpha`).
const TICK: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
const BIG_TICK: Color = Color::srgba(0.0, 0.0, 0.0, 0.95);
/// The marks' widths in interface pixels, small and big: a hair at 1080
/// lines, still a line at 4K.
pub const TICK_WIDTH: f32 = 1.5;
pub const BIG_TICK_WIDTH: f32 = 2.5;

/// One unit's bar and its parts.
pub struct Bar {
    root: Entity,
    ghost: Entity,
    fill: Entity,
    /// The marks over the health, and the maximum and width they were laid
    /// out for.
    ticks: Vec<Entity>,
    laid_for: (i32, u32),
    /// The drawn groups of pips, and the layout they were spawned for.
    segments: Vec<Entity>,
    hit_blocks: Option<HitBlocks>,
    mana: Option<Entity>,
    level: Option<Entity>,
    name: Option<crate::player_name::Label>,
    /// The well and the fill of a hero's teleport channel.
    channel: Option<(Entity, Entity)>,
    /// The colour of the bar's outline when its unit is not under the cursor.
    outline: Color,
}

/// Whether a unit is a creep, whose bar has Dota's frame round a grey well
/// (`creep_health_bar_*.svg`); the other bars are as they were.
fn is_creep(kind: UnitKind) -> bool {
    bota_core::kind_def(kind).creep
}

/// The bar of every unit root.
#[derive(Resource, Default)]
pub struct Bars(HashMap<Entity, Bar>);

impl Bars {
    /// Whether a unit root has a health bar.
    pub fn has(&self, unit: Entity) -> bool {
        self.0.contains_key(&unit)
    }

    /// The health fill of a unit root's bar.
    pub fn fill(&self, unit: Entity) -> Option<Entity> {
        self.0.get(&unit).map(|bar| bar.fill)
    }

    /// The root of a unit root's bar.
    pub fn root(&self, unit: Entity) -> Option<Entity> {
        self.0.get(&unit).map(|bar| bar.root)
    }

    /// The marks of a unit root's bar, the hidden ones too.
    pub fn ticks(&self, unit: Entity) -> &[Entity] {
        self.0.get(&unit).map_or(&[], |bar| &bar.ticks)
    }

    /// The blocks of a unit root's bar of health counted in hits.
    pub fn segments(&self, unit: Entity) -> &[Entity] {
        self.0.get(&unit).map_or(&[], |bar| &bar.segments)
    }
}

/// Where a bar `w` wide marks a maximum of `max_hp`: every [`TICK_HP`] short
/// of it, and whether that one is a big mark ([`BIG_TICK_HP`]). Small marks
/// that would crowd closer than [`TICK_SPACING_MIN`] are left out.
pub fn tick_marks(max_hp: i32, w: f32) -> Vec<(f32, bool)> {
    if max_hp <= TICK_HP {
        return Vec::new();
    }
    let step = w * TICK_HP as f32 / max_hp as f32;
    (1..)
        .map(|k| k * TICK_HP)
        .take_while(|hp| *hp < max_hp)
        .take(MAX_TICKS)
        .map(|hp| (w * hp as f32 / max_hp as f32, hp % BIG_TICK_HP == 0))
        .filter(|(_, big)| *big || step >= TICK_SPACING_MIN)
        .collect()
}

/// The team of the local melee hero. A spectator's camera can follow any player.
pub(crate) fn melee_viewer_team(core: &bota_client_core::App, view: &WorldView) -> Option<Team> {
    let player = view.player(core.my_slot?)?;
    let hero = bota_core::hero_def(player.hero)?;
    hero.unit.projectile_speed.is_none().then_some(player.team)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct HitBlocks {
    pips: usize,
    pips_per_hit: usize,
}

/// Group the target's pips by how much the local melee hero's attack takes.
pub(crate) fn hit_blocks(live: &UnitView, melee_team: Option<Team>) -> Option<HitBlocks> {
    use bota_proto::Fixed;
    let count = bota_core::kind_def(live.kind).hit_count?;
    let max = Fixed::from_int(live.max_hp.clamp(1, Fixed::MAX.to_int()));
    let pip = match count {
        HitCount::Pips { .. } => QUARTERS_A_PIP,
        HitCount::Hits { .. } => count.taken(Striker::RangedHero, max).to_int().max(1),
    };
    let pips = (live.max_hp.max(1) / pip).max(1) as usize;
    let pips_per_hit = if melee_team.is_some_and(|team| team != live.team)
        && live.hit_health.is_some()
        && matches!(count, HitCount::Pips { fragile: true })
    {
        (count.taken(Striker::MeleeHero, max).to_int() / pip).max(1) as usize
    } else {
        1
    };
    Some(HitBlocks {
        pips: pips.min(MAX_SEGMENTS),
        pips_per_hit,
    })
}

pub(crate) struct HitBlock {
    pub left: f32,
    pub width: f32,
    pub fill: f32,
    pub shaded: bool,
}

impl HitBlocks {
    pub(crate) fn layout(self, live: &UnitView, width: f32) -> impl Iterator<Item = HitBlock> {
        let pip_width = block_width(width, self.pips);
        let pip_health = live.max_hp.max(1) as f32 / self.pips as f32;
        let health = exact_health(live);
        (0..self.pips).step_by(self.pips_per_hit).map(move |first| {
            let pips = (self.pips - first).min(self.pips_per_hit);
            HitBlock {
                left: first as f32 * (pip_width + SEGMENT_GAP),
                width: pips as f32 * (pip_width + SEGMENT_GAP) - SEGMENT_GAP,
                fill: ((health - first as f32 * pip_health) / (pips as f32 * pip_health))
                    .clamp(0.0, 1.0),
                shaded: pips > 1,
            }
        })
    }
}

/// A unit's health exactly where it is counted in hits, whole otherwise.
pub fn exact_health(live: &UnitView) -> f32 {
    live.hit_health
        .map_or(live.hp as f32, |health| health.to_f32())
        .max(0.0)
}

/// The width of one whole block of a bar `w` wide with `blocks` of them.
fn block_width(w: f32, blocks: usize) -> f32 {
    let gaps = SEGMENT_GAP * (blocks as f32 - 1.0);
    ((w - gaps) / blocks as f32).max(1.0)
}

/// The pool of floating texts.
#[derive(Resource, Default)]
struct Floaters(Vec<Entity>);

/// The lightning marks beside the pool's texts, one each: shown beside a critical strike's number.
#[derive(Resource, Default)]
struct Marks(Vec<Entity>);

/// A mark of the pool, for the queries that tell it from the texts.
#[derive(Component)]
struct Mark;

/// The meaning of a rendered floating caption, separate from its localized text.
#[derive(Component, Default)]
struct FloaterText(Option<FloaterKind>);

/// The layer the bars and floaters sit in, under the rest of the interface.
#[derive(Resource)]
struct Layer(Entity);

pub fn plugin(app: &mut App) {
    app.init_resource::<Bars>()
        .init_resource::<Floaters>()
        .init_resource::<Marks>()
        .add_systems(First, reset.in_set(crate::loading::Begin))
        .add_systems(Startup, spawn_layer)
        // Placed from the camera's own transform, not its propagated one,
        // so the bars and the interface's layout run beside the transform
        // propagation of every animated skeleton, not after it.
        .add_systems(
            PostUpdate,
            (bars, floaters)
                .after(bevy::camera::CameraUpdateSystems)
                .before(bevy::ui::UiSystems::Layout),
        )
        .add_systems(Update, hover_outline.after(crate::pick::update));
}

/// The well of a bar, top and bottom.
const WELL: (Color, Color) = (
    Color::srgb(0.349, 0.349, 0.349),
    Color::srgb(0.502, 0.502, 0.502),
);

/// The outline of a bar: dark, and light while its unit is under the
/// cursor, as Dota lights a hovered unit's bar
/// (`dota_hud_healthbar_hoveroutline_alpha`). A creep's is Dota's opaque black stroke.
const OUTLINE: Color = Color::srgba(0.0, 0.0, 0.0, 0.9);
const OUTLINE_CREEP: Color = Color::BLACK;
const OUTLINE_HOVERED: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);

/// Lights the bar of the unit a click would get, and dims the one lit
/// before.
fn hover_outline(
    hover: Res<crate::pick::Hover>,
    index: Res<crate::units::UnitIndex>,
    bars: Res<Bars>,
    mut outlines: Query<&mut Outline>,
    mut lit: Local<Option<(Entity, Color)>>,
) {
    let now = hover
        .left()
        .and_then(|(id, _)| index.0.get(&id))
        .and_then(|unit| bars.0.get(unit))
        .map(|bar| (bar.root, bar.outline));
    if now.map(|(root, _)| root) == lit.map(|(root, _)| root) {
        return;
    }
    if let Some((root, rest)) = *lit
        && let Ok(mut outline) = outlines.get_mut(root)
    {
        outline.color = rest;
    }
    if let Some((root, _)) = now
        && let Ok(mut outline) = outlines.get_mut(root)
    {
        outline.color = OUTLINE_HOVERED;
    }
    *lit = now;
}

fn spawn_layer(
    mut commands: Commands,
    fonts: Res<Fonts>,
    mut pool: ResMut<Floaters>,
    mut marks: ResMut<Marks>,
    mut images: ResMut<Assets<Image>>,
) {
    let bolt = images.add(crate::bolt::image());
    let layer = commands
        .spawn((ui::fill(), Pickable::IGNORE, GlobalZIndex(-10)))
        .id();
    for _ in 0..FLOATERS {
        let text = commands
            .spawn((
                ui::text("", &fonts.bold, 16.0, Color::WHITE),
                FloaterText::default(),
                ui::shadow(),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
                ChildOf(layer),
            ))
            .id();
        pool.0.push(text);
        let mark = commands
            .spawn((
                Mark,
                ImageNode::new(bolt.clone()),
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Visibility::Hidden,
                ChildOf(layer),
            ))
            .id();
        marks.0.push(mark);
    }
    commands.insert_resource(Layer(layer));
}

/// Whether a unit of a kind is drawn with a bar.
pub fn shows_bar(kind: UnitKind) -> bool {
    bar_size(kind).is_some()
}

/// Width and height of a unit's bar in interface pixels: Dota's own sizes at a
/// 1080-line screen (a hero's 110 by 10, a creep's 55 by 5), turned into
/// interface pixels, so they hold at 1080 and 4K alike.
fn bar_size(kind: UnitKind) -> Option<(f32, f32)> {
    let dota = |w: f32, h: f32| Some((w * ui::DOTA, h * ui::DOTA));
    match kind {
        UnitKind::Hero => dota(110.0, 10.0),
        UnitKind::Tower | UnitKind::Barracks | UnitKind::Ancient => dota(120.0, 8.0),
        UnitKind::Filler => dota(90.0, 7.0),
        // The map's objectives never take damage: Dota shows them no health.
        UnitKind::Fountain
        | UnitKind::Outpost
        | UnitKind::Watcher
        | UnitKind::Shrine
        | UnitKind::LotusPool
        | UnitKind::TwinGate => None,
        UnitKind::Courier => dota(50.0, 6.0),
        UnitKind::Ward => dota(40.0, 5.0),
        _ => dota(55.0, 5.0),
    }
}

/// The height of a hero's mana bar under its health.
const MANA_HEIGHT: f32 = 4.0 * ui::DOTA;
/// Interface pixels around a bar that its parts (the mana and channel wells
/// below) may reach: a bar this near the screen is laid out and drawn.
const BAR_REACH: f32 = 40.0;

/// How high over its feet a unit's bar floats, in world units.
pub(crate) fn bar_height(kind: UnitKind) -> f32 {
    match kind {
        UnitKind::Hero => 260.0,
        UnitKind::Tower => 560.0,
        UnitKind::Barracks | UnitKind::Ancient => 420.0,
        UnitKind::Filler => 330.0,
        UnitKind::CreepSiege => 170.0,
        _ => 165.0,
    }
}

fn spawn_bar(commands: &mut Commands, layer: Entity, fonts: &Fonts, live: &UnitView) -> Bar {
    let kind = live.kind;
    let (w, h) = bar_size(kind).unwrap_or((40.0, 5.0));
    let creep = is_creep(kind);
    let outline = if creep { OUTLINE_CREEP } else { OUTLINE };
    let root = commands
        .spawn((
            ui::absolute(0.0, 0.0, w, h),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
            Outline::new(
                px(if creep { ui::DOTA.max(1.0) } else { 1.0 }),
                px(0.0),
                outline,
            ),
            Visibility::Hidden,
            ChildOf(layer),
        ))
        .id();
    if creep {
        // Dota's well (`creep_health_bar_background.svg`: #595959 to #808080) in Dota's black
        // stroke: the bars of creeps that stand close are told apart.
        commands.entity(root).insert((
            BackgroundColor(WELL.0),
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                WELL.0.into(),
                WELL.1.into(),
            ])),
        ));
    }
    let part = |commands: &mut Commands, color: Color| {
        commands
            .spawn((
                ui::absolute(0.0, 0.0, w, h),
                BackgroundColor(color),
                ChildOf(root),
            ))
            .id()
    };
    let ghost = part(commands, Color::srgba(1.0, 0.9, 0.7, 0.85));
    let fill = part(commands, ui::HP_FOE);
    let hero = kind == UnitKind::Hero;
    let mana = hero.then(|| {
        let well = commands
            .spawn((
                ui::absolute(0.0, h + 1.0, w, MANA_HEIGHT),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                ChildOf(root),
            ))
            .id();
        commands
            .spawn((
                ui::absolute(0.0, 0.0, w, MANA_HEIGHT),
                BackgroundColor(ui::MANA),
                ChildOf(well),
            ))
            .id()
    });
    let channel = hero.then(|| {
        let well = commands
            .spawn((
                ui::absolute(0.0, h + MANA_HEIGHT + 2.0, w, 4.0),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                Visibility::Hidden,
                ChildOf(root),
            ))
            .id();
        let fill = commands
            .spawn((
                ui::absolute(0.0, 0.0, 0.0, 4.0),
                BackgroundColor(ui::GOLDEN),
                ChildOf(well),
            ))
            .id();
        (well, fill)
    });
    let level = hero.then(|| {
        let plate = commands
            .spawn((
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(-17.0),
                    top: px(-3.0),
                    width: px(16.0),
                    height: px(h + MANA_HEIGHT + 3.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        commands
            .spawn((ui::text("1", &fonts.bold, 11.0, ui::GOLDEN), ChildOf(plate)))
            .id()
    });
    let name = hero.then(|| {
        let label = crate::player_name::spawn(
            commands,
            fonts,
            root,
            "",
            None,
            crate::theme::Type {
                size: 10.0,
                face: crate::theme::Face::Bold,
                ..crate::theme::BODY
            },
            ui::INK,
        );
        commands.entity(label.root).insert(Node {
            position_type: PositionType::Absolute,
            bottom: px(h + 3.0),
            left: px(0),
            width: px(240),
            min_width: px(0),
            align_items: AlignItems::Center,
            column_gap: px(4),
            overflow: Overflow::clip(),
            ..default()
        });
        label
    });
    Bar {
        root,
        ghost,
        fill,
        ticks: Vec::new(),
        laid_for: (0, 0),
        segments: Vec::new(),
        hit_blocks: None,
        mana,
        level,
        name,
        channel,
        outline,
    }
}

fn lay_segments(
    commands: &mut Commands,
    bar: &mut Bar,
    live: &UnitView,
    (w, h): (f32, f32),
    layout: Option<HitBlocks>,
    color: Color,
) {
    if bar.hit_blocks == layout {
        return;
    }
    bar.hit_blocks = layout;
    for old in bar.segments.drain(..) {
        commands.entity(old).despawn();
    }
    let Some(layout) = layout else {
        return;
    };
    for block in layout.layout(live, w) {
        let mut node = ui::absolute(block.left, 0.0, block.width * block.fill, h);
        node.overflow = Overflow::clip();
        let fill = commands
            .spawn((
                node,
                BackgroundColor(color),
                ui::visible(block.fill > 0.0),
                ChildOf(bar.root),
            ))
            .id();
        bar.segments.push(fill);
        if block.shaded {
            // Keep the shading in place as health recedes across the group.
            let edge = Color::srgba(0.0, 0.0, 0.0, 0.8);
            let center = Color::srgba(0.0, 0.0, 0.0, 0.48);
            commands.spawn((
                ui::absolute(0.0, 0.0, block.width, h),
                BackgroundGradient::from(LinearGradient::to_right(vec![
                    edge.into(),
                    center.into(),
                    edge.into(),
                ])),
                ChildOf(fill),
            ));
        }
    }
}

/// Lays the marks of a bar `w` wide out for its unit's maximum, spawning
/// what it lacks; only when the maximum or the width changed.
fn lay_ticks(commands: &mut Commands, bar: &mut Bar, max_hp: i32, (w, h): (f32, f32)) {
    let key = (max_hp, w.to_bits());
    if bar.laid_for == key || !bar.segments.is_empty() {
        return;
    }
    bar.laid_for = key;
    let marks = tick_marks(max_hp, w);
    while bar.ticks.len() < marks.len() {
        let tick = commands
            .spawn((
                ui::absolute(0.0, 0.0, 1.0, h),
                BackgroundColor(TICK),
                ChildOf(bar.root),
            ))
            .id();
        bar.ticks.push(tick);
    }
    for (at, tick) in bar.ticks.iter().enumerate() {
        let mark = marks.get(at).copied();
        let (x, big) = mark.unwrap_or((0.0, false));
        let (width, height, colour) = if big {
            (BIG_TICK_WIDTH, h, BIG_TICK)
        } else {
            (TICK_WIDTH, h * 0.6, TICK)
        };
        commands.entity(*tick).insert((
            ui::absolute(x - width / 2.0, 0.0, width, height),
            BackgroundColor(colour),
            ui::visible(mark.is_some()),
        ));
    }
}

/// Everything the bars read besides the core.
#[derive(bevy::ecs::system::SystemParam)]
struct BarParams<'w, 's> {
    commands: Commands<'w, 's>,
    bars: ResMut<'w, Bars>,
    layer: Option<Res<'w, Layer>>,
    fonts: Res<'w, Fonts>,
    ground: Res<'w, Ground>,
    fog: Res<'w, crate::fog::FogGrid>,
    ui_scale: Res<'w, UiScale>,
    camera: Single<'w, 's, (Entity, &'static Camera), With<crate::camera::RtsCamera>>,
    transforms: TransformHelper<'w, 's>,
}

fn bars(
    session: NonSend<Session>,
    mut p: BarParams,
    units: Query<(Entity, &Unit, Has<Dying>)>,
    mut nodes: Query<(&mut Node, &mut Visibility, Option<&mut BackgroundColor>)>,
    mut texts: Query<&mut Text>,
    mut versions: Query<&mut crate::player_name::VersionRun>,
) {
    let core = &session.0;
    let (Some(view), Some(layer)) = (core.view(), &p.layer) else {
        return;
    };
    let melee_team = melee_viewer_team(core, view);
    let layer = layer.0;
    let (camera_entity, camera) = *p.camera;
    let Ok(cam_gt) = p.transforms.compute_global_transform(camera_entity) else {
        return;
    };
    let cam_gt = &cam_gt;
    let scale = p.ui_scale.0.max(0.1);
    // The screen in interface pixels, as the bars' corners are.
    let viewport = camera
        .logical_viewport_rect()
        .map(|r| Rect::from_corners(r.min / scale, r.max / scale));
    let mut alive = Vec::new();
    for (entity, unit, dying) in &units {
        let Some((w, h)) = bar_size(unit.kind) else {
            continue;
        };
        let Some(live) = view.unit(unit.id).filter(|_| !dying) else {
            continue;
        };
        // A neutral the fog hides is drawn dulled, with no bar over it.
        if p.fog.hides(live.seen_by) {
            continue;
        }
        let head = Vec3::new(
            unit.at.x,
            p.ground.at(unit.at.x, unit.at.y)
                + crate::units::flight_height(unit)
                + bar_height(unit.kind),
            -unit.at.y,
        );
        let Ok(screen) = camera.world_to_viewport(cam_gt, head) else {
            continue;
        };
        alive.push(entity);
        let Some(bar) = p.bars.0.get_mut(&entity) else {
            let bar = spawn_bar(&mut p.commands, layer, &p.fonts, live);
            p.bars.0.insert(entity, bar);
            continue;
        };
        // A bar off the screen is out of the layout and not drawn: every bar
        // moves on the screen as the camera does, and laying out and drawing the
        // ones nobody sees was most of what the bars cost.
        let corner = Vec2::new(screen.x / scale - w / 2.0, screen.y / scale - h);
        let on_screen = viewport.is_none_or(|room| {
            let reach = Vec2::new(w + BAR_REACH, h + BAR_REACH);
            !Rect::from_corners(corner - reach, corner + reach)
                .intersect(room)
                .is_empty()
        });
        if !on_screen {
            set(
                &mut nodes,
                bar.root,
                |n| n.display = Display::None,
                Some(false),
                None,
            );
            continue;
        }
        let color = bar_color(core, live);
        lay_segments(
            &mut p.commands,
            bar,
            live,
            (w, h),
            hit_blocks(live, melee_team),
            color,
        );
        lay_ticks(&mut p.commands, bar, live.max_hp, (w, h));
        fill_bar(
            core,
            view,
            live,
            bar,
            corner,
            w,
            color,
            &mut nodes,
            &mut texts,
            &mut versions,
        );
    }
    alive.sort_unstable();
    p.bars.0.retain(|entity, bar| {
        let keep = alive.binary_search(entity).is_ok();
        if !keep {
            p.commands.entity(bar.root).despawn();
        }
        keep
    });
}

/// Puts a bar's top-left corner at `corner`, in interface pixels, and fills
/// it from its unit, `w` wide.
fn fill_bar(
    core: &bota_client_core::App,
    view: &WorldView,
    live: &UnitView,
    bar: &Bar,
    corner: Vec2,
    w: f32,
    color: Color,
    nodes: &mut Query<(&mut Node, &mut Visibility, Option<&mut BackgroundColor>)>,
    texts: &mut Query<&mut Text>,
    versions: &mut Query<&mut crate::player_name::VersionRun>,
) {
    let place = |node: &mut Node| {
        node.left = px(corner.x);
        node.top = px(corner.y);
        node.display = Display::Flex;
    };
    set(nodes, bar.root, |n| place(n), Some(true), None);
    let max = live.max_hp.max(1) as f32;
    let share = live.hp.max(0) as f32 / max;
    let ghost = core.derived.ghost(live.id, live.hp) / max;
    let blocks = !bar.segments.is_empty();
    set(
        nodes,
        bar.fill,
        |n| n.width = px(w * share),
        Some(!blocks),
        Some(color),
    );
    set(
        nodes,
        bar.ghost,
        |n| n.width = px(w * ghost.min(1.0)),
        Some(!blocks),
        None,
    );
    if let Some(layout) = bar.hit_blocks {
        for (segment, block) in bar.segments.iter().zip(layout.layout(live, w)) {
            set(
                nodes,
                *segment,
                |n| n.width = px(block.width * block.fill),
                Some(block.fill > 0.0),
                Some(color),
            );
        }
    }
    if let Some(mana) = bar.mana {
        let m = live.mana.max(0) as f32 / live.max_mana.max(1) as f32;
        set(nodes, mana, |n| n.width = px(w * m), None, None);
    }
    if let Some((well, fill)) = bar.channel {
        let share = crate::teleport::channel_share(view, live.id);
        set(nodes, well, |_| {}, Some(share.is_some()), None);
        set(
            nodes,
            fill,
            |n| n.width = px(w * share.unwrap_or(0.0)),
            None,
            None,
        );
    }
    if let Some(level) = bar.level
        && let Ok(mut t) = texts.get_mut(level)
    {
        let s = live.level.to_string();
        t.set_if_neq(Text(s));
    }
    if let Some(name) = bar.name
        && let Ok(mut t) = texts.get_mut(name.name)
    {
        // The player's name only: the hero is plain to see.
        let s = live
            .owner
            .and_then(|o| core.player_name(o))
            .unwrap_or_default()
            .to_string();
        t.set_if_neq(Text(s));
    }
    if let Some(name) = bar.name
        && let Ok(mut version) = versions.get_mut(name.version)
        && let Ok(mut text) = texts.get_mut(name.version)
    {
        version.set(
            live.owner.and_then(|slot| core.player_version(slot)),
            &mut text,
        );
    }
}

/// Changes one node: its layout, whether it shows, its colour.
fn set(
    nodes: &mut Query<(&mut Node, &mut Visibility, Option<&mut BackgroundColor>)>,
    entity: Entity,
    layout: impl FnOnce(&mut Node),
    show: Option<bool>,
    color: Option<Color>,
) {
    let Ok((mut node, mut visibility, background)) = nodes.get_mut(entity) else {
        return;
    };
    // Written only when it moved: a changed node is laid out again, and
    // every bar's every part each frame was most of the interface's cost.
    let mut laid = node.clone();
    layout(&mut laid);
    node.set_if_neq(laid);
    if let Some(show) = show {
        visibility.set_if_neq(ui::visible(show));
    }
    if let (Some(color), Some(mut bg)) = (color, background)
        && bg.0 != color
    {
        bg.0 = color;
    }
}

/// The colour of a unit's health by its side, the same whoever watches:
/// Radiant green, Dire and the neutrals red; one's own hero's brighter.
pub fn bar_color(core: &bota_client_core::App, u: &UnitView) -> Color {
    let own = Some(u.id) == core.my_hero();
    match (u.team, own) {
        (Team::Radiant, true) => ui::HP_MINE,
        (Team::Radiant, false) => ui::HP_FRIEND,
        (_, true) => HP_DIRE_OWN,
        (_, false) => ui::HP_FOE,
    }
}

/// Puts the client's floaters on the pool of texts.
fn floaters(
    session: NonSend<Session>,
    pool: Res<Floaters>,
    marks: Res<Marks>,
    ground: Res<Ground>,
    settings: Res<Settings>,
    ui_scale: Res<UiScale>,
    camera: Single<(Entity, &Camera), With<crate::camera::RtsCamera>>,
    transforms: TransformHelper,
    mut texts: Query<(
        &mut Text,
        &mut TextFont,
        &mut TextColor,
        &mut Node,
        &mut Visibility,
        &mut UiTransform,
        &mut FloaterText,
    )>,
    mut bolts: Query<
        (&mut Node, &mut Visibility, &mut ImageNode, &mut UiTransform),
        (With<Mark>, Without<FloaterText>),
    >,
    index: Res<crate::units::UnitIndex>,
    units: Query<&crate::units::Unit>,
    fog: Res<crate::fog::FogGrid>,
    // Kept between frames so that a frame allocates nothing: the floaters drawn (as places in the
    // core's list, newest first), how far each stands aside, and the words being compared.
    mut shown: Local<Vec<usize>>,
    mut sideways: Local<Vec<f32>>,
    mut words: Local<String>,
) {
    let core = &session.0;
    let (camera_entity, camera) = *camera;
    let Ok(cam_gt) = transforms.compute_global_transform(camera_entity) else {
        return;
    };
    let cam_gt = &cam_gt;
    let scale = ui_scale.0.max(0.1);
    let numbers = settings.aids.numbers;
    shown.clear();
    shown.extend(
        core.floaters
            .iter()
            .enumerate()
            // A level gained floats no words: Dota marks it with its glow and
            // sound, and the panel's level.
            .filter(|(_, f)| f.kind != FloaterKind::Level)
            // Damage numbers are an aid to turn off; the gold, the deny mark, the miss notice and
            // a critical strike are Dota's own overhead alerts (Dota shows its critical strike
            // alert whatever its damage numbers setting), always shown.
            .filter(|(_, f)| {
                numbers
                    || matches!(
                        f.kind,
                        FloaterKind::Gold
                            | FloaterKind::Deny
                            | FloaterKind::Miss
                            | FloaterKind::Crit
                    )
            })
            // Nothing over what the watched side does not see.
            .filter(|(_, f)| !fog.on || fog.lit(f.world.0, f.world.1))
            .map(|(place, _)| place)
            .rev()
            .take(FLOATERS),
    );
    lanes(&core.floaters, &shown, &mut sideways);
    for (i, entity) in pool.0.iter().enumerate() {
        let Ok((
            mut text,
            mut font,
            mut color,
            mut node,
            mut visibility,
            mut transform,
            mut caption,
        )) = texts.get_mut(*entity)
        else {
            continue;
        };
        let Some(f) = shown.get(i).map(|place| &core.floaters[*place]) else {
            caption.0 = None;
            visibility.set_if_neq(Visibility::Hidden);
            hide_mark(&marks, i, &mut bolts);
            continue;
        };
        caption.0 = Some(f.kind);
        // Floaters set off together from one place stack upwards.
        let stacked = shown[..i]
            .iter()
            .map(|place| &core.floaters[*place])
            .filter(|g| {
                (g.age - f.age).abs() < 0.6
                    && Vec2::from(g.world).distance(Vec2::from(f.world)) < 120.0
            })
            .count();
        let k = (f.age / f.kind.life()).clamp(0.0, 1.0);
        let (size, tint) = look(f.kind, f.amount);
        let pop = 1.0 + 0.35 * (1.0 - (k * 6.0).min(1.0));
        let rise = (1.0 - (1.0 - k) * (1.0 - k)) * 110.0;
        // Over its unit while the unit is drawn, else where it was.
        let body = index.0.get(&f.unit).and_then(|e| units.get(*e).ok());
        let (x, y) = body.map_or(f.world, |u| (u.at.x, u.at.y));
        // Above the health bars, which float about 260 over a hero and
        // lower over everything else.
        let over = match body.map(|u| u.kind) {
            Some(UnitKind::Hero) | None => 300.0,
            Some(_) => 220.0,
        };
        let at = Vec3::new(x, ground.at(x, y) + over + rise, -y);
        let Ok(screen) = camera.world_to_viewport(cam_gt, at) else {
            *visibility = Visibility::Hidden;
            hide_mark(&marks, i, &mut bolts);
            continue;
        };
        let fade = (1.0 - (k - 0.6).max(0.0) / 0.4).clamp(0.0, 1.0);
        words.clear();
        write_words(f, &mut words);
        if text.0 != *words {
            text.0.clone_from(&words);
        }
        // The pop scales the laid-out text: a font size that changed every
        // frame had every glyph drawn again at each new size, into new atlases.
        font.set_if_neq(TextFont {
            font_size: FontSize::Px(size),
            ..font.clone()
        });
        transform.scale = Vec2::splat(pop);
        color.0 = tint.with_alpha(fade);
        let width = size * 0.56 * words.chars().count() as f32;
        node.left = px(screen.x / scale - width / 2.0 + sideways[i]);
        node.top = px(screen.y / scale - size - stacked as f32 * (size + 2.0));
        *visibility = Visibility::Inherited;
        let beside = (f.kind == FloaterKind::Crit).then(|| {
            // The mark stands right of the digits, as tall as they are.
            let height = size * 1.4;
            let left = screen.x / scale - width / 2.0 + sideways[i] + width + 2.0;
            (
                left,
                screen.y / scale - size * 1.25 - stacked as f32 * (size + 2.0),
                height,
            )
        });
        if let Some(mark) = marks.0.get(i)
            && let Ok((mut at, mut shown, mut image, mut turn)) = bolts.get_mut(*mark)
        {
            match beside {
                Some((left, top, height)) => {
                    at.left = px(left);
                    at.top = px(top);
                    at.height = px(height);
                    at.width = px(height * crate::bolt::WIDTH as f32 / crate::bolt::HEIGHT as f32);
                    image.color = tint.with_alpha(fade);
                    turn.scale = Vec2::splat(pop);
                    *shown = Visibility::Inherited;
                }
                None => {
                    shown.set_if_neq(Visibility::Hidden);
                }
            }
        }
    }
}

/// Hides the mark beside pool text `at`.
fn hide_mark(
    marks: &Marks,
    at: usize,
    bolts: &mut Query<
        (&mut Node, &mut Visibility, &mut ImageNode, &mut UiTransform),
        (With<Mark>, Without<FloaterText>),
    >,
) {
    if let Some(mark) = marks.0.get(at)
        && let Ok((_, mut shown, ..)) = bolts.get_mut(*mark)
    {
        shown.set_if_neq(Visibility::Hidden);
    }
}

/// Writes what a floater says: the number, with a plus for what is gained, or a word.
fn write_words(floater: &Floater, into: &mut String) {
    use std::fmt::Write;
    let amount = floater.amount;
    let _ = match floater.kind {
        FloaterKind::Damage | FloaterKind::Magic | FloaterKind::Pure | FloaterKind::Crit => {
            write!(into, "{amount}")
        }
        FloaterKind::Miss => write!(into, "miss"),
        FloaterKind::Heal | FloaterKind::Gold => write!(into, "+{amount}"),
        FloaterKind::Level => write!(into, "level {amount}"),
        FloaterKind::Deny => write!(into, "!"),
    };
}

/// The font size of a damage number of a small sum.
pub(crate) const DAMAGE_SIZE: f32 = 15.0;
/// The sum a damage number starts to grow at, and the one it stops growing at (a sum of this
/// many times the first is as large as it gets).
const GROWTH_FROM: f32 = 10.0;
const GROWTH_SPAN_DOUBLINGS: f32 = 5.0;
/// How much larger the largest damage number is than the smallest.
pub(crate) const GROWTH_MAX: f32 = 1.5;
/// The miss notice's: at its largest Dota draws it 25/16 the size of a damage digit.
const MISS_SIZE: f32 = DAMAGE_SIZE * 25.0 / 16.0;
/// Dota's yellow of a message particle (`msg_damage`'s constant colour), the colour of pure
/// damage.
const PURE_YELLOW: Color = Color::srgb_u8(255, 200, 33);

/// How much larger than a small one a damage number of `amount` is drawn: a sum that grew in a
/// window reads larger, a little for each doubling, and never more than [`GROWTH_MAX`].
fn growth(amount: i32) -> f32 {
    let doublings = (amount.max(1) as f32 / GROWTH_FROM).log2();
    1.0 + (GROWTH_MAX - 1.0) * (doublings / GROWTH_SPAN_DOUBLINGS).clamp(0.0, 1.0)
}

/// The size and colour of a floater by what it tells of; a damage number by its sum too.
fn look(kind: FloaterKind, amount: i32) -> (f32, Color) {
    match kind {
        // Dota's colours of damage by its type: physical white, magical the purple, pure yellow.
        FloaterKind::Damage => (DAMAGE_SIZE * growth(amount), Color::WHITE),
        FloaterKind::Magic => (DAMAGE_SIZE * growth(amount), Color::srgb(0.72, 0.62, 1.0)),
        FloaterKind::Pure => (DAMAGE_SIZE * growth(amount), PURE_YELLOW),
        // Dota's colour for the miss notice, and as large beside a damage number as Dota draws
        // it at its largest: both established from the installed game (build 6975).
        FloaterKind::Miss => (MISS_SIZE, Color::srgb_u8(163, 38, 38)),
        FloaterKind::Heal => (16.0, Color::srgb(0.45, 1.0, 0.5)),
        FloaterKind::Level => (18.0, ui::GOLDEN),
        FloaterKind::Gold => (18.0, ui::GOLDEN),
        FloaterKind::Deny => (28.0, Color::srgb(0.6, 0.85, 1.0)),
        // Red, as Dota's alert for it is, and larger than a damage digit so that it reads at a
        // glance. The shade is ours (the alert's colour is set in the game's native code); the
        // installed particle pops to 2.25 where a damage digit's pops to 2.
        FloaterKind::Crit => (DAMAGE_SIZE * 1.5, Color::srgb_u8(255, 48, 48)),
    }
}

/// Interface pixels between damage numbers of two types that run side by side.
const LANE_GAP: f32 = 8.0;

/// The lanes damage numbers run in, from the left: a number of a damage type stands in the lane
/// of its type. A new damage type that sums does not compile until it has one.
const LANES: usize = 3;

/// Which lane a type of damage runs in; none for what does not sum.
fn lane_of(kind: FloaterKind) -> Option<usize> {
    match kind {
        FloaterKind::Damage => Some(0),
        FloaterKind::Magic => Some(1),
        FloaterKind::Pure => Some(2),
        FloaterKind::Crit
        | FloaterKind::Miss
        | FloaterKind::Heal
        | FloaterKind::Level
        | FloaterKind::Gold
        | FloaterKind::Deny => None,
    }
}

/// How wide a summed damage number is drawn, in interface pixels.
fn damage_width(floater: &Floater) -> f32 {
    let (size, _) = look(floater.kind, floater.amount);
    let digits = floater.amount.max(1).ilog10() + 1;
    size * 0.56 * digits as f32
}

/// How far each of the floaters at `shown` (places in `floaters`) moves sideways, in interface
/// pixels, written to `into` one for one: so that the damage numbers of different types over one
/// unit (a physical blow during a magical burn) stand side by side, each in its own colour,
/// instead of one on the other. Numbers of one type (the sum and the one before it that finishes
/// its climb) share their lane, and anything else floats where it always did. Allocates nothing
/// once `into` has grown to the most that are shown; the work is the square of how many are, at
/// most [`FLOATERS`] of them.
fn lanes(floaters: &[Floater], shown: &[usize], into: &mut Vec<f32>) {
    into.clear();
    for place in shown {
        let f = &floaters[*place];
        let Some(own) = lane_of(f.kind) else {
            into.push(0.0);
            continue;
        };
        // The widest number of each lane over this unit.
        let mut wide = [0.0_f32; LANES];
        for other in shown.iter().map(|place| &floaters[*place]) {
            if let Some(lane) = lane_of(other.kind).filter(|_| other.unit == f.unit) {
                wide[lane] = wide[lane].max(damage_width(other));
            }
        }
        let used = wide.iter().filter(|w| **w > 0.0).count();
        let total: f32 = wide.iter().sum::<f32>() + LANE_GAP * (used - 1) as f32;
        let mut left = -total / 2.0;
        let mut aside = 0.0;
        for (lane, width) in wide.iter().enumerate().filter(|(_, w)| **w > 0.0) {
            if lane == own {
                aside = left + width / 2.0;
            }
            left += width + LANE_GAP;
        }
        into.push(aside);
    }
}

/// A floating caption as drawn: what it tells of, its words, its font size and colour.
#[cfg(test)]
pub(crate) struct DrawnFloater {
    pub entity: Entity,
    pub kind: FloaterKind,
    pub words: String,
    pub size: f32,
    pub color: Color,
}

/// The floating captions drawn this frame.
#[cfg(test)]
pub(crate) fn drawn_floaters(world: &mut World) -> Vec<DrawnFloater> {
    world
        .query::<(
            Entity,
            &FloaterText,
            &Text,
            &TextFont,
            &TextColor,
            &Visibility,
            &InheritedVisibility,
        )>()
        .iter(world)
        .filter(|(_, _, _, _, _, own, inherited)| **own != Visibility::Hidden && inherited.get())
        .filter_map(|(entity, caption, text, font, color, ..)| {
            let FontSize::Px(size) = font.font_size else {
                return None;
            };
            Some(DrawnFloater {
                entity,
                kind: caption.0?,
                words: text.0.clone(),
                size,
                color: color.0,
            })
        })
        .collect()
}

/// The lightning marks drawn this frame.
#[cfg(test)]
pub(crate) fn drawn_marks(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<(Entity, &Visibility, &InheritedVisibility), With<Mark>>()
        .iter(world)
        .filter(|(_, own, inherited)| **own != Visibility::Hidden && inherited.get())
        .map(|(entity, ..)| entity)
        .collect()
}

#[cfg(test)]
pub(crate) fn drawn_gold_popups(world: &mut World) -> Vec<i32> {
    let mut amounts: Vec<_> = world
        .query::<(&FloaterText, &Text, &Visibility, &InheritedVisibility)>()
        .iter(world)
        .filter(|(caption, _, own, inherited)| {
            caption.0 == Some(FloaterKind::Gold) && **own != Visibility::Hidden && inherited.get()
        })
        .map(|(_, text, ..)| {
            text.0
                .strip_prefix('+')
                .expect("a gold caption has a plus")
                .parse::<i32>()
                .expect("a gold caption is an integer")
        })
        .collect();
    amounts.sort_unstable();
    amounts
}

fn reset(mut commands: Commands, mut bars: ResMut<Bars>) {
    for (_, bar) in bars.0.drain() {
        commands.entity(bar.root).despawn();
    }
}
