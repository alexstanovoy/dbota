//! Health bars over units, Dota's way: screen-space bars that keep their
//! size, the health just lost trailing off, a small mark every 250 health
//! and a big one every 1000 (`dota_health_per_vertical_marker`), level and
//! name; health counted in hits (a courier's, a ward's) as one block a hit.
//! Also the numbers and words that rise off units.

use std::collections::HashMap;

use bevy::prelude::*;
use bota_client_core::{FLOATER_LIFE, Floater, FloaterKind};
use bota_proto::{Team, UnitKind, UnitView};

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
    /// One block a hit, for health counted in hits; empty otherwise.
    segments: Vec<Entity>,
    mana: Option<Entity>,
    level: Option<Entity>,
    name: Option<Entity>,
    /// The well and the fill of a hero's teleport channel.
    channel: Option<(Entity, Entity)>,
    threat: Entity,
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

/// A unit's health counted in hits, as Dota draws it: the health one block
/// stands for (the smallest blow's share) and how many blocks there are.
/// None for health that damage takes.
pub fn hit_blocks(kind: UnitKind, max_hp: i32) -> Option<(i32, usize)> {
    let count = match kind {
        UnitKind::Courier => bota_core::COURIER.hit_count,
        UnitKind::Ward => bota_core::OBSERVER_WARD.hit_count,
        _ => None,
    }?;
    let block = count
        .melee_hero
        .min(count.ranged_hero)
        .min(count.other)
        .max(1);
    let blocks = usize::try_from((max_hp.max(1) as u32).div_ceil(block as u32)).ok()?;
    Some((block, blocks.min(MAX_SEGMENTS)))
}

/// The pool of floating texts.
#[derive(Resource, Default)]
struct Floaters(Vec<Entity>);

/// The layer the bars and floaters sit in, under the rest of the interface.
#[derive(Resource)]
struct Layer(Entity);

pub fn plugin(app: &mut App) {
    app.init_resource::<Bars>()
        .init_resource::<Floaters>()
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

/// The outline of a bar: dark, and light while its unit is under the
/// cursor, as Dota lights a hovered unit's bar
/// (`dota_hud_healthbar_hoveroutline_alpha`).
const OUTLINE: Color = Color::srgba(0.0, 0.0, 0.0, 0.9);
const OUTLINE_HOVERED: Color = Color::srgba(1.0, 1.0, 1.0, 0.9);

/// Lights the bar of the unit a click would get, and dims the one lit
/// before.
fn hover_outline(
    hover: Res<crate::pick::Hover>,
    index: Res<crate::units::UnitIndex>,
    bars: Res<Bars>,
    mut outlines: Query<&mut Outline>,
    mut lit: Local<Option<Entity>>,
) {
    let now = hover
        .left()
        .and_then(|(id, _)| index.0.get(&id))
        .and_then(|unit| bars.0.get(unit))
        .map(|bar| bar.root);
    if now == *lit {
        return;
    }
    for (root, color) in [(*lit, OUTLINE), (now, OUTLINE_HOVERED)] {
        if let Some(mut outline) = root.and_then(|r| outlines.get_mut(r).ok()) {
            outline.color = color;
        }
    }
    *lit = now;
}

fn spawn_layer(mut commands: Commands, fonts: Res<Fonts>, mut pool: ResMut<Floaters>) {
    let layer = commands
        .spawn((ui::fill(), Pickable::IGNORE, GlobalZIndex(-10)))
        .id();
    for _ in 0..FLOATERS {
        let text = commands
            .spawn((
                ui::text("", &fonts.bold, 16.0, Color::WHITE),
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
        UnitKind::Fountain | UnitKind::Outpost | UnitKind::Watcher | UnitKind::Shrine => None,
        UnitKind::Courier => dota(50.0, 6.0),
        UnitKind::Ward => dota(40.0, 5.0),
        _ => dota(55.0, 5.0),
    }
}

/// The height of a hero's mana bar under its health.
const MANA_HEIGHT: f32 = 4.0 * ui::DOTA;
/// Interface pixels around a bar that its parts (the threat mark above, the
/// mana and channel wells below) may reach: a bar this near the screen is laid
/// out and drawn.
const BAR_REACH: f32 = 40.0;

/// How high over its feet a unit's bar floats, in world units.
fn bar_height(kind: UnitKind) -> f32 {
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
    let root = commands
        .spawn((
            ui::absolute(0.0, 0.0, w, h),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
            Outline::new(px(1.0), px(0.0), OUTLINE),
            Visibility::Hidden,
            ChildOf(layer),
        ))
        .id();
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
    let segments = spawn_segments(commands, root, live, (w, h));
    let threat = commands
        .spawn((
            ui::absolute(w / 2.0 - 5.0, -12.0, 10.0, 8.0),
            BackgroundColor(Color::srgb(1.0, 0.25, 0.2)),
            Visibility::Hidden,
            ChildOf(root),
        ))
        .id();
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
        commands
            .spawn((
                ui::text("", &fonts.bold, 10.0, ui::INK),
                ui::shadow(),
                TextLayout::no_wrap(),
                Node {
                    position_type: PositionType::Absolute,
                    bottom: px(h + 3.0),
                    left: px(0.0),
                    ..default()
                },
                ChildOf(root),
            ))
            .id()
    });
    Bar {
        root,
        ghost,
        fill,
        ticks: Vec::new(),
        laid_for: (0, 0),
        segments,
        mana,
        level,
        name,
        channel,
        threat,
    }
}

/// One block a hit for health counted in hits, `SEGMENT_GAP` apart; none
/// for health that damage takes.
fn spawn_segments(
    commands: &mut Commands,
    root: Entity,
    live: &UnitView,
    (w, h): (f32, f32),
) -> Vec<Entity> {
    let Some((_, blocks)) = hit_blocks(live.kind, live.max_hp) else {
        return Vec::new();
    };
    let gaps = SEGMENT_GAP * (blocks as f32 - 1.0);
    let each = ((w - gaps) / blocks as f32).max(1.0);
    (0..blocks)
        .map(|at| {
            let left = at as f32 * (each + SEGMENT_GAP);
            commands
                .spawn((
                    ui::absolute(left, 0.0, each, h),
                    BackgroundColor(ui::HP_FOE),
                    ChildOf(root),
                ))
                .id()
        })
        .collect()
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
) {
    let core = &session.0;
    let (Some(view), Some(layer)) = (core.view(), &p.layer) else {
        return;
    };
    let layer = layer.0;
    let (camera_entity, camera) = *p.camera;
    let Ok(cam_gt) = p.transforms.compute_global_transform(camera_entity) else {
        return;
    };
    let cam_gt = &cam_gt;
    let scale = p.ui_scale.0.max(0.1);
    let me = core.ears().and_then(|id| view.unit(id));
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
            p.ground.at(unit.at.x, unit.at.y) + bar_height(unit.kind),
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
        lay_ticks(&mut p.commands, bar, live.max_hp, (w, h));
        fill_bar(core, view, me, live, bar, corner, w, &mut nodes, &mut texts);
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
    view: &bota_proto::WorldView,
    me: Option<&UnitView>,
    live: &UnitView,
    bar: &Bar,
    corner: Vec2,
    w: f32,
    nodes: &mut Query<(&mut Node, &mut Visibility, Option<&mut BackgroundColor>)>,
    texts: &mut Query<&mut Text>,
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
    let color = bar_color(core, live);
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
    if let Some((block, _)) = hit_blocks(live.kind, live.max_hp) {
        // A block stands while any of its health is left.
        let standing = usize::try_from((live.hp.max(0) as u32).div_ceil(block as u32)).unwrap_or(0);
        for (at, segment) in bar.segments.iter().enumerate() {
            set(nodes, *segment, |_| {}, Some(at < standing), Some(color));
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
        && let Ok(mut t) = texts.get_mut(name)
    {
        // The player's name only: the hero is plain to see.
        let s = live
            .owner
            .and_then(|o| core.player_name(o))
            .unwrap_or_default()
            .to_string();
        t.set_if_neq(Text(s));
    }
    let threat = me.is_some_and(|me| {
        live.kind != UnitKind::Hero
            && live.team != me.team
            && crate::overlay::targets(core, live.id, me.id)
    });
    set(nodes, bar.threat, |_| {}, Some(threat), None);
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
    )>,
    index: Res<crate::units::UnitIndex>,
    units: Query<&crate::units::Unit>,
    fog: Res<crate::fog::FogGrid>,
) {
    let core = &session.0;
    let (camera_entity, camera) = *camera;
    let Ok(cam_gt) = transforms.compute_global_transform(camera_entity) else {
        return;
    };
    let cam_gt = &cam_gt;
    let scale = ui_scale.0.max(0.1);
    let numbers = settings.aids.numbers;
    let shown: Vec<_> = core
        .floaters
        .iter()
        // A level gained floats no words: Dota marks it with its glow and
        // sound, and the panel's level.
        .filter(|f| f.kind != FloaterKind::Level)
        .filter(|f| numbers || matches!(f.kind, FloaterKind::Gold | FloaterKind::Deny))
        // Nothing over what the watched side does not see.
        .filter(|f| !fog.on || fog.lit(f.world.0, f.world.1))
        .rev()
        .take(FLOATERS)
        .collect();
    for (i, entity) in pool.0.iter().enumerate() {
        let Ok((mut text, mut font, mut color, mut node, mut visibility, mut transform)) =
            texts.get_mut(*entity)
        else {
            continue;
        };
        let Some(f) = shown.get(i) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        // Floaters set off together from one place stack upwards.
        let stacked = shown[..i]
            .iter()
            .filter(|g| {
                (g.age - f.age).abs() < 0.6
                    && Vec2::from(g.world).distance(Vec2::from(f.world)) < 120.0
            })
            .count();
        let k = (f.age / FLOATER_LIFE).clamp(0.0, 1.0);
        let (size, tint) = look(f.kind);
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
            continue;
        };
        let fade = (1.0 - (k - 0.6).max(0.0) / 0.4).clamp(0.0, 1.0);
        let words = floater_words(f);
        if text.0 != words {
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
        node.left = px(screen.x / scale - width / 2.0);
        node.top = px(screen.y / scale - size - stacked as f32 * (size + 2.0));
        *visibility = Visibility::Inherited;
    }
}

/// What a floater says: the number, with a plus for what is gained, or a word.
fn floater_words(floater: &Floater) -> String {
    let amount = floater.amount;
    match floater.kind {
        FloaterKind::Damage | FloaterKind::Magic => amount.to_string(),
        FloaterKind::Miss => "miss".to_string(),
        FloaterKind::Heal | FloaterKind::Gold => format!("+{amount}"),
        FloaterKind::Level => format!("level {amount}"),
        FloaterKind::Deny => "!".to_string(),
    }
}

/// The size and colour of a floater by what it tells of.
fn look(kind: FloaterKind) -> (f32, Color) {
    match kind {
        FloaterKind::Damage => (15.0, Color::srgb(1.0, 0.94, 0.84)),
        FloaterKind::Magic => (16.0, Color::srgb(0.72, 0.62, 1.0)),
        FloaterKind::Miss => (13.0, Color::srgb(0.75, 0.77, 0.82)),
        FloaterKind::Heal => (16.0, Color::srgb(0.45, 1.0, 0.5)),
        FloaterKind::Level => (18.0, ui::GOLDEN),
        FloaterKind::Gold => (18.0, ui::GOLDEN),
        FloaterKind::Deny => (28.0, Color::srgb(0.6, 0.85, 1.0)),
    }
}
