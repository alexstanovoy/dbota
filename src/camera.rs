//! Dota-like RTS camera: fixed yaw (north up), fixed pitch, long lens, pan and zoom.

use bevy::{
    input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit},
    prelude::*,
    window::{PrimaryWindow, WindowResized},
};

use crate::keys::{Action, Fired};
use crate::map::{DOTA_ORIGIN, Ground};
use crate::session::Session;
use crate::units::{Unit, UnitIndex};
use bota_proto::{SlotId, UnitKind};

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
        .add_systems(
            Update,
            (pan_and_zoom, follow, apply_camera_transform).chain(),
        )
        // Picking needs the camera's GlobalTransform of this frame.
        .add_systems(
            PostUpdate,
            pick_cursor_ground.after(TransformSystems::Propagate),
        )
        .add_systems(Update, log_window_size);
}

/// Camera rotation: yaw 0 (looking north = -Z), pitched `pitch` degrees down.
pub fn camera_rotation(pitch: f32) -> Quat {
    Quat::from_rotation_x(-pitch.to_radians())
}

fn camera_translation(cam: &RtsCamera, rotation: Quat) -> Vec3 {
    // Camera looks along rotation * -Z; step back along the opposite direction.
    cam.focus - rotation * Vec3::NEG_Z * cam.distance
}

fn spawn_camera(
    mut commands: Commands,
    args: Res<crate::Args>,
    settings: Res<crate::config::Settings>,
) {
    let msaa = crate::quality::msaa_of(&settings);
    let lens = settings.camera;
    let (near, far) = lens.range();
    let cam = RtsCamera {
        focus: match args.look[..] {
            [x, y] => Vec3::new(x, 128.0, -y),
            _ => Vec3::new(DOTA_ORIGIN, 128.0, -DOTA_ORIGIN),
        },
        distance: args
            .distance
            .unwrap_or(lens.distance)
            .clamp(near / 2.0, far),
    };
    commands.spawn((
        Camera3d::default(),
        Projection::Perspective(PerspectiveProjection {
            fov: lens.fov.to_radians(),
            near: 50.0,
            far: 20_000.0,
            ..default()
        }),
        // As the quality settings ask; VRAM scales with it.
        msaa,
        // The scene and its particles add up in a floating-point target and are tonemapped
        // together after, as Source's renderer does (Dota tonemaps): an overbright additive
        // card keeps its hue instead of each channel clipping to white at 1.
        bevy::camera::Hdr,
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        // Layer 1 holds what the minimap's photograph leaves out.
        bevy::camera::visibility::RenderLayers::from_layers(&[0, 1]),
        Transform::from_translation(camera_translation(&cam, camera_rotation(lens.pitch)))
            .with_rotation(camera_rotation(lens.pitch)),
        cam,
    ));
}

/// Which way the cursor at the window's edge pans, in sim axes (x east, y
/// north), all in logical pixels whatever the scale factor. The HUD bars do
/// not stop it, as in Dota. A cursor that left the window across an edge
/// (onto a title bar or a panel above a window that is not fullscreen) keeps
/// panning that way until it comes back.
fn edge_pan(cursor: Option<Vec2>, last: Option<Vec2>, size: Vec2) -> Vec2 {
    let Some(p) = cursor.or(last) else {
        return Vec2::ZERO;
    };
    let m = EDGE_PAN_MARGIN;
    let x = f32::from(p.x >= size.x - m) - f32::from(p.x <= m);
    // Window y grows downward; sim y is north (up on screen).
    let y = f32::from(p.y <= m) - f32::from(p.y >= size.y - m);
    Vec2::new(x, y)
}

#[allow(clippy::too_many_arguments)]
fn pan_and_zoom(
    time: Res<Time>,
    fired: Res<Fired>,
    scroll: Res<AccumulatedMouseScroll>,
    motion: Res<AccumulatedMouseMotion>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut cam: Single<&mut RtsCamera>,
    mut session: NonSendMut<Session>,
    args: Res<crate::Args>,
    panels: Res<crate::input::Panels>,
    over: Res<crate::input::OverHud>,
    settings: Res<crate::config::Settings>,
    mut last_cursor: Local<Option<Vec2>>,
) {
    let lens = settings.camera;
    grip(
        &fired,
        &motion,
        &window,
        &mut cam,
        &mut session,
        panels.camera_lock,
        lens.fov,
    );
    let mut dir = Vec2::ZERO; // sim axes: x east, y north
    for (action, d) in [
        (Action::CameraLeft, Vec2::NEG_X),
        (Action::CameraRight, Vec2::X),
        (Action::CameraUp, Vec2::Y),
        (Action::CameraDown, Vec2::NEG_Y),
    ] {
        if fired.held(action) {
            dir += d;
        }
    }
    let cursor = window.cursor_position();
    if window.focused && settings.aids.edge_pan && !args.unattended() {
        dir += edge_pan(cursor, *last_cursor, window.size());
    }
    if cursor.is_some() || !window.focused {
        *last_cursor = cursor;
    }
    let speed = lens.pan_speed * cam.distance / lens.distance.max(1.0) * time.delta_secs();
    let step = dir.normalize_or_zero() * speed;
    if step != Vec2::ZERO && !panels.camera_lock {
        // Driving the camera by hand lets go of whatever carried it.
        session.0.pinned = None;
        cam.focus += Vec3::new(step.x, 0.0, -step.y);
    }

    let lines = if over.0 {
        0.0
    } else {
        match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 40.0,
        }
    };
    if lines != 0.0 {
        let (near, far) = lens.range();
        cam.distance = (cam.distance * (1.0 - ZOOM_PER_LINE).powf(lines)).clamp(near, far);
    }
}

/// Dota's camera grip: while it is held the map follows the mouse, the
/// ground under the cursor staying under it.
fn grip(
    fired: &Fired,
    motion: &AccumulatedMouseMotion,
    window: &Window,
    cam: &mut RtsCamera,
    session: &mut Session,
    locked: bool,
    fov: f32,
) {
    if !fired.held(Action::CameraGrip) || motion.delta == Vec2::ZERO || locked {
        return;
    }
    // World units a pixel spans at the focus.
    let per_pixel = 2.0 * cam.distance * (fov.to_radians() / 2.0).tan() / window.height().max(1.0);
    session.0.pinned = None;
    cam.focus -= Vec3::new(motion.delta.x, 0.0, motion.delta.y) * per_pixel;
}

/// Places the camera as its focus, distance and the settings' pitch and lens
/// ask.
fn apply_camera_transform(
    mut q: Query<(&RtsCamera, &mut Transform, &mut Projection)>,
    settings: Res<crate::config::Settings>,
) {
    let lens = settings.camera;
    let rotation = camera_rotation(lens.pitch);
    for (cam, mut transform, mut projection) in &mut q {
        let translation = camera_translation(cam, rotation);
        if transform.translation != translation || transform.rotation != rotation {
            transform.translation = translation;
            transform.rotation = rotation;
        }
        if let Projection::Perspective(p) = projection.as_ref()
            && p.fov != lens.fov.to_radians()
            && let Projection::Perspective(p) = projection.as_mut()
        {
            p.fov = lens.fov.to_radians();
        }
    }
}

/// Cursor -> world: ray from the camera through the cursor, met with the
/// ground by stepping between the ground's height and the ray.
fn pick_cursor_ground(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<crate::camera::RtsCamera>>,
    ground: Res<Ground>,
    mut out: ResMut<CursorGround>,
) {
    let (camera, camera_transform) = *camera;
    out.0 = window
        .cursor_position() // logical pixels, origin top-left
        .and_then(|p| camera.viewport_to_world(camera_transform, p).ok())
        .and_then(|ray| ray_to_ground(ray, &ground));
}

/// Where a ray meets the ground.
pub fn ray_to_ground(ray: Ray3d, ground: &Ground) -> Option<Vec3> {
    let mut height = 128.0;
    let mut hit = None;
    for _ in 0..4 {
        let t = ray.intersect_plane(Vec3::new(0.0, height, 0.0), InfinitePlane3d::new(Vec3::Y))?;
        let p = ray.get_point(t);
        height = ground.at(p.x, -p.z);
        hit = Some(Vec3::new(p.x, height, p.z));
    }
    hit
}

/// Carries the camera with the unit it is pinned to, with one's own hero
/// while the camera is locked or the centre key is held, and puts it on
/// one's own hero the first time that hero stands (unless `--look` placed it).
fn follow(
    mut session: NonSendMut<Session>,
    index: Res<UnitIndex>,
    units: Query<&Unit>,
    ground: Res<Ground>,
    time: Res<Time>,
    fired: Res<Fired>,
    panels: Res<crate::input::Panels>,
    args: Res<crate::Args>,
    mut cam: Single<&mut RtsCamera>,
) {
    let core = &mut session.0;
    // A camera placed by `--look` stays where it was put.
    if !core.found_my_hero
        && args.look.is_empty()
        && core.my_hero().is_some()
        && let Some(slot) = core.my_slot
    {
        core.found_my_hero = true;
        core.pinned = Some(bota_client_core::Pin::Hero(slot));
    }
    // The bars' check watches the seat's courier, whose health is counted
    // in hits.
    if args.bars_check
        && let Some(seat) = args.pov
        && let Some(courier) = core.view().and_then(|view| {
            view.units
                .iter()
                .find(|u| u.kind == UnitKind::Courier && u.owner == Some(SlotId(seat)))
                .map(|u| u.id)
        })
    {
        core.pinned = Some(bota_client_core::Pin::Unit(courier));
    }
    let live = matches!(core.source, bota_client_core::Source::Live(_));
    // Dota's hero key held: the camera follows the hero.
    let centre = live && fired.held(Action::SelectHero);
    let carrier = if centre || panels.camera_lock {
        core.my_hero()
    } else {
        core.pinned_unit()
    };
    let Some(unit) = carrier
        .and_then(|id| index.0.get(&id))
        .and_then(|e| units.get(*e).ok())
    else {
        return;
    };
    let target = Vec3::new(unit.at.x, ground.at(unit.at.x, unit.at.y), -unit.at.y);
    let k = if centre {
        1.0
    } else {
        (time.delta_secs() * 8.0).min(1.0)
    };
    let focus = cam.focus + (target - cam.focus) * k;
    if focus.distance_squared(cam.focus) > 1e-4 {
        cam.focus = focus;
    }
}

fn log_window_size(
    mut resized: MessageReader<WindowResized>,
    window: Single<&Window, With<PrimaryWindow>>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    /// All four edges pan their way at scale factors 1 and 1.5 (logical
    /// sizes 1600x900 of a 1600x900 and a 2400x1350 window), the cursor on
    /// a HUD bar or not; a cursor gone over an edge keeps panning.
    #[test]
    fn every_edge_pans_its_way() {
        for size in [Vec2::new(1600.0, 900.0), Vec2::new(2400.0, 1350.0) / 1.5] {
            let mid = size / 2.0;
            let at = |x: f32, y: f32| edge_pan(Some(Vec2::new(x, y)), None, size);
            // The top bar of the HUD spans the top edge; the bottom panel the
            // bottom one: neither matters.
            assert_eq!(at(mid.x, 0.0), Vec2::Y, "top");
            assert_eq!(at(mid.x, 2.0), Vec2::Y, "top, inside the margin");
            assert_eq!(at(mid.x, size.y - 1.0), Vec2::NEG_Y, "bottom");
            assert_eq!(at(0.0, mid.y), Vec2::NEG_X, "left");
            assert_eq!(at(size.x - 1.0, mid.y), Vec2::X, "right");
            assert_eq!(at(0.0, 0.0), Vec2::new(-1.0, 1.0), "top-left corner");
            assert_eq!(at(mid.x, mid.y), Vec2::ZERO, "middle");
            assert_eq!(
                at(mid.x, 40.0),
                Vec2::ZERO,
                "over the top bar, off the edge"
            );
            // Left through the top: no cursor now, the last one at the edge.
            let gone = edge_pan(None, Some(Vec2::new(mid.x, 0.0)), size);
            assert_eq!(gone, Vec2::Y, "left across the top");
            assert_eq!(
                edge_pan(None, Some(mid), size),
                Vec2::ZERO,
                "left elsewhere"
            );
        }
    }
}
