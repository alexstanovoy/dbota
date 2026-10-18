//! The bottom panel's portrait of the unit shown there: a live render of the
//! unit's own model, framed as Dota frames it. The unit's parts join a render
//! layer of their own for as long as it is the subject, and one camera, aimed
//! by the framing the cache holds for its model (or by the model's bounds
//! where Dota has none), draws them into one image the panel shows.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    camera::{ImageRenderTarget, RenderTarget, primitives::Aabb, visibility::RenderLayers},
    light::{AmbientLight, SpotLight},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    transform::TransformSystems,
    world_serialization::WorldAssetRoot,
};
use bota_proto::UnitView;

use crate::manifest::Manifest;
use crate::session::Session;
use crate::units::{ModelBank, ModelOf, Unit, UnitIndex};

/// Pixels of the portrait's image: twice the panel's 124 by 140.
const SIZE: (u32, u32) = (248, 280);
/// The layer only the portrait camera and its light see.
const LAYER: usize = 2;
/// Lux of the portrait light at its target, (its power follows
/// from the distance).
const LIGHT_LUX: f32 = 7_000.0;
/// Where a camera framed by bounds stands, in the model's own axes (x to its
/// right, y up, -z ahead): in front, a little to the left and above.
const BOUNDS_VIEW: Vec3 = Vec3::new(-0.35, 0.3, -0.9);

/// What the panel shows of the portrait.
#[derive(Resource)]
pub struct Portrait {
    /// The image the camera draws into.
    pub image: Handle<Image>,
    /// Whether a unit is drawn in it now.
    pub live: bool,
}

/// Dota's framing of one model, in the model's own inches and degrees.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Framing {
    camera: Vec3,
    /// Source's angles: pitch down, yaw about up.
    angles: Vec3,
    /// Horizontal field of view in degrees.
    fov: f32,
    far: f32,
    light: Vec3,
    light_angles: Vec3,
    light_fov: f32,
    light_color: Vec3,
    ambient: Vec3,
}

/// Every framing of the cache, by the model's logical name.
#[derive(Resource, Default)]
struct Framings(HashMap<String, Framing>);

impl Framings {
    fn load(root: Option<&std::path::Path>) -> Self {
        let text = root
            .and_then(|r| crate::cachefs::read_small(r, "models/portraits.txt"))
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .unwrap_or_default();
        Self(text.lines().filter_map(parse_line).collect())
    }
}

fn parse_line(line: &str) -> Option<(String, Framing)> {
    if line.starts_with('#') {
        return None;
    }
    let mut words = line.split_whitespace();
    let logical = words.next()?.to_owned();
    let n: Vec<f32> = words.map(|w| w.parse().ok()).collect::<Option<_>>()?;
    // The angles' roll is always zero in Dota's file and goes unread.
    let [
        cx,
        cy,
        cz,
        pitch,
        yaw,
        _,
        fov,
        far,
        lx,
        ly,
        lz,
        lpitch,
        lyaw,
        _,
        light_fov,
        r,
        g,
        b,
        ar,
        ag,
        ab,
    ] = n.as_slice()
    else {
        return None;
    };
    Some((
        logical,
        Framing {
            camera: Vec3::new(*cx, *cy, *cz),
            angles: Vec3::new(*pitch, *yaw, 0.0),
            fov: *fov,
            far: *far,
            light: Vec3::new(*lx, *ly, *lz),
            light_angles: Vec3::new(*lpitch, *lyaw, 0.0),
            light_fov: *light_fov,
            light_color: Vec3::new(*r, *g, *b) / 255.0,
            ambient: Vec3::new(*ar, *ag, *ab) / 255.0,
        },
    ))
}

/// Dota's model axes (x ahead, y left, z up) in the loaded scene's: the glTF
/// loader turns the models to Bevy's forward, -z, with +x on the right.
fn to_scene(v: Vec3) -> Vec3 {
    Vec3::new(-v.y, v.z, -v.x)
}

/// The way Source's angles look, in the model's axes.
fn look_dir(angles: Vec3) -> Vec3 {
    let (pitch, yaw) = (angles.x.to_radians(), angles.y.to_radians());
    Vec3::new(
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    )
}

#[derive(Component)]
struct PortraitCamera;

#[derive(Component)]
struct PortraitLight;

pub fn plugin(app: &mut App) {
    app.add_systems(
        PreStartup,
        |mut commands: Commands, manifest: Res<Manifest>| {
            commands.insert_resource(Framings::load(manifest.root.as_deref()));
        },
    )
    .add_systems(Startup, spawn)
    .add_systems(Update, select_named)
    .add_systems(
        PostUpdate,
        aim.after(TransformSystems::Propagate)
            .before(bevy::camera::CameraUpdateSystems),
    );
}

fn spawn(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut image = Image::new_fill(
        Extent3d {
            width: SIZE.0,
            height: SIZE.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 4],
        TextureFormat::Bgra8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT;
    let image = images.add(image);
    commands.insert_resource(Portrait {
        image: image.clone(),
        live: false,
    });
    commands.spawn((
        PortraitCamera,
        Camera3d::default(),
        Camera {
            order: -2,
            is_active: false,
            clear_color: ClearColorConfig::Custom(crate::ui::PANEL_DEEP),
            ..default()
        },
        RenderTarget::Image(ImageRenderTarget {
            handle: image,
            scale_factor: 1.0,
        }),
        Projection::Perspective(PerspectiveProjection::default()),
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        AmbientLight::default(),
        Msaa::Sample4,
        RenderLayers::layer(LAYER),
    ));
    commands.spawn((
        PortraitLight,
        SpotLight {
            range: 20_000.0,
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
        RenderLayers::layer(LAYER),
    ));
}

/// The parts of a unit that the portrait draws: all of them, wearables
/// included.
fn parts_of(root: Entity, children: &Query<&Children>) -> Vec<Entity> {
    children.iter_descendants(root).collect()
}

/// Frames the panel's unit: its parts join the layer, the camera and the
/// light take their places, or the camera rests when there is no unit.
fn aim(
    session: NonSend<Session>,
    index: Res<UnitIndex>,
    framings: Res<Framings>,
    bank: Res<ModelBank>,
    mut portrait: ResMut<Portrait>,
    mut commands: Commands,
    units: Query<(&Unit, &ModelOf)>,
    children: Query<&Children>,
    layers: Query<&RenderLayers>,
    transforms: Query<&GlobalTransform>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    scenes: Query<
        &Transform,
        (
            With<WorldAssetRoot>,
            Without<PortraitCamera>,
            Without<PortraitLight>,
        ),
    >,
    mut camera: Single<
        (
            &mut Camera,
            &mut Transform,
            &mut Projection,
            &mut AmbientLight,
        ),
        (With<PortraitCamera>, Without<PortraitLight>),
    >,
    mut light: Single<(&mut SpotLight, &mut Transform), With<PortraitLight>>,
    mut subject: Local<Option<Entity>>,
) {
    let wanted = crate::hud::subject(&session.0)
        .1
        .map(|u: &UnitView| u.id)
        .and_then(|id| index.0.get(&id).copied())
        .filter(|e| units.contains(*e));
    if *subject != wanted {
        if let Some(old) = *subject {
            release(old, &mut commands, &children, &layers);
        }
        *subject = wanted;
    }
    let Some(root) = wanted else {
        camera.0.is_active = false;
        portrait.live = false;
        return;
    };
    for part in parts_of(root, &children) {
        if !layers.contains(part) {
            commands
                .entity(part)
                .insert(RenderLayers::from_layers(&[0, LAYER]));
        }
    }
    let Ok((unit, ModelOf(pivot))) = units.get(root) else {
        return;
    };
    let Ok(pivot_at) = transforms.get(*pivot) else {
        return;
    };
    let model = unit.model.map(|m| &bank.0[m].model.logical);
    let size = children
        .get(*pivot)
        .ok()
        .and_then(|kids| kids.iter().find_map(|k| scenes.get(k).ok()))
        .map(|t| t.scale.x / crate::map::UNITS_PER_METRE);
    let aspect = SIZE.0 as f32 / SIZE.1 as f32;
    let framed = match (model.and_then(|m| framings.0.get(m)), size) {
        (Some(f), Some(size)) => Some(place(f, size)),
        _ => None,
    };
    let placed = framed.or_else(|| {
        let (center, radius) = bounds_in(*pivot, pivot_at, &children, &bounds)?;
        Some(fit(center, radius))
    });
    let Some(placed) = placed else {
        camera.0.is_active = false;
        portrait.live = false;
        return;
    };
    let to_world = |local: &Transform| pivot_at.mul_transform(*local).compute_transform();
    *camera.1 = to_world(&placed.camera);
    *camera.2 = Projection::Perspective(PerspectiveProjection {
        fov: placed.vfov,
        aspect_ratio: aspect,
        near: 4.0,
        far: placed.far,
        ..default()
    });
    camera.3.color = placed.ambient;
    camera.3.brightness = 600.0;
    *light.1 = to_world(&placed.light.0);
    light.0.color = placed.light.1;
    light.0.intensity = LIGHT_LUX * 4.0 * std::f32::consts::PI * placed.light.2.powi(2);
    light.0.outer_angle = placed.light.3;
    light.0.inner_angle = placed.light.3 * 0.8;
    camera.0.is_active = true;
    portrait.live = true;
}

/// Where the camera and the light stand, in the pivot's space.
struct Placed {
    camera: Transform,
    vfov: f32,
    far: f32,
    ambient: Color,
    /// Position and direction, colour, distance to the model and cone.
    light: (Transform, Color, f32, f32),
}

/// Dota's framing, scaled with the model as it is drawn. Its field of view is
/// the picture's height.
fn place(f: &Framing, size: f32) -> Placed {
    let camera = to_scene(f.camera) * size;
    let ahead = to_scene(look_dir(f.angles));
    let vfov = f.fov.to_radians();
    let light = to_scene(f.light) * size;
    let target = Vec3::new(0.0, f.camera.z.min(f.light.z) * size * 0.5, 0.0);
    Placed {
        camera: Transform::from_translation(camera).looking_to(ahead, Vec3::Y),
        vfov,
        far: f.far * size.max(1.0),
        ambient: Color::srgb(f.ambient.x, f.ambient.y, f.ambient.z),
        light: (
            Transform::from_translation(light)
                .looking_to(to_scene(look_dir(f.light_angles)), Vec3::Y),
            Color::srgb(f.light_color.x, f.light_color.y, f.light_color.z),
            light.distance(target),
            (f.light_fov.to_radians() / 2.0).clamp(0.2, 1.5),
        ),
    }
}

/// A framing from the model's bounds: the centre and the radius of its sphere.
fn fit(center: Vec3, radius: f32) -> Placed {
    let vfov = 30f32.to_radians();
    let distance = radius / (vfov / 2.0).sin() * 1.1;
    let at = center + BOUNDS_VIEW.normalize() * distance;
    let light = center + Vec3::new(-0.6, 1.0, -0.8).normalize() * distance * 1.5;
    Placed {
        camera: Transform::from_translation(at).looking_at(center, Vec3::Y),
        vfov,
        far: distance + radius * 4.0,
        ambient: Color::srgb(0.7, 0.75, 0.85),
        light: (
            Transform::from_translation(light).looking_at(center, Vec3::Y),
            Color::srgb(1.0, 0.97, 0.92),
            distance * 1.5,
            0.9,
        ),
    }
}

/// The sphere round the parts of a model with bounds, in the pivot's space.
fn bounds_in(
    pivot: Entity,
    pivot_at: &GlobalTransform,
    children: &Query<&Children>,
    bounds: &Query<(&Aabb, &GlobalTransform)>,
) -> Option<(Vec3, f32)> {
    let to_pivot = pivot_at.affine().inverse();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for part in children.iter_descendants(pivot) {
        let Ok((aabb, at)) = bounds.get(part) else {
            continue;
        };
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        for corner in 0..8u8 {
            let sign = |bit: u8| if corner & bit == 0 { -1.0 } else { 1.0 };
            let p = c + h * Vec3::new(sign(1), sign(2), sign(4));
            let p = to_pivot.transform_point3(at.affine().transform_point3(p));
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo.x <= hi.x).then(|| ((lo + hi) / 2.0, ((hi - lo) / 2.0).length().max(8.0)))
}

/// Takes a unit's parts out of the portrait's layer again.
fn release(
    root: Entity,
    commands: &mut Commands,
    children: &Query<&Children>,
    layers: &Query<&RenderLayers>,
) {
    for part in parts_of(root, children) {
        if layers.contains(part) {
            commands.entity(part).remove::<RenderLayers>();
        }
    }
}

/// Debug: selects the first unit shown that `--select` names, until one is.
fn select_named(args: Res<crate::Args>, mut session: NonSendMut<Session>, units: Query<&Unit>) {
    let Some(want) = args.select.as_deref() else {
        return;
    };
    let (name, side) = want.split_once('@').unwrap_or((want, ""));
    let core = &mut session.0;
    let Some(view) = core.view() else {
        return;
    };
    let found = units.iter().find_map(|u| {
        let live = view.unit(u.id)?;
        let side_ok = side.is_empty() || format!("{:?}", u.team).eq_ignore_ascii_case(side);
        (side_ok && crate::units::unit_name(live).eq_ignore_ascii_case(name)).then_some(u.id)
    });
    if found.is_some() && core.selected != found {
        core.selected = found;
    }
}
