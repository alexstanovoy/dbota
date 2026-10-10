//! Dota-like RTS camera: fixed yaw (north up), fixed pitch, long lens, pan and zoom.

use bevy::{
    input::mouse::{AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    window::{PrimaryWindow, WindowResized},
};

use crate::coords::world_to_sim;

/// Camera pitch below the horizon.
const PITCH_DEG: f32 = 58.0;
/// Vertical field of view: narrow, like Dota's long lens.
const FOV_Y_DEG: f32 = 25.0;
/// Distance from the focus point; 2500 shows ~2000x1300 world units at 16:9.
const DEFAULT_DISTANCE: f32 = 2500.0;
const MIN_DISTANCE: f32 = 1200.0;
const MAX_DISTANCE: f32 = 4000.0;
/// Pan speed in world units per second at the default distance.
const PAN_SPEED: f32 = 2000.0;
/// Cursor within this many logical pixels of a window edge pans the camera.
const EDGE_PAN_MARGIN: f32 = 6.0;
/// Fraction of the distance removed per scroll line.
const ZOOM_PER_LINE: f32 = 0.1;

#[derive(Component, Debug)]
pub struct RtsCamera {
    /// Ground point (Bevy coordinates, y = 0) the camera looks at.
    pub focus: Vec3,
    pub distance: f32,
}

/// Ground point under the cursor, updated every frame (None if off-window or sky).
#[derive(Resource, Default, Debug)]
pub struct CursorGround(pub Option<Vec3>);

pub fn plugin(app: &mut App) {
    app.init_resource::<CursorGround>()
        .add_systems(Startup, spawn_camera)
        .add_systems(Update, (pan_and_zoom, apply_camera_transform).chain())
        // Picking needs the camera's GlobalTransform of this frame.
        .add_systems(
            PostUpdate,
            pick_cursor_ground.after(TransformSystems::Propagate),
        )
        .add_systems(Update, (log_window_size, draw_cursor_marker));
}

/// Camera rotation: yaw 0 (looking north = -Z), pitched down. Constant, so billboards can
/// copy it once at spawn.
pub fn camera_rotation() -> Quat {
    Quat::from_rotation_x(-PITCH_DEG.to_radians())
}

fn camera_translation(cam: &RtsCamera) -> Vec3 {
    // Camera looks along rotation * -Z; step back along the opposite direction.
    cam.focus - camera_rotation() * Vec3::NEG_Z * cam.distance
}

fn spawn_camera(mut commands: Commands) {
    let cam = RtsCamera {
        focus: Vec3::ZERO,
        distance: DEFAULT_DISTANCE,
    };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_Y_DEG.to_radians(),
            near: 50.0,
            far: 20_000.0,
            ..default()
        }),
        // MSAA 4 is the default; spelled out because VRAM scales with it.
        Msaa::Sample4,
        Transform::from_translation(camera_translation(&cam)).with_rotation(camera_rotation()),
        cam,
    ));
}

fn pan_and_zoom(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    scroll: Res<AccumulatedMouseScroll>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cam: Single<&mut RtsCamera>,
) {
    let mut dir = Vec2::ZERO; // sim axes: x east, y north
    for (key, d) in [
        (KeyCode::ArrowLeft, Vec2::NEG_X),
        (KeyCode::ArrowRight, Vec2::X),
        (KeyCode::ArrowUp, Vec2::Y),
        (KeyCode::ArrowDown, Vec2::NEG_Y),
    ] {
        if keys.pressed(key) {
            dir += d;
        }
    }
    if window.focused
        && let Some(p) = window.cursor_position()
    {
        let size = window.size();
        dir.x += f32::from(p.x >= size.x - EDGE_PAN_MARGIN) - f32::from(p.x <= EDGE_PAN_MARGIN);
        // Window y grows downward; sim y is north (up on screen).
        dir.y += f32::from(p.y <= EDGE_PAN_MARGIN) - f32::from(p.y >= size.y - EDGE_PAN_MARGIN);
    }
    let speed = PAN_SPEED * cam.distance / DEFAULT_DISTANCE * time.delta_secs();
    let step = dir.normalize_or_zero() * speed;
    cam.focus += Vec3::new(step.x, 0.0, -step.y);

    let lines = match scroll.unit {
        MouseScrollUnit::Line => scroll.delta.y,
        MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
    };
    if lines != 0.0 {
        cam.distance =
            (cam.distance * (1.0 - ZOOM_PER_LINE).powf(lines)).clamp(MIN_DISTANCE, MAX_DISTANCE);
    }
}

fn apply_camera_transform(mut q: Query<(&RtsCamera, &mut Transform), Changed<RtsCamera>>) {
    for (cam, mut transform) in &mut q {
        transform.translation = camera_translation(cam);
    }
}

/// Cursor -> world: ray from the camera through the cursor, intersected with y = 0.
fn pick_cursor_ground(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform)>,
    mut out: ResMut<CursorGround>,
) {
    let (camera, camera_transform) = *camera;
    out.0 = window
        .cursor_position() // logical pixels, origin top-left
        .and_then(|p| camera.viewport_to_world(camera_transform, p).ok())
        .and_then(|ray| {
            let t = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Vec3::Y))?;
            Some(ray.get_point(t))
        });
}

fn draw_cursor_marker(cursor: Res<CursorGround>, mut gizmos: Gizmos) {
    if let Some(p) = cursor.0 {
        let flat = Isometry3d::new(
            p + Vec3::Y,
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        );
        gizmos.circle(flat, 24.0, Color::srgb(0.2, 1.0, 0.4));
    }
}

fn log_window_size(
    mut resized: MessageReader<WindowResized>,
    window: Single<&Window, With<PrimaryWindow>>,
    cursor: Res<CursorGround>,
    keys: Res<ButtonInput<KeyCode>>,
    mut logged: Local<bool>,
) {
    // The first WindowResized arrives once winit knows the real scale factor.
    if resized.read().last().is_some() || !*logged {
        *logged = true;
        let r = &window.resolution;
        info!(
            "window: scale_factor={} logical={}x{} physical={}x{}",
            r.scale_factor(),
            r.width(),
            r.height(),
            r.physical_width(),
            r.physical_height()
        );
    }
    if keys.just_pressed(KeyCode::KeyP)
        && let Some(p) = cursor.0
    {
        info!("cursor ground: bevy={p} sim={}", world_to_sim(p));
    }
}
