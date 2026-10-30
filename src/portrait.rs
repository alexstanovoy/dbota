//! The bottom panel's portrait, as Dota draws it. With `animate_portrait`
//! (Dota's "Animate Portrait", on by default) the unit's model plays its idle
//! loop in the portrait, drawn at most [`RATE`] times a second; a hero's face
//! plays its idle expression over it and, while the hero says a line, the
//! line's lip sync (see `talk`). Off, a hero shows Dota's still picture of
//! it, and a unit Dota has no picture of shows a render of its model made
//! once, in its idle pose, and kept.
//!
//! Either render is of a copy of the model alone on a stage of its own: a
//! render layer only the portrait camera and its light see, so the camera
//! draws neither the world nor its shadows or fog, and the copy is never in
//! the main camera's view, its queries or its bars. The camera stays off
//! while the main world works out visibility and the sun's cascades, and is
//! switched on afterwards (on the frames it draws) with the stage's meshes
//! as all it sees: no pass over the world's entities is made for it, and no
//! shadow cascade (its light casts none). The camera is aimed
//! by the framing the cache holds for the model (or by the model's bounds
//! where Dota has none), and the panel lays the render over the model's
//! background, tinted at its corners as Dota tints it, or a plain dark one.

use std::collections::HashMap;

use bevy::{
    asset::RenderAssetUsages,
    camera::{
        ImageRenderTarget, RenderTarget,
        primitives::Aabb,
        visibility::{
            RenderLayers, SetViewVisibility, VisibilityClass, VisibilitySystems, VisibleEntities,
            check_visibility_cpu_culling,
        },
    },
    gltf::{GltfMeshExtras, GltfMeshName},
    light::{AmbientLight, DirectionalLight, SimulationLightSystems, cluster::ClusterConfig},
    mesh::morph::{MeshMorphWeights, MorphWeights},
    prelude::*,
    render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
    world_serialization::{WorldAssetRoot, WorldInstanceReady},
};
use bota_client_core::ClientSession;
use bota_proto::{EntityId, UnitView};

use crate::config::Settings;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::talk::{Face, Faces, Speech};
use crate::units::{ModelBank, ModelOf, Posable, Scenes, Unit, UnitIndex, Wearable};

/// Pixels of the portrait's image: twice the panel's frame, of its shape,
/// so a render fills the frame whole.
const SIZE: (u32, u32) = (
    (crate::hud::PORTRAIT.0 * 2.0) as u32,
    (crate::hud::PORTRAIT.1 * 2.0) as u32,
);
/// The layer only the portrait camera, its light and the model on its stage
/// are on.
const LAYER: usize = 2;
/// Lux of the portrait light per unit of Dota's light scale.
const LIGHT_LUX: f32 = 2_800.0;
/// Brightness of the ambient colour per unit of Dota's ambient scale.
const AMBIENT: f32 = 75.0;
/// Where a camera framed by bounds stands, in the model's own axes (x to its
/// right, y up, -z ahead): in front, a little to the left and above.
const BOUNDS_VIEW: Vec3 = Vec3::new(-0.35, 0.3, -0.9);
/// Renders a second at most of an animated portrait.
const RATE: f32 = 30.0;
/// Where the stage stands: far under the map, out of every other view.
const STAGE: Vec3 = Vec3::new(0.0, -50_000.0, 0.0);
/// Renders of a still made after the GPU has every pipeline it asked for:
/// a pipeline still compiling draws nothing.
const SETTLE: u8 = 3;
/// Still renders kept at most (a quarter of a megabyte each).
const STILLS: usize = 32;

/// What the panel shows of the portrait: a render of the subject's model
/// and the background behind it (`None`: Dota has none, a plain dark one).
/// Absent, the panel shows Dota's still picture of the hero, if any.
#[derive(Resource, Default)]
pub struct Portrait {
    pub shown: Option<(Handle<Image>, Option<Handle<Image>>)>,
}

/// Dota's framing of one model, in the model's own inches and degrees.
#[derive(Clone, Debug, PartialEq)]
struct Framing {
    camera: Vec3,
    /// Source's angles: pitch down, yaw about up.
    angles: Vec3,
    /// Horizontal field of view in degrees.
    fov: f32,
    far: f32,
    light_angles: Vec3,
    light_color: Vec3,
    ambient: Vec3,
    /// Dota's brightness of the light and of the ambient colour.
    light_scale: f32,
    ambient_scale: f32,
    /// The background's image in the cache and its corner tints (top left,
    /// top right, bottom right, bottom left), when Dota gives the model one.
    background: Option<(String, [Vec3; 4])>,
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
    let line = crate::cache_text::framing(line)?;
    let n = line.numbers;
    let v = |at: usize| Vec3::new(n[at], n[at + 1], n[at + 2]);
    let background = line.background.map(|(path, t)| {
        let corner = |c: usize| Vec3::new(t[3 * c], t[3 * c + 1], t[3 * c + 2]);
        (path, [0, 1, 2, 3].map(corner))
    });
    // The angles' roll is always zero in Dota's file and goes unread, as do
    // the light's place and cone: a light from its direction stands for
    // Dota's spot on the model.
    Some((
        line.logical,
        Framing {
            camera: v(0),
            angles: Vec3::new(n[3], n[4], 0.0),
            fov: n[6],
            far: n[7],
            light_angles: Vec3::new(n[11], n[12], 0.0),
            light_color: v(15) / 255.0,
            ambient: v(18) / 255.0,
            light_scale: n[21],
            ambient_scale: n[22],
            background,
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

/// A scene of the model on the stage (its body or a wearable), and whether
/// its instance is ready and on the stage's layer.
#[derive(Component)]
struct ActorPart {
    ready: bool,
}

/// The model on the stage.
pub struct Actor {
    /// Its place in the model bank.
    model: usize,
    /// The pivot its scenes hang under.
    pub pivot: Entity,
    /// Its scale as the units draw it.
    size: f32,
    /// Whether its idle clip is playing (or posed, for a still).
    posed: bool,
    /// Whether it is drawn as a still, and the image the still goes to.
    still: Option<Handle<Image>>,
    /// Renders of a still left once the pipelines are ready.
    settle: u8,
    /// Its face, when it has one and is animated.
    pub face: Option<Face>,
}

/// The stage: the model on it, the stills made so far and the backgrounds.
#[derive(Resource)]
pub struct Stage {
    /// The model of the panel's unit, when the portrait draws it.
    subject: Option<usize>,
    /// The panel's unit.
    unit: Option<EntityId>,
    pub actor: Option<Actor>,
    /// Still renders by model, oldest first.
    stills: Vec<(usize, Handle<Image>)>,
    /// Tinted backgrounds by their image's cache path: absent until the
    /// image has loaded.
    backdrops: HashMap<String, Handle<Image>>,
    /// The image an animated portrait is drawn into.
    live: Handle<Image>,
    /// Seconds since the start when an animated portrait is drawn next.
    next_render: f32,
    /// Whether the camera draws this frame, and whether it drew the last.
    render: bool,
    rendered: bool,
    /// Debug: the model shown whatever the panel's unit, at its own scale,
    /// and drawn every frame.
    pub forced: Option<usize>,
    pub every: bool,
}

impl Stage {
    /// Whether the camera draws the stage this frame.
    pub fn drawing(&self) -> bool {
        self.render
    }

    /// The unit the portrait shows.
    pub fn unit(&self) -> Option<EntityId> {
        self.unit
    }

    /// The image an animated portrait is drawn into.
    pub fn live(&self) -> &Handle<Image> {
        &self.live
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Portrait>()
        .add_systems(
            PreStartup,
            |mut commands: Commands, manifest: Res<Manifest>| {
                commands.insert_resource(Framings::load(manifest.root.as_deref()));
            },
        )
        .add_systems(Startup, spawn)
        .add_systems(Update, (select_named, stage, pose, tint_backdrops).chain())
        .add_systems(
            PostUpdate,
            (
                (
                    aim,
                    speak,
                    crate::talk::trace.run_if(|args: Res<crate::Args>| args.face_trace),
                )
                    .chain()
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::CameraUpdateSystems),
                on_stage
                    .in_set(VisibilitySystems::CheckVisibility)
                    .after(check_visibility_cpu_culling),
                // After the world's visibility and the sun's cascades, before the
                // camera's (one) cluster is worked out.
                activate
                    .after(VisibilitySystems::CheckVisibility)
                    .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                    .before(SimulationLightSystems::AssignLightsToClusters)
                    .before(SimulationLightSystems::UpdateLightFrusta),
            ),
        );
}

/// An image a portrait camera can draw into.
fn target_image(images: &mut Assets<Image>) -> Handle<Image> {
    let mut image = Image::new_fill(
        Extent3d {
            width: SIZE.0,
            height: SIZE.1,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &[0; 4],
        // RGBA: WebGL2 takes no BGRA pixels for the zeros it starts with.
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    // Copied out by the talking-portrait check.
    image.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT | TextureUsages::COPY_SRC;
    images.add(image)
}

fn spawn(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let live = target_image(&mut images);
    commands.insert_resource(Stage {
        subject: None,
        unit: None,
        actor: None,
        stills: Vec::new(),
        backdrops: HashMap::new(),
        live: live.clone(),
        next_render: 0.0,
        render: false,
        rendered: false,
        forced: None,
        every: false,
    });
    commands.spawn((
        PortraitCamera,
        Camera3d::default(),
        Camera {
            order: -2,
            is_active: false,
            // The panel lays the render over the background.
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(ImageRenderTarget {
            handle: live,
            scale_factor: 1.0,
        }),
        Projection::Perspective(PerspectiveProjection::default()),
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
        AmbientLight::default(),
        Msaa::Sample4,
        RenderLayers::layer(LAYER),
        // Its light is directional: nothing to cluster.
        ClusterConfig::Single,
    ));
    commands.spawn((
        PortraitLight,
        // Directional, as a light only this view sees needs no clustering:
        // a view drawn now and then has no clusters of its own frames before.
        DirectionalLight {
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
        RenderLayers::layer(LAYER),
    ));
}

/// The model the panel's unit is drawn with, its scale, its hero and the unit.
fn subject(
    session: &Session,
    index: &UnitIndex,
    units: &Query<(&Unit, &ModelOf)>,
    children: &Query<&Children>,
    scenes: &Query<&Transform, With<WorldAssetRoot>>,
) -> Option<(usize, f32, Option<u16>, EntityId)> {
    let unit: &UnitView = crate::hud::subject(&session.0).1?;
    let (drawn, ModelOf(pivot)) = units.get(*index.0.get(&unit.id)?).ok()?;
    let size = children
        .get(*pivot)
        .ok()?
        .iter()
        .find_map(|k| scenes.get(k).ok())?
        .scale
        .x
        / crate::map::UNITS_PER_METRE;
    Some((drawn.model?, size, unit.hero.map(|h| h.0), unit.id))
}

/// Puts the panel's unit on the stage, or shows what is kept of it: Dota's
/// picture of a hero, or a still made before, unless portraits are animated.
fn stage(
    session: NonSend<Session>,
    index: Res<UnitIndex>,
    manifest: Res<Manifest>,
    settings: Res<Settings>,
    bank: Res<ModelBank>,
    mut stage: ResMut<Stage>,
    mut portrait: ResMut<Portrait>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
    units: Query<(&Unit, &ModelOf)>,
    children: Query<&Children>,
    scenes: Query<&Transform, With<WorldAssetRoot>>,
) {
    let animate = settings.animate_portrait || stage.forced.is_some();
    let shown = match stage.forced {
        Some(model) => Some((model, bank.0[model].model.scale, None, None)),
        None => subject(&session, &index, &units, &children, &scenes)
            .map(|(model, size, hero, unit)| (model, size, hero, Some(unit))),
    };
    let wanted = shown
        // Dota's picture of a hero stands for a still of its model.
        .filter(|(_, _, hero, _)| animate || hero.and_then(|h| manifest.portrait(h)).is_none());
    stage.subject = wanted.map(|(model, ..)| model);
    stage.unit = wanted.and_then(|(.., unit)| unit);
    let kept = wanted
        .filter(|_| !animate)
        .and_then(|(model, ..)| stage.stills.iter().find(|(m, _)| *m == model))
        .map(|(_, still)| still.clone());
    let want_actor = wanted
        .filter(|_| kept.is_none())
        .map(|(model, size, ..)| (model, size));
    let on_stage = stage
        .actor
        .as_ref()
        .map(|a| (a.model, a.still.is_none() == animate));
    if on_stage != want_actor.map(|(model, _)| (model, true)) {
        if let Some(actor) = stage.actor.take() {
            commands.entity(actor.pivot).despawn();
        }
        // A unit is drawn with a model only once its scenes are taken.
        if let Some((model, size)) = want_actor
            && let Some(scenes) = &bank.0[model].scenes
        {
            let still = (!animate).then(|| target_image(&mut images));
            stage.actor = Some(spawn_actor(&mut commands, scenes, model, size, still));
        }
    }
    let backdrop = stage
        .subject
        .and_then(|model| stage.backdrops.get(&bank.0[model].model.logical).cloned());
    let image = kept.or_else(|| {
        let actor = stage.actor.as_ref().filter(|a| a.posed)?;
        Some(actor.still.clone().unwrap_or_else(|| stage.live.clone()))
    });
    portrait.shown = image.map(|image| (image, backdrop));
}

/// A copy of a model of the bank on the stage, hidden until every part of it
/// is ready and on the stage's layer.
fn spawn_actor(
    commands: &mut Commands,
    scenes: &Scenes,
    model: usize,
    size: f32,
    still: Option<Handle<Image>>,
) -> Actor {
    let pivot = commands
        .spawn((
            Transform::from_translation(STAGE),
            Visibility::Hidden,
            RenderLayers::layer(LAYER),
        ))
        .id();
    let scale = Transform::from_scale(Vec3::splat(crate::map::UNITS_PER_METRE * size));
    let body = commands
        .spawn((
            WorldAssetRoot(scenes.body.clone()),
            scale,
            ChildOf(pivot),
            ActorPart { ready: false },
        ))
        .observe(on_part_ready)
        .id();
    for wearable in &scenes.wearables {
        commands
            .spawn((
                WorldAssetRoot(wearable.clone()),
                scale,
                ChildOf(pivot),
                ActorPart { ready: false },
                Wearable::on(body),
            ))
            .observe(on_part_ready);
    }
    Actor {
        model,
        pivot,
        size,
        posed: false,
        still,
        settle: SETTLE,
        face: None,
    }
}

/// Moves a ready part of the model on the stage onto the stage's layer.
fn on_part_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    children: Query<&Children>,
    mut parts: Query<&mut ActorPart>,
    mut wearables: Query<&mut Wearable>,
) {
    let root = ready.entity;
    for part in children.iter_descendants(root) {
        commands.entity(part).insert(RenderLayers::layer(LAYER));
    }
    commands.entity(root).insert(RenderLayers::layer(LAYER));
    if let Ok(mut part) = parts.get_mut(root) {
        part.ready = true;
    }
    match wearables.get_mut(root) {
        Ok(mut wearable) => wearable.mark_ready(),
        // The body: its wearables bind to its joints once it is posable.
        Err(_) => {
            Wearable::wearer_ready(root, &mut wearables);
            commands.entity(root).insert(Posable);
        }
    }
}

/// Starts the idle clip of the model on the stage once all of it is ready,
/// and shows it: frozen at its start for a still; an animated one with its
/// face, which plays the model's idle expression.
fn pose(
    mut stage: ResMut<Stage>,
    bank: Res<ModelBank>,
    args: Res<crate::Args>,
    mut faces: ResMut<Faces>,
    time: Res<Time<bevy::time::Real>>,
    children: Query<&Children>,
    parts: Query<&ActorPart>,
    wearing: Query<&Wearable>,
    meshes: Query<(&MeshMorphWeights, &GltfMeshName, &GltfMeshExtras)>,
    mut players: Query<&mut AnimationPlayer>,
    mut commands: Commands,
) {
    let Some(actor) = stage.actor.as_mut().filter(|a| !a.posed) else {
        return;
    };
    // Each part ready, and each wearable bound to the body's joints.
    let (any, ready) = children
        .iter_descendants(actor.pivot)
        .filter_map(|e| {
            Some(parts.get(e).ok()?.ready && wearing.get(e).ok().is_none_or(Wearable::bound))
        })
        .fold((false, true), |(_, all), one| (true, all && one));
    if !any || !ready {
        return;
    }
    let entry = &bank.0[actor.model];
    let player = children
        .iter_descendants(actor.pivot)
        .find(|e| players.contains(*e));
    let idle = entry.clips.iter().find(|(name, ..)| name == "idle");
    if let (Some(player), Some(graph), Some((_, node, ..))) = (player, &entry.graph, idle) {
        commands
            .entity(player)
            .insert(AnimationGraphHandle(graph.clone()));
        if let Ok(mut p) = players.get_mut(player) {
            let active = p.play(*node).repeat();
            if actor.still.is_some() {
                active.pause();
            }
        }
    }
    if actor.still.is_none() {
        let expression = faces.expression(&entry.model.logical);
        actor.face = Face::of(
            actor.pivot,
            expression,
            time.elapsed_secs(),
            &mut faces,
            &children,
            &meshes,
        );
        // Traced, the face holds its idle expression: what moves it is the line.
        if let Some(face) = actor.face.as_mut().filter(|_| args.face_trace) {
            face.frozen = Some(0.0);
        }
    }
    commands.entity(actor.pivot).insert(Visibility::Inherited);
    actor.posed = true;
}

/// Makes each model's background once its image has loaded: the image
/// tinted towards its corners' colours, as Dota tints it.
fn tint_backdrops(
    framings: Res<Framings>,
    bank: Res<ModelBank>,
    server: Res<AssetServer>,
    mut stage: ResMut<Stage>,
    mut images: ResMut<Assets<Image>>,
    mut asked: Local<HashMap<String, Handle<Image>>>,
) {
    let Some(model) = stage.subject else {
        return;
    };
    let logical = &bank.0[model].model.logical;
    if stage.backdrops.contains_key(logical) {
        return;
    }
    let Some((path, tints)) = framings.0.get(logical).and_then(|f| f.background.as_ref()) else {
        return;
    };
    let source = asked
        .entry(path.clone())
        .or_insert_with(|| server.load(crate::manifest::cache_path(path)))
        .clone();
    let Some(tinted) = images.get(&source).and_then(|image| tint(image, tints)) else {
        return;
    };
    let handle = images.add(tinted);
    stage.backdrops.insert(logical.clone(), handle);
}

/// An image multiplied by the colours of its corners, blended across it.
fn tint(image: &Image, corners: &[Vec3; 4]) -> Option<Image> {
    let (w, h) = (image.width(), image.height());
    let data = image.data.as_ref()?;
    if !matches!(
        image.texture_descriptor.format,
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Rgba8Unorm
    ) || data.len() != (w * h * 4) as usize
    {
        return None;
    }
    let [tl, tr, br, bl] = *corners;
    let mut out = data.clone();
    for (i, px) in out.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = ((i as u32 % w) as f32, (i as u32 / w) as f32);
        let (u, v) = (x / (w - 1).max(1) as f32, y / (h - 1).max(1) as f32);
        let c = tl.lerp(tr, u).lerp(bl.lerp(br, u), v);
        for (channel, k) in px.iter_mut().zip([c.x, c.y, c.z]) {
            *channel = (f32::from(*channel) * k).round().clamp(0.0, 255.0) as u8;
        }
    }
    Some(Image::new(
        Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        out,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    ))
}

/// Aims the camera and its light at the model on the stage, and lets it
/// draw as [`draw`] decides.
fn aim(
    framings: Res<Framings>,
    bank: Res<ModelBank>,
    ready: Res<crate::warmup::PipelinesReady>,
    time: Res<Time<bevy::time::Real>>,
    mut stage: ResMut<Stage>,
    mut commands: Commands,
    children: Query<&Children>,
    bounds: Query<(&Aabb, &GlobalTransform)>,
    mut camera: Single<
        (
            &mut Camera,
            &mut RenderTarget,
            &mut Transform,
            &mut Projection,
            &mut AmbientLight,
        ),
        (With<PortraitCamera>, Without<PortraitLight>),
    >,
    mut light: Single<(&mut DirectionalLight, &mut Transform), With<PortraitLight>>,
) {
    let placed = stage.actor.as_ref().filter(|a| a.posed).and_then(|actor| {
        let framed = framings.0.get(&bank.0[actor.model].model.logical);
        framed.map(|f| place(f, actor.size)).or_else(|| {
            let (center, radius) = bounds_in(actor.pivot, &children, &bounds)?;
            Some(fit(center, radius))
        })
    });
    let live = stage.live.clone();
    // Off while the main world works out what views see (see `activate`).
    if camera.0.is_active {
        camera.0.is_active = false;
    }
    let (Some(placed), Some(actor)) = (placed, stage.actor.as_mut()) else {
        stage.render = false;
        stage.rendered = false;
        return;
    };
    let at = |local: &Transform| Transform::from_translation(STAGE) * *local;
    *camera.2 = at(&placed.camera);
    *camera.3 = Projection::Perspective(PerspectiveProjection {
        fov: placed.vfov,
        aspect_ratio: SIZE.0 as f32 / SIZE.1 as f32,
        near: 4.0,
        far: placed.far,
        ..default()
    });
    camera.4.color = placed.ambient;
    camera.4.brightness = placed.ambient_brightness;
    *light.1 = at(&placed.light.0);
    light.0.color = placed.light.1;
    light.0.illuminance = placed.lux;
    let target = actor.still.clone().unwrap_or(live);
    if camera.1.as_image() != Some(&target) {
        *camera.1 = RenderTarget::Image(ImageRenderTarget {
            handle: target,
            scale_factor: 1.0,
        });
    }
    let ready = ready.get();
    stage.rendered = stage.render;
    stage.render = draw(&mut stage, ready, time.elapsed_secs(), &mut commands);
}

/// Whether the camera draws this frame: a still until its render is whole
/// (then it is kept and its model leaves the stage), an animated portrait
/// at most [`RATE`] times a second.
fn draw(stage: &mut Stage, ready: bool, now: f32, commands: &mut Commands) -> bool {
    let rendered = stage.rendered;
    let Some(actor) = stage.actor.as_mut() else {
        return false;
    };
    let Some(still) = actor.still.clone() else {
        let render = stage.every || now >= stage.next_render;
        if render {
            stage.next_render = (stage.next_render + 1.0 / RATE).max(now);
        }
        return render;
    };
    // The render drawn last frame counts once nothing was compiling.
    if rendered && ready {
        actor.settle = actor.settle.saturating_sub(1);
    }
    let render = actor.settle > 0;
    if actor.settle == 0 {
        let (model, pivot) = (actor.model, actor.pivot);
        commands.entity(pivot).despawn();
        stage.actor = None;
        if stage.stills.len() >= STILLS {
            stage.stills.remove(0);
        }
        stage.stills.push((model, still));
    }
    render
}

/// Moves the face on the stage, on the frames the camera draws: its idle
/// expression, and the line its hero is saying.
fn speak(
    stage: Res<Stage>,
    faces: Res<Faces>,
    speech: Res<Speech>,
    time: Res<Time<bevy::time::Real>>,
    mut weights: Query<&mut MorphWeights>,
) {
    let Some(face) = stage.actor.as_ref().and_then(|a| a.face.as_ref()) else {
        return;
    };
    if stage.render {
        let now = time.elapsed_secs();
        face.pose(now, speech.of(stage.unit, now), &faces, &mut weights);
    }
}

/// Marks the model on the stage and its light visible on the frames the
/// camera draws: no view the main world works out visibility for sees them,
/// the portrait's camera being off by then.
fn on_stage(
    stage: Res<Stage>,
    tree: Query<&Children>,
    mut seen: Query<(&InheritedVisibility, &mut ViewVisibility)>,
    light: Single<Entity, With<PortraitLight>>,
) {
    let Some(actor) = stage.actor.as_ref().filter(|a| a.posed && stage.render) else {
        return;
    };
    let parts = tree.iter_descendants(actor.pivot).chain([*light]);
    for e in parts {
        if let Ok((inherited, mut view)) = seen.get_mut(e)
            && inherited.get()
        {
            view.set_visible();
        }
    }
}

/// Switches the camera on for the frames it draws, after the main world's
/// visibility and the sun's cascades: what it sees is the stage alone.
fn activate(
    stage: Res<Stage>,
    tree: Query<&Children>,
    classes: Query<(&VisibilityClass, &InheritedVisibility)>,
    light: Single<Entity, With<PortraitLight>>,
    mut camera: Single<(&mut Camera, &mut VisibleEntities), With<PortraitCamera>>,
) {
    let actor = stage.actor.as_ref().filter(|_| stage.render);
    if camera.0.is_active != actor.is_some() {
        camera.0.is_active = actor.is_some();
    }
    let Some(actor) = actor else {
        return;
    };
    let seen = &mut camera.1;
    seen.clear_all();
    for e in tree.iter_descendants(actor.pivot).chain([*light]) {
        if let Ok((class, inherited)) = classes.get(e)
            && inherited.get()
        {
            for kind in class.iter() {
                seen.get_mut(*kind).push(e);
            }
        }
    }
}

/// Where the camera and the light stand, in the stage's space.
struct Placed {
    camera: Transform,
    vfov: f32,
    far: f32,
    ambient: Color,
    ambient_brightness: f32,
    /// Lux of the light at the model.
    lux: f32,
    /// The light's direction and colour.
    light: (Transform, Color),
}

/// Dota's framing, scaled with the model as it is drawn. Its field of view is
/// the picture's height.
fn place(f: &Framing, size: f32) -> Placed {
    let camera = to_scene(f.camera) * size;
    let ahead = to_scene(look_dir(f.angles));
    Placed {
        camera: Transform::from_translation(camera).looking_to(ahead, Vec3::Y),
        vfov: f.fov.to_radians(),
        far: f.far * size.max(1.0),
        ambient: Color::srgb(f.ambient.x, f.ambient.y, f.ambient.z),
        ambient_brightness: AMBIENT * f.ambient_scale,
        lux: LIGHT_LUX * f.light_scale,
        light: (
            Transform::default().looking_to(to_scene(look_dir(f.light_angles)), Vec3::Y),
            Color::srgb(f.light_color.x, f.light_color.y, f.light_color.z),
        ),
    }
}

/// A framing from the model's bounds: the centre and the radius of its sphere.
fn fit(center: Vec3, radius: f32) -> Placed {
    let vfov = 30f32.to_radians();
    let distance = radius / (vfov / 2.0).sin() * 1.1;
    let at = center + BOUNDS_VIEW.normalize() * distance;
    Placed {
        camera: Transform::from_translation(at).looking_at(center, Vec3::Y),
        vfov,
        far: distance + radius * 4.0,
        ambient: Color::srgb(0.7, 0.75, 0.85),
        // A creep's light as Dota frames one.
        ambient_brightness: AMBIENT * 8.0,
        lux: LIGHT_LUX * 2.5,
        light: (
            Transform::default().looking_to(-Vec3::new(-0.6, 1.0, -0.8), Vec3::Y),
            Color::srgb(1.0, 0.97, 0.92),
        ),
    }
}

/// The sphere round the parts of the model on the stage that have bounds, in
/// the stage's space.
fn bounds_in(
    pivot: Entity,
    children: &Query<&Children>,
    bounds: &Query<(&Aabb, &GlobalTransform)>,
) -> Option<(Vec3, f32)> {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for part in children.iter_descendants(pivot) {
        let Ok((aabb, at)) = bounds.get(part) else {
            continue;
        };
        let (c, h) = (Vec3::from(aabb.center), Vec3::from(aabb.half_extents));
        for corner in 0..8u8 {
            let sign = |bit: u8| if corner & bit == 0 { -1.0 } else { 1.0 };
            let p = c + h * Vec3::new(sign(1), sign(2), sign(4));
            let p = at.affine().transform_point3(p) - STAGE;
            lo = lo.min(p);
            hi = hi.max(p);
        }
    }
    (lo.x <= hi.x).then(|| ((lo + hi) / 2.0, ((hi - lo) / 2.0).length().max(8.0)))
}

/// Debug: selects the first unit shown that `--select` names, until one is.
fn select_named(args: Res<crate::Args>, mut session: NonSendMut<Session>, units: Query<&Unit>) {
    let Some(want) = args.select.as_deref() else {
        return;
    };
    let (name, side) = want.split_once('@').unwrap_or((want, ""));
    let core = &mut session.0;
    let (Some(view), Some(map)) = (core.view(), core.session().map(ClientSession::map)) else {
        return;
    };
    let found = units.iter().find_map(|u| {
        let live = view.unit(u.id)?;
        let side_ok = side.is_empty() || format!("{:?}", u.team).eq_ignore_ascii_case(side);
        let named = crate::units::unit_name(map, live).eq_ignore_ascii_case(name);
        (side_ok && named).then_some(u.id)
    });
    if found.is_some() && core.selected != found {
        core.selected = found;
    }
}

#[cfg(test)]
mod tests {
    use super::parse_line;

    /// The importer's line for Shadow Fiend, as `dbota import` writes it.
    const NEVERMORE: &str = "hero.nevermore.model 126.07 -14.4 175.18 1.85 172.19 0 33 10000 85 9.68 296.06 \
        62.94 185.36 0 75 243 243 243 131 0 25 1 2.5 0 0 0 0 0 0 0.8 0.2 0.1 0 0 0 \
        models/portrait_backgrounds/portraitbackground_desert.png";

    #[test]
    fn the_importers_line_reads_whole() {
        let (logical, f) = parse_line(NEVERMORE).unwrap();
        assert_eq!(logical, "hero.nevermore.model");
        assert_eq!((f.fov, f.light_scale, f.ambient_scale), (33.0, 1.0, 2.5));
        let (path, tints) = f.background.unwrap();
        assert_eq!(
            path,
            "models/portrait_backgrounds/portraitbackground_desert.png"
        );
        assert_eq!(tints[2].to_array(), [0.8, 0.2, 0.1], "bottom right is red");
        let plain = NEVERMORE.replace(
            "models/portrait_backgrounds/portraitbackground_desert.png",
            "-",
        );
        assert!(parse_line(&plain).unwrap().1.background.is_none());
        let short: Vec<&str> = NEVERMORE.split_whitespace().take(20).collect();
        assert!(
            parse_line(&short.join(" ")).is_none(),
            "a cut line is no framing"
        );
    }
}
