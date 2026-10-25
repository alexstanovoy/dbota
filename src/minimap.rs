//! The minimap: the map photographed once from straight above, the fog over
//! it, a mark for every unit shown and the camera's frame. A hero's mark is
//! Dota's arrow in its player's colour, turned the way the hero faces; with
//! Alt held (Dota's DotaAlt) it shows the hero's own minimap icon in a disc
//! of that colour instead. Creeps are Dota's dots, buildings its building
//! icons, tinted by side. A left click or a
//! drag moves the camera; a right click walks there.

use bevy::{
    camera::{ImageRenderTarget, RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::TextureFormat,
    window::PrimaryWindow,
};
use bota_client_core::Intent;
use bota_proto::{EffectId, Team, UnitKind, UnitView};

use crate::camera::RtsCamera;
use crate::fog::FogGrid;
use crate::hud::{Blocker, Docks};
use crate::map::Ground;
use crate::session::Session;

/// World units of the map along each axis.
const MAP_SIZE: f32 = bota_core::rules::MAP_SIZE as f32;
/// Interface pixels of the minimap's side.
pub const SIDE: f32 = 250.0;
/// Pixels of the photograph's side.
const PHOTO: u32 = 512;
/// Unit marks shown at most.
const MARKS: usize = 160;
/// Hero marks shown at most: Dota's ten seats.
const HERO_MARKS: usize = 10;
/// Interface pixels across a hero's mark, and across it with its icon.
const HERO_MARK: f32 = 15.0;
const HERO_ICON: f32 = 28.0;
/// Interface pixels across the face in a hero's icon.
const HERO_FACE: f32 = 20.0;
/// Rings shown at most: teleports first, then what the side's players have
/// pinged.
const PINGS: usize = 2 * 10 + crate::pings::MARKS;
/// Interface pixels across a ping at its smallest and at its widest.
const PING_SIZES: (f32, f32) = (10.0, 30.0);
/// Ticks one swell of a ping takes.
const PING_PERIOD: u32 = 30;
/// The map's objectives marked at most: Dota's two Outposts, ten Watchers
/// and two Shrines of Wisdom, with room to spare.
const OBJECTIVE_MARKS: usize = 24;
/// The tint of a Watcher not lit and of a Shrine not ready, so the lit and
/// the ready ones stand out.
const DORMANT: Color = Color::srgb(0.55, 0.55, 0.55);

/// The handles of the minimap's icons by their manifest name, looked up once:
/// a mark is set every frame, and the asset server is not asked each time.
#[derive(Default)]
struct IconHandles(std::collections::HashMap<String, Option<Handle<Image>>>);

impl IconHandles {
    fn get(
        &mut self,
        logical: &str,
        manifest: &crate::manifest::Manifest,
        server: &AssetServer,
    ) -> Option<Handle<Image>> {
        if let Some(found) = self.0.get(logical) {
            return found.clone();
        }
        let handle = manifest.file(logical).map(|p| server.load::<Image>(p));
        self.0.insert(logical.to_owned(), handle.clone());
        handle
    }
}

/// The minimap's frame, the photo, the marks and the camera frame.
#[derive(Resource)]
struct Minimap {
    frame: Entity,
    photo: Handle<Image>,
    fog: Entity,
    marks: Vec<Entity>,
    /// The layer of the map's objectives' icons, under the units' marks, and
    /// the icons made so far: a match without objectives has none.
    objectives: (Entity, Vec<Entity>),
    /// A hero's mark and the icon in it, one a seat.
    heroes: Vec<(Entity, Entity)>,
    pings: Vec<Entity>,
    view: Entity,
}

/// A hero's mark on the minimap: the side and seat of the hero it marks
/// now, none while it marks none.
#[derive(Component, Default)]
pub struct HeroMark(pub Option<(Team, bota_proto::SlotId)>);

/// The photograph on the minimap.
#[derive(Component)]
struct Photo;

/// The camera that takes the photograph, and the frames until it stops.
#[derive(Component)]
struct PhotoCamera(u8);

/// Whether the photograph is taken: the fog waits for it, so the map is
/// photographed unfogged.
#[derive(Resource, Default)]
pub struct PhotoTaken(pub bool);

pub fn plugin(app: &mut App) {
    app.init_resource::<PhotoTaken>()
        .add_systems(Startup, spawn)
        .add_systems(
            Update,
            (
                photograph,
                marks,
                objective_marks,
                hero_marks,
                pings,
                clicks,
            ),
        );
}

fn spawn(mut commands: Commands, mut images: ResMut<Assets<Image>>, docks: Res<Docks>) {
    // RGBA: WebGL2 takes no BGRA pixels, and a target starts as uploaded zeros.
    let photo = images.add(Image::new_target_texture(
        PHOTO,
        PHOTO,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    let frame = commands
        .spawn((
            Node {
                width: px(SIDE),
                height: px(SIDE),
                border: UiRect::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.11, 0.07)),
            BorderColor::all(Color::srgb(0.18, 0.2, 0.24)),
            Interaction::default(),
            Blocker,
            ChildOf(docks.minimap),
        ))
        .id();
    // Hidden until the photograph is taken: a target never rendered is black.
    commands.spawn((
        Photo,
        crate::ui::fill(),
        ImageNode::new(photo.clone()),
        Visibility::Hidden,
        Pickable::IGNORE,
        ChildOf(frame),
    ));
    let fog = commands
        .spawn((
            crate::ui::fill(),
            ImageNode::default(),
            Pickable::IGNORE,
            ChildOf(frame),
        ))
        .id();
    let objectives = commands
        .spawn((crate::ui::fill(), Pickable::IGNORE, ChildOf(frame)))
        .id();
    // A mark is Dota's icon tinted by the side; without the icon, a plain disc.
    let marks = (0..MARKS)
        .map(|_| {
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    ImageNode::default(),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ChildOf(frame),
                ))
                .id()
        })
        .collect();
    let heroes = (0..HERO_MARKS)
        .map(|_| {
            let mark = commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    HeroMark::default(),
                    ImageNode::default(),
                    UiTransform::default(),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ChildOf(frame),
                ))
                .id();
            let icon = commands
                .spawn((
                    Node {
                        width: px(HERO_FACE),
                        height: px(HERO_FACE),
                        ..default()
                    },
                    ImageNode::default(),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ChildOf(mark),
                ))
                .id();
            (mark, icon)
        })
        .collect();
    let pings = (0..PINGS)
        .map(|_| {
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ChildOf(frame),
                ))
                .id()
        })
        .collect();
    let view = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                border: UiRect::all(px(1)),
                ..default()
            },
            BorderColor::all(Color::WHITE),
            Pickable::IGNORE,
            ChildOf(frame),
        ))
        .id();
    commands.insert_resource(Minimap {
        frame,
        photo,
        fog,
        marks,
        objectives: (objectives, Vec::new()),
        heroes,
        pings,
        view,
    });
}

/// Photographs the map from straight above once the ground and the forest
/// are there, then puts the camera away.
fn photograph(
    mut commands: Commands,
    ready: Res<crate::trees::SceneryReady>,
    minimap: Res<Minimap>,
    mut cameras: Query<(Entity, &mut PhotoCamera, &mut Camera)>,
    mut shown: Single<&mut Visibility, With<Photo>>,
    mut started: Local<f32>,
    time: Res<Time>,
    mut taken: ResMut<PhotoTaken>,
) {
    if !ready.0 {
        return;
    }
    if *started == 0.0 {
        *started = time.elapsed_secs();
        return;
    }
    // Leave the forest a moment to appear.
    if time.elapsed_secs() - *started < 0.5 {
        return;
    }
    match cameras.single_mut() {
        Ok((entity, mut frames, mut camera)) => {
            if frames.0 == 0 {
                camera.is_active = false;
                commands.entity(entity).despawn();
                **shown = Visibility::Inherited;
                taken.0 = true;
            } else {
                frames.0 -= 1;
            }
        }
        Err(_) if *started > 0.0 && time.elapsed_secs() - *started < 1.0 => {
            commands.spawn((
                Camera3d::default(),
                Camera {
                    order: -1,
                    ..default()
                },
                RenderTarget::Image(ImageRenderTarget {
                    handle: minimap.photo.clone(),
                    scale_factor: 1.0,
                }),
                Projection::Orthographic(OrthographicProjection {
                    near: 1.0,
                    far: 12_000.0,
                    scaling_mode: ScalingMode::Fixed {
                        width: MAP_SIZE,
                        height: MAP_SIZE,
                    },
                    ..OrthographicProjection::default_3d()
                }),
                Transform::from_xyz(MAP_SIZE / 2.0, 6_000.0, -MAP_SIZE / 2.0)
                    .looking_at(Vec3::new(MAP_SIZE / 2.0, 0.0, -MAP_SIZE / 2.0), Vec3::NEG_Z),
                RenderLayers::layer(0),
                Msaa::Off,
                PhotoCamera(3),
            ));
        }
        Err(_) => {}
    }
}

/// Where a bota world point falls on the minimap, in interface pixels from
/// its inner top-left corner.
fn on_map(x: f32, y: f32) -> Vec2 {
    let inner = SIDE - 6.0;
    Vec2::new(x / MAP_SIZE * inner, (1.0 - y / MAP_SIZE) * inner)
}

/// Puts a mark on every unit shown, the fog over the photo, and the camera's
/// frame.
fn marks(
    session: NonSend<Session>,
    minimap: Res<Minimap>,
    grid: Res<FogGrid>,
    veil: Option<Res<crate::fog::VeilImage>>,
    camera: Single<(&Camera, &GlobalTransform), With<RtsCamera>>,
    window: Single<&Window, With<PrimaryWindow>>,
    ground: Res<Ground>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    mut nodes: Query<(&mut Node, &mut BackgroundColor, &mut Visibility)>,
    mut images: Query<&mut ImageNode>,
    mut handles: Local<IconHandles>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    if let (Some(veil), Ok(mut image)) = (veil, images.get_mut(minimap.fog))
        && image.image != veil.0
    {
        image.image = veil.0.clone();
    }
    let shown = view.units.iter().filter(|u| {
        !matches!(u.kind, UnitKind::Ward | UnitKind::Hero)
            && objective_icon(u).is_none()
            && !grid.hides(u.seen_by)
    });
    let mut used = 0;
    for u in shown.take(MARKS) {
        let mark = minimap.marks[used];
        let (Ok((mut node, _, mut vis)), Ok(mut image)) =
            (nodes.get_mut(mark), images.get_mut(mark))
        else {
            break;
        };
        used += 1;
        let (icon, size) = unit_icon(u);
        let icon = icon.and_then(|logical| handles.get(logical, &manifest, &server));
        let at = on_map(u.pos.x.to_f32(), u.pos.y.to_f32());
        // Written only when changed: a changed node is laid out again.
        node.set_if_neq(Node {
            left: px(at.x - size / 2.0),
            top: px(at.y - size / 2.0),
            width: px(size),
            height: px(size),
            border_radius: if icon.is_some() {
                BorderRadius::ZERO
            } else {
                BorderRadius::MAX
            },
            ..node.clone()
        });
        let handle = icon.unwrap_or_default();
        let tint = crate::overlay::team_color(u.team);
        if image.image != handle || image.color != tint {
            image.image = handle;
            image.color = tint;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
    for mark in &minimap.marks[used..] {
        if let Ok((_, _, mut vis)) = nodes.get_mut(*mark)
            && *vis != Visibility::Hidden
        {
            *vis = Visibility::Hidden;
        }
    }
    let (camera, cam_gt) = *camera;
    let size = window.size();
    let corners = [
        Vec2::ZERO,
        Vec2::new(size.x, 0.0),
        size,
        Vec2::new(0.0, size.y),
    ]
    .map(|p| {
        camera
            .viewport_to_world(cam_gt, p)
            .ok()
            .and_then(|ray| crate::camera::ray_to_ground(ray, &ground))
            .map(|g| on_map(g.x, -g.z))
            .unwrap_or_default()
    });
    let (min, max) = corners.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), c| (lo.min(*c), hi.max(*c)),
    );
    if let Ok((mut node, _, _)) = nodes.get_mut(minimap.view) {
        node.set_if_neq(Node {
            left: px(min.x),
            top: px(min.y),
            width: px((max.x - min.x).max(2.0)),
            height: px((max.y - min.y).max(2.0)),
            ..node.clone()
        });
    }
}

/// How the minimap marks a unit that is not a hero or an objective: Dota's icon
/// (a white mask, tinted by the side) and its size in interface pixels. Towers
/// and barracks of the mid lane's diagonal wear the icon turned 45 degrees, the
/// others the one at 90.
fn unit_icon(u: &UnitView) -> (Option<&'static str>, f32) {
    let diagonal = (u.pos.x.to_f32() - u.pos.y.to_f32()).abs() < MID_DIAGONAL;
    match u.kind {
        UnitKind::Tower if diagonal => (Some("icon.minimap.tower45"), 11.0),
        UnitKind::Tower => (Some("icon.minimap.tower90"), 11.0),
        UnitKind::Barracks if diagonal => (Some("icon.minimap.barracks45"), 11.0),
        UnitKind::Barracks => (Some("icon.minimap.barracks90"), 11.0),
        UnitKind::Ancient => (Some("icon.minimap.ancient"), 15.0),
        UnitKind::CreepSiege => (Some("icon.minimap.siege"), 8.0),
        UnitKind::Courier => (Some("icon.minimap.courier"), 9.0),
        _ => (Some("icon.minimap.creep"), 7.0),
    }
}

/// How far off the map's diagonal, in world units, a building still lies on the mid lane.
const MID_DIAGONAL: f32 = 2500.0;

/// How the minimap marks one of the map's objectives: Dota's icon (a cell of
/// the minimap sheet where it is one, in pixels of the sheet), its tint and
/// its size in interface pixels. None for any other unit.
fn objective_icon(u: &UnitView) -> Option<(&'static str, Option<Rect>, Color, f32)> {
    let has = |id: EffectId| u.effects.iter().any(|e| e.id == id);
    let cell = |x: f32, y: f32, side: f32| Some(Rect::new(x, y, x + side, y + side));
    match u.kind {
        // minimap_miscbuilding, a white mask tinted by the side holding it.
        UnitKind::Outpost => Some((
            "icon.minimap.building",
            None,
            crate::overlay::team_color(u.team),
            14.0,
        )),
        // minimap_watcher (128,0 64x64).
        UnitKind::Watcher => Some((
            "icon.minimap.sheet",
            cell(128.0, 0.0, 64.0),
            if has(EffectId::VISION_GRANT) {
                Color::WHITE
            } else {
                DORMANT
            },
            12.0,
        )),
        // The unit's minimap_wisdom has no texture in the install: the wisdom
        // rune's cell, minimap_rune_xp (544,448 32x32).
        UnitKind::Shrine => Some((
            "icon.minimap.sheet",
            cell(544.0, 448.0, 32.0),
            if has(EffectId::SHRINE_READY) {
                Color::WHITE
            } else {
                DORMANT
            },
            12.0,
        )),
        _ => None,
    }
}

/// Marks each of the map's objectives shown by its own icon. The icons are
/// made as objectives come into view, so a match with none carries none.
fn objective_marks(
    mut commands: Commands,
    session: NonSend<Session>,
    mut minimap: ResMut<Minimap>,
    grid: Res<FogGrid>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    mut marks: Query<(&mut Node, &mut ImageNode, &mut Visibility)>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let shown: Vec<_> = view
        .units
        .iter()
        .filter(|u| !grid.hides(u.seen_by))
        .filter_map(|u| Some((u, objective_icon(u)?)))
        .filter_map(|(u, (icon, rect, tint, size))| {
            Some((u, manifest.file(icon)?, rect, tint, size))
        })
        .take(OBJECTIVE_MARKS)
        .collect();
    let (layer, icons) = &mut minimap.objectives;
    while icons.len() < shown.len() {
        icons.push(
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    ImageNode::default(),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ChildOf(*layer),
                ))
                .id(),
        );
    }
    for (i, icon) in icons.iter().enumerate() {
        let Ok((mut node, mut image, mut vis)) = marks.get_mut(*icon) else {
            continue;
        };
        let Some((u, path, rect, tint, size)) = shown.get(i) else {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        };
        let at = on_map(u.pos.x.to_f32(), u.pos.y.to_f32());
        node.set_if_neq(Node {
            left: px(at.x - size / 2.0),
            top: px(at.y - size / 2.0),
            width: px(*size),
            height: px(*size),
            ..node.clone()
        });
        let handle: Handle<Image> = server.load(path.clone());
        if image.image != handle || image.rect != *rect || image.color != *tint {
            image.image = handle;
            image.rect = *rect;
            image.color = *tint;
        }
        vis.set_if_neq(Visibility::Inherited);
    }
}

/// Marks every hero shown by Dota's arrow in its player's colour, turned the
/// way it faces; with Alt held, as its own icon in a disc of that colour.
#[allow(clippy::type_complexity)]
fn hero_marks(
    session: NonSend<Session>,
    minimap: Res<Minimap>,
    grid: Res<FogGrid>,
    colours: Res<crate::player_colour::PlayerColours>,
    fired: Res<crate::keys::Fired>,
    manifest: Res<crate::manifest::Manifest>,
    server: Res<AssetServer>,
    mut marks: Query<(
        &mut Node,
        &mut ImageNode,
        &mut UiTransform,
        &mut Visibility,
        &mut HeroMark,
    )>,
    mut icons: Query<(&mut ImageNode, &mut Visibility), Without<HeroMark>>,
    mut handles: Local<IconHandles>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let icons_on = fired.held(crate::keys::Action::ShowRanges);
    let heroes = view
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Hero && u.owner.is_some() && !grid.hides(u.seen_by));
    let mut used = 0;
    for u in heroes.take(HERO_MARKS) {
        let (mark, icon) = minimap.heroes[used];
        used += 1;
        let (Ok((mut node, mut image, mut turn, mut vis, mut marked)), Some(seat)) =
            (marks.get_mut(mark), u.owner)
        else {
            continue;
        };
        marked.0 = Some((u.team, seat));
        let face = u
            .hero
            .filter(|_| icons_on)
            .and_then(|hero| manifest.dota_name("hero", hero.0))
            .and_then(|name| handles.get(&format!("minimap.hero.{name}"), &manifest, &server));
        let size = if face.is_some() { HERO_ICON } else { HERO_MARK };
        let at = on_map(u.pos.x.to_f32(), u.pos.y.to_f32());
        node.set_if_neq(Node {
            left: px(at.x - size / 2.0),
            top: px(at.y - size / 2.0),
            width: px(size),
            height: px(size),
            border_radius: BorderRadius::MAX,
            ..node.clone()
        });
        // The arrow points east in its art and the map's north is up: a
        // facing counter-clockwise on the map is the opposite way on screen.
        let rotation = if face.is_some() {
            Rot2::IDENTITY
        } else {
            Rot2::radians(-f32::from(u.facing.brads) / 65536.0 * std::f32::consts::TAU)
        };
        turn.set_if_neq(UiTransform::from_rotation(rotation));
        let tint = colours.of(view, seat);
        let invisible = u.statuses.bits & bota_proto::StatusFlags::INVISIBLE != 0;
        let handle = handles
            .get(hero_backing(face.is_some(), invisible), &manifest, &server)
            .unwrap_or_default();
        if image.image != handle || image.color != tint {
            image.image = handle;
            image.color = tint;
        }
        vis.set_if_neq(Visibility::Inherited);
        if let Ok((mut image, mut shown)) = icons.get_mut(icon) {
            shown.set_if_neq(crate::ui::visible(face.is_some()));
            if let Some(handle) = face
                && image.image != handle
            {
                image.image = handle;
            }
        }
    }
    for (mark, _) in &minimap.heroes[used..] {
        if let Ok((_, _, _, mut vis, mut marked)) = marks.get_mut(*mark) {
            vis.set_if_neq(Visibility::Hidden);
            marked.0 = None;
        }
    }
}

/// What a hero's mark is drawn on: a disc round its icon (Alt held), else
/// Dota's arrow, filled; the hollow one only for a hero who is invisible (an
/// ally's, or an enemy's under true sight: the view tells no other).
fn hero_backing(icon: bool, invisible: bool) -> &'static str {
    match (icon, invisible) {
        (true, _) => "icon.minimap.circle",
        (false, true) => "icon.minimap.hero_invisible",
        (false, false) => "icon.minimap.hero_arrow",
    }
}

/// The rings a teleport shows on the minimap, in its side's colour: where it
/// lands, which the sim tells both sides, and where it leaves from, when the
/// channelling hero is in sight (the rule of the world's own effects).
fn teleport_rings(
    teleport: &bota_proto::TeleportView,
    origin: Option<(f32, f32)>,
) -> impl Iterator<Item = (Vec2, f32, Color)> {
    let size = ping_size(teleport.ticks_total.saturating_sub(teleport.ticks_left));
    let colour = crate::overlay::team_color(teleport.team);
    let landing = Vec2::new(
        teleport.destination.x.to_f32(),
        teleport.destination.y.to_f32(),
    );
    [Some(landing), origin.map(|(x, y)| Vec2::new(x, y))]
        .into_iter()
        .flatten()
        .map(move |at| (at, size, colour))
}

/// How wide a ping is after `elapsed` ticks of a channel: it swells and
/// starts over, so it draws the eye for as long as the channel runs.
fn ping_size(elapsed: u32) -> f32 {
    let swell = (elapsed % PING_PERIOD) as f32 / PING_PERIOD as f32;
    PING_SIZES.0 + (PING_SIZES.1 - PING_SIZES.0) * swell
}

/// Rings the landing of every teleport under way, in its player's colour,
/// whichever side it brings (Dota marks both, through the fog), and then
/// every spot the side's players pinged, swelling the same way.
fn pings(
    session: NonSend<Session>,
    minimap: Res<Minimap>,
    colours: Res<crate::player_colour::PlayerColours>,
    mut nodes: Query<(&mut Node, &mut BorderColor, &mut Visibility)>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let teleports = view
        .teleports
        .iter()
        .flat_map(|teleport| teleport_rings(teleport, crate::teleport::origin(view, teleport)));
    let marks = crate::pings::marks(core, &colours, |_| None)
        .into_iter()
        .map(|mark| {
            let ticks = (mark.age * crate::pings::TICKS_PER_SECOND) as u32;
            (mark.at, ping_size(ticks), mark.color())
        });
    let mut rings = teleports.chain(marks);
    for ping in &minimap.pings {
        let Ok((mut node, mut border, mut visibility)) = nodes.get_mut(*ping) else {
            continue;
        };
        let Some((at, size, color)) = rings.next() else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let centre = on_map(at.x, at.y);
        node.set_if_neq(Node {
            left: px(centre.x - size / 2.0),
            top: px(centre.y - size / 2.0),
            width: px(size),
            height: px(size),
            ..node.clone()
        });
        border.set_if_neq(BorderColor::all(color));
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// A left click or a drag moves the camera; a right click walks there.
fn clicks(
    mut session: NonSendMut<Session>,
    minimap: Res<Minimap>,
    frames: Query<(&Interaction, &ComputedNode, &UiGlobalTransform)>,
    buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cam: Single<&mut RtsCamera>,
    ground: Res<Ground>,
    fired: Res<crate::keys::Fired>,
    mut pinged: Local<bool>,
) {
    // A ping's click, held on, drags nothing.
    if !buttons.pressed(MouseButton::Left) {
        *pinged = false;
    }
    let Ok((interaction, computed, transform)) = frames.get(minimap.frame) else {
        return;
    };
    if *interaction == Interaction::None {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    // The node's centre and size are in physical pixels.
    let scale = window.scale_factor();
    let center = transform.translation / scale;
    let half = computed.size() / scale / 2.0;
    let k = ((cursor - (center - half)) / (half * 2.0)).clamp(Vec2::ZERO, Vec2::ONE);
    let (x, y) = (k.x * MAP_SIZE, (1.0 - k.y) * MAP_SIZE);
    let core = &mut session.0;
    if let Some(kind) = crate::pings::pressed(&fired) {
        *pinged = true;
        if crate::pings::may(core) {
            let at = bota_client_core::world_vec((x, y));
            core.ping(bota_proto::Ping {
                kind,
                at: bota_proto::PingAt::Spot(at),
            });
        }
        return;
    }
    if buttons.pressed(MouseButton::Left) && core.held_item.is_none() && !*pinged {
        core.pinned = None;
        cam.focus = Vec3::new(x, ground.at(x, y), -y);
    }
    if buttons.just_pressed(MouseButton::Right)
        && core.my_hero().is_some()
        && core.controls_selection()
    {
        core.act(Intent::MoveTo((x, y)));
    }
}

#[cfg(test)]
mod tests {
    use super::{hero_backing, teleport_rings};
    use bota_proto::{EntityId, SlotId, Team, TeleportView, Vec2};

    /// A hero seen is the filled arrow, whichever side it is on; the hollow
    /// arrow is the invisible one's alone, and the icon's disc is the same for both.
    #[test]
    fn the_hollow_arrow_is_only_for_an_invisible_hero() {
        assert_eq!(hero_backing(false, false), "icon.minimap.hero_arrow");
        assert_eq!(hero_backing(false, true), "icon.minimap.hero_invisible");
        assert_eq!(hero_backing(true, false), hero_backing(true, true));
    }

    fn teleport() -> TeleportView {
        TeleportView {
            slot: SlotId(0),
            team: Team::Dire,
            building: EntityId {
                idx: 1,
                generation: 0,
            },
            destination: Vec2::from_ints(4000, 9000),
            ticks_left: 60,
            ticks_total: 90,
        }
    }

    /// A teleport shows where it lands and, with the hero in sight, where it
    /// leaves from, both in its side's colour; out of sight, only the landing.
    #[test]
    fn a_teleport_shows_both_ends_as_far_as_the_hero_is_in_sight() {
        let side = crate::overlay::team_color(Team::Dire);
        let both: Vec<_> = teleport_rings(&teleport(), Some((1000.0, 2000.0))).collect();
        assert_eq!(both.len(), 2);
        assert_eq!(both[0].0, bevy::math::Vec2::new(4000.0, 9000.0));
        assert_eq!(both[1].0, bevy::math::Vec2::new(1000.0, 2000.0));
        assert!(both.iter().all(|ring| ring.2 == side));
        let landing: Vec<_> = teleport_rings(&teleport(), None).collect();
        assert_eq!(landing.len(), 1);
        assert_eq!(landing[0].0, bevy::math::Vec2::new(4000.0, 9000.0));
    }
}
