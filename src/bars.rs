//! Health bars over units, Dota's way: screen-space bars that keep their
//! size, the health just lost trailing off, notches every 250 on heroes,
//! level and name. Also the numbers and words that rise off units.

use std::collections::HashMap;

use bevy::prelude::*;
use bota_client_core::{FLOATER_LIFE, FloaterKind};
use bota_proto::{UnitKind, UnitView};

use crate::config::Settings;
use crate::map::Ground;
use crate::session::Session;
use crate::ui::{self, Fonts};
use crate::units::{Dying, Unit};

/// Floating texts shown at once at most.
const FLOATERS: usize = 48;

/// One unit's bar and its parts.
#[derive(Clone, Copy)]
pub struct Bar {
    root: Entity,
    ghost: Entity,
    fill: Entity,
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
        .add_systems(
            PostUpdate,
            (bars, floaters).after(TransformSystems::Propagate),
        );
}

fn spawn_layer(mut commands: Commands, fonts: Res<Fonts>, mut pool: ResMut<Floaters>) {
    let layer = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                ..default()
            },
            Pickable::IGNORE,
            GlobalZIndex(-10),
        ))
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

/// Width and height of a unit's bar in interface pixels.
fn bar_size(kind: UnitKind) -> Option<(f32, f32)> {
    match kind {
        UnitKind::Hero => Some((92.0, 9.0)),
        UnitKind::Tower | UnitKind::Barracks | UnitKind::Ancient => Some((120.0, 8.0)),
        UnitKind::Filler => Some((90.0, 7.0)),
        UnitKind::Fountain | UnitKind::Ward => None,
        UnitKind::Courier => Some((40.0, 5.0)),
        _ => Some((54.0, 6.0)),
    }
}

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

fn spawn_bar(commands: &mut Commands, layer: Entity, fonts: &Fonts, kind: UnitKind) -> Bar {
    let (w, h) = bar_size(kind).unwrap_or((40.0, 5.0));
    let root = commands
        .spawn((
            ui::absolute(0.0, 0.0, w, h),
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
            Outline::new(px(1.0), px(0.0), Color::srgba(0.0, 0.0, 0.0, 0.9)),
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
                ui::absolute(0.0, h + 1.0, w, 4.0),
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
                ChildOf(root),
            ))
            .id();
        commands
            .spawn((
                ui::absolute(0.0, 0.0, w, 4.0),
                BackgroundColor(ui::MANA),
                ChildOf(well),
            ))
            .id()
    });
    let channel = hero.then(|| {
        let well = commands
            .spawn((
                ui::absolute(0.0, h + 6.0, w, 4.0),
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
                    left: px(-20.0),
                    top: px(-3.0),
                    width: px(18.0),
                    height: px(h + 9.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
                ChildOf(root),
            ))
            .id();
        commands
            .spawn((ui::text("1", &fonts.bold, 12.0, ui::GOLDEN), ChildOf(plate)))
            .id()
    });
    let name = hero.then(|| {
        commands
            .spawn((
                ui::text("", &fonts.bold, 12.0, ui::INK),
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
        mana,
        level,
        name,
        channel,
        threat,
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
    ui_scale: Res<'w, UiScale>,
    camera: Single<'w, 's, (&'static Camera, &'static GlobalTransform)>,
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
    let (camera, cam_gt) = *p.camera;
    let scale = p.ui_scale.0.max(0.1);
    let me = core
        .ears()
        .and_then(|id| view.units.iter().find(|u| u.id == id));
    let mut alive = Vec::new();
    for (entity, unit, dying) in &units {
        let Some((w, h)) = bar_size(unit.kind) else {
            continue;
        };
        let Some(live) = view
            .units
            .iter()
            .find(|u| u.id == unit.id)
            .filter(|_| !dying)
        else {
            continue;
        };
        let head = Vec3::new(
            unit.at.x,
            p.ground.at(unit.at.x, unit.at.y) + bar_height(unit.kind),
            -unit.at.y,
        );
        let Ok(screen) = camera.world_to_viewport(cam_gt, head) else {
            continue;
        };
        alive.push(entity);
        let bar = match p.bars.0.get(&entity) {
            Some(bar) => *bar,
            None => {
                let bar = spawn_bar(&mut p.commands, layer, &p.fonts, unit.kind);
                p.bars.0.insert(entity, bar);
                continue;
            }
        };
        let place = |node: &mut Node| {
            node.left = px(screen.x / scale - w / 2.0);
            node.top = px(screen.y / scale - h);
        };
        set(&mut nodes, bar.root, |n| place(n), Some(true), None);
        let max = live.max_hp.max(1) as f32;
        let share = live.hp.max(0) as f32 / max;
        let ghost = core.derived.ghost(live.id, live.hp) / max;
        let color = bar_color(core, live);
        set(
            &mut nodes,
            bar.fill,
            |n| n.width = px(w * share),
            None,
            Some(color),
        );
        set(
            &mut nodes,
            bar.ghost,
            |n| n.width = px(w * ghost.min(1.0)),
            None,
            None,
        );
        if let Some(mana) = bar.mana {
            let m = live.mana.max(0) as f32 / live.max_mana.max(1) as f32;
            set(&mut nodes, mana, |n| n.width = px(w * m), None, None);
        }
        if let Some((well, fill)) = bar.channel {
            let share = crate::teleport::channel_share(view, live.id);
            set(&mut nodes, well, |_| {}, Some(share.is_some()), None);
            set(
                &mut nodes,
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
            if t.0 != s {
                t.0 = s;
            }
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
            if t.0 != s {
                t.0 = s;
            }
        }
        let threat = me.is_some_and(|me| {
            live.kind != UnitKind::Hero
                && live.team != me.team
                && crate::overlay::targets(core, live.id, me.id)
        });
        set(&mut nodes, bar.threat, |_| {}, Some(threat), None);
    }
    let gone: Vec<Entity> = p
        .bars
        .0
        .keys()
        .filter(|e| !alive.contains(e))
        .copied()
        .collect();
    for entity in gone {
        if let Some(bar) = p.bars.0.remove(&entity) {
            p.commands.entity(bar.root).despawn();
        }
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
        let want = if show {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visibility != want {
            *visibility = want;
        }
    }
    if let (Some(color), Some(mut bg)) = (color, background)
        && bg.0 != color
    {
        bg.0 = color;
    }
}

/// The colour of a unit's health: one's own hero brightest, friends green,
/// foes red; a spectator sees Radiant green and Dire red.
fn bar_color(core: &bota_client_core::App, u: &UnitView) -> Color {
    if Some(u.id) == core.my_hero() {
        return ui::HP_MINE;
    }
    match core.fog_team().or(core.my_team()) {
        Some(side) if side == u.team => ui::HP_FRIEND,
        Some(_) => ui::HP_FOE,
        None => crate::overlay::team_color(u.team),
    }
}

/// Puts the client's floaters on the pool of texts.
fn floaters(
    session: NonSend<Session>,
    pool: Res<Floaters>,
    ground: Res<Ground>,
    settings: Res<Settings>,
    ui_scale: Res<UiScale>,
    camera: Single<(&Camera, &GlobalTransform)>,
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
    let (camera, cam_gt) = *camera;
    let scale = ui_scale.0.max(0.1);
    let numbers = settings.aids.numbers;
    let shown: Vec<_> = core
        .floaters
        .iter()
        .filter(|f| {
            numbers
                || matches!(
                    f.kind,
                    FloaterKind::Gold | FloaterKind::Deny | FloaterKind::Level
                )
        })
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
            if *visibility != Visibility::Hidden {
                *visibility = Visibility::Hidden;
            }
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
        if text.0 != f.text {
            text.0.clone_from(&f.text);
        }
        // The pop scales the laid-out text: a font size that changed every
        // frame had every glyph drawn again at each new size, into new atlases.
        font.set_if_neq(TextFont {
            font_size: FontSize::Px(size),
            ..font.clone()
        });
        transform.scale = Vec2::splat(pop);
        color.0 = tint.with_alpha(fade);
        let width = size * 0.56 * f.text.chars().count() as f32;
        node.left = px(screen.x / scale - width / 2.0);
        node.top = px(screen.y / scale - size - stacked as f32 * (size + 2.0));
        *visibility = Visibility::Inherited;
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
