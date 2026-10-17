//! The minimap: the map photographed once from straight above, the fog over
//! it, a mark for every unit shown and the camera's frame. A left click or a
//! drag moves the camera; a right click walks there.

use bevy::{
    camera::{ImageRenderTarget, RenderTarget, ScalingMode, visibility::RenderLayers},
    prelude::*,
    render::render_resource::TextureFormat,
    window::PrimaryWindow,
};
use bota_client_core::Intent;
use bota_proto::{Team, UnitKind};

use crate::camera::RtsCamera;
use crate::fog::FogGrid;
use crate::hud::Blocker;
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
/// Teleport pings shown at most.
const PINGS: usize = 10;
/// Interface pixels across a ping at its smallest and at its widest.
const PING_SIZES: (f32, f32) = (10.0, 30.0);
/// Ticks one swell of a ping takes.
const PING_PERIOD: u32 = 30;

/// The minimap's frame, the photo, the marks and the camera frame.
#[derive(Resource)]
struct Minimap {
    frame: Entity,
    photo: Handle<Image>,
    fog: Entity,
    marks: Vec<Entity>,
    pings: Vec<Entity>,
    view: Entity,
}

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
        .add_systems(Update, (photograph, marks, pings, clicks));
}

fn spawn(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let photo = images.add(Image::new_target_texture(
        PHOTO,
        PHOTO,
        TextureFormat::Bgra8UnormSrgb,
        None,
    ));
    let frame = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(6),
                bottom: px(6),
                width: px(SIDE),
                height: px(SIDE),
                border: UiRect::all(px(3)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.08, 0.11, 0.07)),
            BorderColor::all(Color::srgb(0.18, 0.2, 0.24)),
            Interaction::default(),
            Blocker,
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
    let marks = (0..MARKS)
        .map(|_| {
            commands
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: px(6),
                        height: px(6),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::WHITE),
                    Outline::new(px(1), px(0), Color::BLACK),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                    ChildOf(frame),
                ))
                .id()
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
    mut nodes: Query<(&mut Node, &mut BackgroundColor, &mut Visibility)>,
    mut images: Query<&mut ImageNode>,
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
    let hidden = view.viewer.is_none() && grid.on;
    let me = core.my_hero();
    let shown = view.units.iter().filter(|u| {
        !matches!(u.kind, UnitKind::Ward)
            && (!hidden
                || Some(u.team) == grid.team
                || crate::units::is_building(u.kind)
                || grid.lit(u.pos.x.to_f32(), u.pos.y.to_f32()))
    });
    let mut used = 0;
    for u in shown.take(MARKS) {
        let Ok((mut node, mut bg, mut vis)) = nodes.get_mut(minimap.marks[used]) else {
            break;
        };
        used += 1;
        let size = match u.kind {
            UnitKind::Hero => 11.0,
            UnitKind::Tower | UnitKind::Barracks | UnitKind::Ancient => 9.0,
            _ => 5.0,
        };
        let at = on_map(u.pos.x.to_f32(), u.pos.y.to_f32());
        // Written only when changed: a changed node is laid out again.
        node.set_if_neq(Node {
            left: px(at.x - size / 2.0),
            top: px(at.y - size / 2.0),
            width: px(size),
            height: px(size),
            ..node.clone()
        });
        bg.set_if_neq(BackgroundColor(if Some(u.id) == me {
            Color::srgb(0.5, 1.0, 0.55)
        } else {
            match u.team {
                Team::Radiant => Color::srgb(0.2, 0.75, 0.25),
                Team::Dire => Color::srgb(0.85, 0.2, 0.15),
                Team::Neutral => Color::srgb(0.8, 0.7, 0.4),
            }
        }));
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

/// How wide a ping is after `elapsed` ticks of a channel: it swells and
/// starts over, so it draws the eye for as long as the channel runs.
fn ping_size(elapsed: u32) -> f32 {
    let swell = (elapsed % PING_PERIOD) as f32 / PING_PERIOD as f32;
    PING_SIZES.0 + (PING_SIZES.1 - PING_SIZES.0) * swell
}

/// Rings the landing of every teleport under way, in its player's colour,
/// whichever side it brings: Dota marks both, through the fog.
fn pings(
    session: NonSend<Session>,
    minimap: Res<Minimap>,
    mut nodes: Query<(&mut Node, &mut BorderColor, &mut Visibility)>,
) {
    let core = &session.0;
    let teleports = core.view().map_or(&[][..], |v| &v.teleports[..]);
    for (at, ping) in minimap.pings.iter().enumerate() {
        let Ok((mut node, mut border, mut visibility)) = nodes.get_mut(*ping) else {
            continue;
        };
        let (Some(teleport), Some(view)) = (teleports.get(at), core.view()) else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let size = ping_size(teleport.ticks_total.saturating_sub(teleport.ticks_left));
        let centre = on_map(
            teleport.destination.x.to_f32(),
            teleport.destination.y.to_f32(),
        );
        node.set_if_neq(Node {
            left: px(centre.x - size / 2.0),
            top: px(centre.y - size / 2.0),
            width: px(size),
            height: px(size),
            ..node.clone()
        });
        border.set_if_neq(BorderColor::all(crate::teleport::player_color(
            view,
            teleport.slot,
        )));
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
) {
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
    if buttons.pressed(MouseButton::Left) && core.held_item.is_none() {
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
