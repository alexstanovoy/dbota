//! The last played hero, on a transparent stage between Home's landscape and its panels.

use bevy::{
    camera::{
        ImageRenderTarget, RenderTarget,
        visibility::{
            RenderLayers, VisibilityClass, VisibilitySystems, VisibleEntities,
            check_visibility_cpu_culling,
        },
    },
    ecs::system::SystemParam,
    gltf::{GltfMeshExtras, GltfMeshName},
    light::{AmbientLight, SimulationLightSystems, cluster::ClusterConfig},
    mesh::morph::{MeshMorphWeights, MorphWeights},
    mesh::skinning::{SkinnedMesh, SkinnedMeshInverseBindposes},
    prelude::*,
};

use crate::{
    manifest::Manifest,
    portrait::{self, Actor, ActorPart},
    talk::Faces,
    units::{ModelBank, Wearable},
};

const LAYER: usize = 3;
const SIZE: (u32, u32) = (1040, 720);
const HEIGHT: f32 = 520.0;
const GROUND: f32 = 770.0;
const TOP: f32 = 100.0;
const FOV: f32 = 25.0_f32.to_radians();
const RATE: f32 = 30.0;
const SAMPLES: usize = 24;
// Presentation clearance above the common contact-shadow line.
const HOVER: &[(bota_proto::HeroId, f32)] = &[(bota_proto::HeroId::SHADOW_FIEND, 32.0)];

#[derive(Component)]
pub(crate) struct HeroImage(bota_proto::HeroId);
/// Marks the hero's picture a screen in front of the menu shows (the strategy screen's): the
/// stage renders its hero, not the hidden menu page's.
#[derive(Component)]
pub(crate) struct Front;
#[derive(Component)]
pub(crate) struct ContactShadow;
/// Where a contact shadow stands when it is not the Home page's: the stage's picture scaled by
/// `scale`, the shadow's middle `centre` pixels from the middle of its place across.
#[derive(Component)]
pub(crate) struct StageShadow {
    pub scale: f32,
    pub centre: f32,
}
#[derive(Component)]
pub(crate) struct HeroCamera;
#[derive(Component)]
struct HeroLight;
#[derive(Component)]
pub(crate) struct StageEntity;

#[derive(Resource, Default)]
pub(crate) struct HeroStage {
    pub(crate) hero: Option<bota_proto::HeroId>,
    pub(crate) actor: Option<Actor>,
    model: Option<usize>,
    image: Handle<Image>,
    next_render: f32,
    framed: bool,
    failed: bool,
    samples: usize,
    envelope: Vec<Vec3>,
    reference: Vec<Vec3>,
    warmup: u8,
    render: bool,
    /// Whether the stage was made for a screen of its own (lit stronger).
    lit: bool,
}

impl HeroStage {
    pub(crate) fn ready(&self) -> bool {
        self.framed && self.actor.as_ref().is_some_and(|a| a.posed)
    }
}

pub(super) fn draw(kit: &mut super::kit::Kit, root: Entity, hero: bota_proto::HeroId) {
    kit.commands.spawn((
        ContactShadow,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            margin: UiRect::left(px(-310)),
            top: px(GROUND - 24.0),
            width: px(220),
            height: px(48),
            ..default()
        },
        crate::theme::hero_contact_shadow(),
        Pickable::IGNORE,
        Visibility::Hidden,
        Name::new("home contact shadow"),
        ChildOf(root),
    ));
    kit.commands.spawn((
        HeroImage(hero),
        ImageNode::default(),
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            margin: UiRect::left(px(-720)),
            top: px(TOP),
            width: px(SIZE.0 as f32),
            height: px(SIZE.1 as f32),
            ..default()
        },
        Pickable::IGNORE,
        Visibility::Hidden,
        Name::new("home hero"),
        ChildOf(root),
    ));
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<HeroStage>()
        .add_systems(
            Update,
            (
                select.run_if(has_home_or_actor),
                pose.run_if(has_actor),
                sample_idle.run_if(has_actor),
            )
                .chain()
                .after(super::screens::draw),
        )
        .add_systems(
            PostUpdate,
            (
                frame
                    .after(bevy::transform::TransformSystems::Propagate)
                    .before(bevy::camera::CameraUpdateSystems),
                visible
                    .in_set(VisibilitySystems::CheckVisibility)
                    .after(check_visibility_cpu_culling),
                activate
                    .after(VisibilitySystems::CheckVisibility)
                    .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                    .before(SimulationLightSystems::AssignLightsToClusters)
                    .before(SimulationLightSystems::UpdateLightFrusta),
            )
                .run_if(has_actor),
        );
}

fn has_actor(stage: Res<HeroStage>) -> bool {
    stage.actor.is_some()
}
fn has_home_or_actor(stage: Res<HeroStage>, nodes: Query<(), With<HeroImage>>) -> bool {
    stage.actor.is_some() || !nodes.is_empty()
}

fn select(
    mut commands: Commands,
    mut stage: ResMut<HeroStage>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<(&HeroImage, &mut ImageNode, &mut Visibility, Has<Front>)>,
    mut shadows: Query<&mut Visibility, (With<ContactShadow>, Without<HeroImage>)>,
    mut bank: ResMut<ModelBank>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    cameras: Query<Entity, With<StageEntity>>,
) {
    let hero = nodes
        .iter()
        .find(|(.., front)| *front)
        .or_else(|| nodes.iter().next())
        .map(|(hero, ..)| hero.0);
    let front = nodes.iter().any(|(.., front)| front);
    if hero != stage.hero || hero.is_none() || front != stage.lit {
        if let Some(actor) = stage.actor.take() {
            commands.entity(actor.pivot).despawn();
        }
        for camera in &cameras {
            commands.entity(camera).despawn();
        }
        stage.image = Handle::default();
        stage.framed = false;
        stage.failed = false;
        stage.samples = 0;
        stage.envelope = Vec::new();
        stage.reference = Vec::new();
        stage.warmup = 0;
        stage.hero = hero;
        stage.lit = front;
        stage.model = hero.and_then(|hero| {
            let name = manifest.dota_name("hero", hero.0)?;
            bank.request(&format!("hero.{name}.model"), &server, &manifest)
        });
    }
    for (_, mut node, mut shown, _) in &mut nodes {
        shown.set_if_neq(crate::ui::visible(stage.ready()));
        if node.image != stage.image {
            node.image = stage.image.clone();
        }
    }
    for mut shown in &mut shadows {
        shown.set_if_neq(crate::ui::visible(stage.ready()));
    }
    if stage.actor.is_some() || stage.failed {
        return;
    }
    let Some(model) = stage.model else {
        return;
    };
    let entry = &bank.0[model];
    if entry.ready()
        && let Some(scenes) = &entry.scenes
    {
        stage.image = portrait::stage_image(&mut images, SIZE);
        stage.actor = Some(portrait::spawn_actor(
            &mut commands,
            scenes,
            model,
            entry.model.scale,
            None,
            LAYER,
        ));
        spawn_camera(&mut commands, stage.image.clone(), front);
    }
}

fn spawn_camera(commands: &mut Commands, image: Handle<Image>, lit: bool) {
    // A screen's own stage (the strategy screen's room) is lit stronger, and from above too.
    let gain = if lit { 1.7 } else { 1.0 };
    commands.spawn((
        StageEntity,
        HeroCamera,
        Camera3d::default(),
        Camera {
            order: -3,
            is_active: false,
            clear_color: ClearColorConfig::Custom(Color::NONE),
            ..default()
        },
        RenderTarget::Image(ImageRenderTarget {
            handle: image,
            scale_factor: 1.0,
        }),
        Projection::Perspective(PerspectiveProjection {
            fov: FOV,
            aspect_ratio: SIZE.0 as f32 / SIZE.1 as f32,
            near: 0.1,
            far: 10_000.0,
            ..default()
        }),
        AmbientLight {
            color: Color::srgb(0.65, 0.75, 0.9),
            brightness: 450.0 * gain,
            ..default()
        },
        RenderLayers::layer(LAYER),
        ClusterConfig::Single,
        Msaa::Sample4,
        bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface,
    ));
    let top = [(
        Vec3::new(0.05, -1.0, 0.15),
        Color::srgb(0.95, 0.95, 1.0),
        6000.0,
    )];
    for (direction, color, illuminance) in [
        (
            Vec3::new(0.6, -1.0, 0.8),
            Color::srgb(0.75, 0.85, 1.0),
            7000.0 * gain,
        ),
        (
            Vec3::new(-0.5, -0.7, -1.0),
            Color::srgb(0.65, 0.8, 1.0),
            3500.0 * gain,
        ),
    ]
    .into_iter()
    .chain(top.into_iter().filter(|_| lit))
    {
        commands.spawn((
            StageEntity,
            HeroLight,
            DirectionalLight {
                color,
                illuminance,
                shadow_maps_enabled: false,
                ..default()
            },
            Transform::default().looking_to(direction, Vec3::Y),
            RenderLayers::layer(LAYER),
        ));
    }
}

fn pose(
    mut stage: ResMut<HeroStage>,
    bank: Res<ModelBank>,
    args: Res<crate::Args>,
    manifest: Res<Manifest>,
    mut faces: ResMut<Faces>,
    time: Res<Time<bevy::time::Real>>,
    children: Query<&Children>,
    parts: Query<&ActorPart>,
    wearing: Query<&Wearable>,
    meshes: Query<(&MeshMorphWeights, &GltfMeshName, &GltfMeshExtras)>,
    materials: Query<&MeshMaterial3d<StandardMaterial>>,
    mut standard: ResMut<Assets<StandardMaterial>>,
    mut players: Query<&mut AnimationPlayer>,
    mut commands: Commands,
) {
    if let Some(actor) = stage.actor.as_mut().filter(|a| !a.posed) {
        portrait::pose_actor(
            actor,
            &bank,
            &manifest,
            &args,
            &mut faces,
            &time,
            &children,
            &parts,
            &wearing,
            &meshes,
            &mut players,
            &mut commands,
        );
        if actor.posed {
            for part in children.iter_descendants(actor.pivot) {
                if let Ok(material) = materials.get(part) {
                    crate::units::lift_hero_material(&material.0, &mut standard);
                }
            }
        }
    }
}

fn frame(
    mut stage: ResMut<HeroStage>,
    time: Res<Time<bevy::time::Real>>,
    geometry: ModelGeometry,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<HeroCamera>>,
    mut shadows: Query<(&mut Node, Option<&StageShadow>), With<ContactShadow>>,
    faces: Res<Faces>,
    mut weights: Query<&mut MorphWeights>,
    mut commands: Commands,
    entities: Query<Entity, With<StageEntity>>,
    mut players: Query<&mut AnimationPlayer>,
) {
    stage.render = false;
    for (mut camera, mut at, mut projection) in &mut cameras {
        camera.is_active = false;
        if !stage.framed {
            let prepared = match prepare(&mut stage, &geometry) {
                Ok(Some(prepared)) => prepared,
                Ok(None) => continue,
                Err(reason) => {
                    disable(&mut stage, &mut commands, &entities, reason);
                    continue;
                }
            };
            *at = prepared.camera;
            if let Projection::Perspective(p) = &mut *projection {
                p.far = prepared.far;
            }
            for (mut shadow, placed) in &mut shadows {
                let (scale, centre) = placed.map_or((1.0, -200.0), |p| (p.scale, p.centre));
                shadow.width = px(prepared.shadow * scale);
                shadow.margin.left = px(centre - prepared.shadow * scale / 2.0);
            }
            stage.framed = true;
            if let Some((player, node, _)) = prepared.idle
                && let Ok(mut player) = players.get_mut(player)
                && let Some(active) = player.animation_mut(node)
            {
                active.set_seek_time(0.0).resume();
            }
        }
        let now = time.elapsed_secs();
        if now < stage.next_render {
            continue;
        }
        stage.next_render = now + 1.0 / RATE;
        stage.render = true;
        if let Some(face) = stage.actor.as_ref().and_then(|a| a.face.as_ref()) {
            face.pose(now, None, &faces, &mut weights);
        }
    }
}

struct Prepared {
    camera: Transform,
    far: f32,
    shadow: f32,
    idle: Option<(Entity, AnimationNodeIndex, f32)>,
}

fn prepare(
    stage: &mut HeroStage,
    geometry: &ModelGeometry,
) -> Result<Option<Prepared>, &'static str> {
    let Some(actor) = stage.actor.as_ref().filter(|actor| actor.posed) else {
        return Ok(None);
    };
    let (pivot, idle) = (actor.pivot, actor.idle);
    // The new animation graph needs a frame to replace the scene's bind pose.
    if stage.warmup < 2 {
        stage.warmup += 1;
        return Ok(None);
    }
    let points = geometry
        .points(pivot)
        .ok_or("missing or invalid framing geometry")?;
    if stage.envelope.len() + points.len() > 2 << 20 {
        return Err("idle framing budget exceeded");
    }
    if stage.samples == 0 {
        stage.reference = points.clone();
    }
    stage.envelope.extend(points);
    stage.samples += 1;
    let samples = if idle.is_some_and(|(_, _, duration)| duration > 0.0) {
        SAMPLES
    } else {
        1
    };
    if stage.samples < samples {
        return Ok(None);
    }
    let points = std::mem::take(&mut stage.envelope);
    let reference = std::mem::take(&mut stage.reference);
    let (center, half) = point_bounds(&points);
    debug!(hero = ?stage.hero, vertices = reference.len(), sampled_vertices = points.len(), "Home idle framing ready");
    // A hero on a screen's own stage stands on its floor; Home's hover above the shadow's line.
    let hover = HOVER
        .iter()
        .find(|(hero, _)| Some(*hero) == stage.hero && !stage.lit)
        .map_or(0.0, |(_, hover)| *hover);
    let camera = framing(center, half, &reference, &points, hover);
    Ok(Some(Prepared {
        far: (camera.translation - portrait::STAGE).length() + half.length() * 4.0,
        shadow: stance_width(&camera, &reference),
        camera,
        idle,
    }))
}

fn disable(
    stage: &mut HeroStage,
    commands: &mut Commands,
    entities: &Query<Entity, With<StageEntity>>,
    reason: &'static str,
) {
    warn!(hero = ?stage.hero, reason, "Home hero stage disabled");
    if let Some(actor) = stage.actor.take() {
        commands.entity(actor.pivot).despawn();
    }
    for entity in entities {
        commands.entity(entity).despawn();
    }
    stage.failed = true;
    stage.image = Handle::default();
    stage.envelope = Vec::new();
    stage.reference = Vec::new();
}

/// Fit a whole idle cycle while the image is still hidden, then let that same clip loop.
fn sample_idle(stage: Res<HeroStage>, mut players: Query<&mut AnimationPlayer>) {
    if stage.framed {
        return;
    }
    if let Some((player, node, duration)) = stage
        .actor
        .as_ref()
        .filter(|a| a.posed)
        .and_then(|a| a.idle)
        && let Ok(mut player) = players.get_mut(player)
        && let Some(active) = player.animation_mut(node)
    {
        active
            .pause()
            .set_seek_time(duration * stage.samples as f32 / SAMPLES as f32);
    }
}

fn visible(
    stage: Res<HeroStage>,
    children: Query<&Children>,
    mut seen: Query<(&InheritedVisibility, &mut ViewVisibility)>,
    lights: Query<Entity, With<HeroLight>>,
) {
    if let Some(actor) = stage.actor.as_ref().filter(|_| stage.render) {
        portrait::show_actor(actor.pivot, lights.iter(), &children, &mut seen);
    }
}

fn activate(
    stage: Res<HeroStage>,
    children: Query<&Children>,
    classes: Query<(&VisibilityClass, &InheritedVisibility)>,
    lights: Query<Entity, With<HeroLight>>,
    mut cameras: Query<(&mut Camera, &mut VisibleEntities), With<HeroCamera>>,
) {
    if let Ok((mut camera, mut seen)) = cameras.single_mut() {
        portrait::camera_actor(
            stage
                .actor
                .as_ref()
                .filter(|_| stage.render)
                .map(|a| a.pivot),
            lights.iter(),
            &children,
            &classes,
            &mut camera,
            &mut seen,
        );
    }
}

/// The resting pose sets the size; the complete idle envelope sets the limits. A common
/// screen-space ground line keeps differently proportioned models from jumping between games.
fn framing(center: Vec3, half: Vec3, reference: &[Vec3], points: &[Vec3], hover: f32) -> Transform {
    let (yaw, pitch) = (22.5_f32.to_radians(), 10.0_f32.to_radians());
    let from = Vec3::new(
        yaw.sin() * pitch.cos(),
        pitch.sin(),
        -yaw.cos() * pitch.cos(),
    );
    let rotation = Transform::default().looking_to(-from, Vec3::Y).rotation;
    let (right, up) = (rotation * Vec3::X, rotation * Vec3::Y);
    let aspect = SIZE.0 as f32 / SIZE.1 as f32;
    let tan = (FOV / 2.0).tan();
    let ground_y = 1.0 - 2.0 * (GROUND - hover - TOP) / SIZE.1 as f32;
    let mut distance = (half.y.max(8.0) / tan * SIZE.1 as f32 / HEIGHT).max(half.length() * 1.1);
    let mut offset = Vec2::ZERO;
    let mut camera = Transform::IDENTITY;
    for _ in 0..12 {
        camera = Transform {
            translation: center + from * distance + right * offset.x + up * offset.y,
            rotation,
            ..default()
        };
        let matrix =
            bevy::math::proj::perspective(FOV, aspect, 0.1, distance + half.length() * 4.0)
                * Mat4::from(camera.compute_affine().inverse());
        let (lo, hi) = projected_bounds(matrix, points);
        let (reference_lo, reference_hi) = projected_bounds(matrix, reference);
        let height = (reference_hi.y - reference_lo.y) * SIZE.1 as f32 / 2.0;
        offset.x += (reference_lo.x + reference_hi.x) / 2.0 * distance * tan * aspect;
        offset.y += (lo.y - ground_y) * distance * tan;
        distance *= (height / HEIGHT)
            .max(lo.x.abs().max(hi.x.abs()) / 0.95)
            .max((hi.y - lo.y) / (0.95 - ground_y));
        distance = distance.max((half.length() * 1.1).max(1.0));
    }
    #[cfg(test)]
    {
        let matrix =
            bevy::math::proj::perspective(FOV, aspect, 0.1, distance + half.length() * 4.0)
                * Mat4::from(camera.compute_affine().inverse());
        let (lo, hi) = projected_bounds(matrix, points);
        let (a, b) = projected_bounds(matrix, reference);
        println!(
            "Framing: {} vertices, rest height {}, idle span {} by {}",
            reference.len(),
            (b.y - a.y) * SIZE.1 as f32 / 2.0,
            (hi.x - lo.x) * SIZE.0 as f32 / 2.0,
            (hi.y - lo.y) * SIZE.1 as f32 / 2.0
        );
    }
    camera.translation += portrait::STAGE;
    camera
}

fn projected_bounds(matrix: Mat4, points: &[Vec3]) -> (Vec2, Vec2) {
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for point in points {
        let point = matrix.project_point3(*point).truncate();
        lo = lo.min(point);
        hi = hi.max(point);
    }
    (lo, hi)
}

fn point_bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for point in points {
        lo = lo.min(*point);
        hi = hi.max(*point);
    }
    ((lo + hi) / 2.0, (hi - lo) / 2.0)
}

fn stance_width(camera: &Transform, points: &[Vec3]) -> f32 {
    let (center, half) = point_bounds(points);
    let cutoff = center.y - half.y + half.y * 0.16;
    let mut camera = *camera;
    camera.translation -= portrait::STAGE;
    let matrix = bevy::math::proj::perspective(
        FOV,
        SIZE.0 as f32 / SIZE.1 as f32,
        0.1,
        camera.translation.length() + half.length() * 4.0,
    ) * Mat4::from(camera.compute_affine().inverse());
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for point in points.iter().filter(|point| point.y <= cutoff) {
        let x = matrix.project_point3(*point).x;
        lo = lo.min(x);
        hi = hi.max(x);
    }
    ((hi - lo) * SIZE.0 as f32 / 2.0 * 1.4).clamp(120.0, 500.0)
}

#[derive(SystemParam)]
pub(crate) struct ModelGeometry<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    instances: Query<
        'w,
        's,
        (
            &'static Mesh3d,
            &'static GlobalTransform,
            Option<&'static SkinnedMesh>,
        ),
    >,
    joints: Query<'w, 's, &'static GlobalTransform>,
    meshes: Res<'w, crate::units::GeometryBank>,
    bindposes: Res<'w, Assets<SkinnedMeshInverseBindposes>>,
}

impl ModelGeometry<'_, '_> {
    /// Pose the retained hero vertices once, using the same joints and inverse bind poses the
    /// renderer uses. Conservative culling boxes cannot provide a consistent visual height.
    pub(crate) fn points(&self, pivot: Entity) -> Option<Vec<Vec3>> {
        let mut points = Vec::new();
        for part in self.children.iter_descendants(pivot) {
            let Ok((handle, transform, skin)) = self.instances.get(part) else {
                continue;
            };
            let mesh = self.meshes.meshes.get(&handle.id())?.as_ref()?;
            let positions = &mesh.positions;
            if points.len().checked_add(positions.len())? > 1 << 20 {
                return None;
            }
            let Some(skin) = skin else {
                points.extend(positions.iter().map(|p| {
                    transform.affine().transform_point3(Vec3::from_array(*p)) - portrait::STAGE
                }));
                continue;
            };
            if skin.joints.len() > 4096 {
                return None;
            }
            let inverse = self.bindposes.get(&skin.inverse_bindposes)?;
            let matrices = skin
                .joints
                .iter()
                .zip(inverse.iter())
                .map(|(joint, bindpose)| {
                    let mut matrix = Mat4::from(self.joints.get(*joint).ok()?.affine()) * *bindpose;
                    matrix.w_axis -= portrait::STAGE.extend(0.0);
                    Some(matrix)
                })
                .collect::<Option<Vec<_>>>()?;
            let (indices, weights) = mesh.skin.as_ref()?;
            for (i, position) in positions.iter().enumerate() {
                let (indices, weights) = (indices.get(i)?, weights.get(i)?);
                let mut point = Vec3::ZERO;
                let mut total = 0.0;
                for (&joint, &weight) in indices.iter().zip(weights) {
                    if weight == 0.0 {
                        continue;
                    }
                    point += (*matrices.get(usize::from(joint))?
                        * Vec3::from_array(*position).extend(1.0))
                    .truncate()
                        * weight;
                    total += weight;
                }
                if total <= 0.0 {
                    return None;
                }
                points.push(point / total);
            }
        }
        if points.is_empty()
            || !points
                .iter()
                .all(|p| p.is_finite() && p.abs().max_element() <= 1_000_000.0)
        {
            return None;
        }
        (point_bounds(&points).1.y > 1e-3).then_some(points)
    }
}

/// Where a hero stands on a screen: the middle of its feet across the screen as a share of its
/// width, the line its feet stand on and its height, in interface pixels.
pub(crate) struct Stand {
    pub middle: f32,
    pub feet: f32,
    pub height: f32,
}

/// Puts the stage's hero on a screen of its own, standing as `stand` says, in front of the
/// menu's pages: the picture the stage renders, and the shadow under it. The picture's frame is
/// the stage's ([`SIZE`]) scaled so that the hero is as tall as asked.
pub(crate) fn stand(
    commands: &mut Commands,
    root: Entity,
    hero: bota_proto::HeroId,
    stand: &Stand,
) {
    let scale = stand.height / HEIGHT;
    let top = stand.feet - (GROUND - TOP) * scale;
    let (width, height) = (SIZE.0 as f32 * scale, SIZE.1 as f32 * scale);
    let left = |across: f32| Node {
        position_type: PositionType::Absolute,
        left: percent(stand.middle * 100.0),
        margin: UiRect::left(px(-across / 2.0)),
        ..default()
    };
    commands.spawn((
        ContactShadow,
        StageShadow { scale, centre: 0.0 },
        Node {
            top: px(top + (GROUND - 24.0 - TOP) * scale),
            width: px(220.0 * scale),
            height: px(48.0 * scale),
            ..left(220.0 * scale)
        },
        crate::theme::hero_contact_shadow(),
        Pickable::IGNORE,
        Visibility::Hidden,
        Name::new("strategy hero shadow"),
        ChildOf(root),
    ));
    commands.spawn((
        HeroImage(hero),
        Front,
        ImageNode::default(),
        Node {
            top: px(top),
            width: px(width),
            height: px(height),
            ..left(width)
        },
        Pickable::IGNORE,
        Visibility::Hidden,
        Name::new("strategy hero"),
        ChildOf(root),
    ));
}
