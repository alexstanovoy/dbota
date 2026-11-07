//! The last played hero, on a transparent stage between Home's landscape and its panels.

use bevy::{
    camera::{ImageRenderTarget, RenderTarget, primitives::Aabb, visibility::{RenderLayers, VisibilitySystems, VisibilityClass, VisibleEntities, check_visibility_cpu_culling}},
    gltf::{GltfMeshExtras, GltfMeshName},
    light::{AmbientLight, SimulationLightSystems, cluster::ClusterConfig},
    mesh::morph::{MeshMorphWeights, MorphWeights},
    prelude::*,
};

use crate::{manifest::Manifest, portrait::{self, Actor, ActorPart}, talk::Faces, units::{ModelBank, Wearable}};

const LAYER: usize = 3;
const SIZE: (u32, u32) = (300, 400);
const RATE: f32 = 30.0;

/// The newest local match with a known player. A fresh profile has no subject.
#[derive(Resource, Default)]
pub(crate) struct LastPlayedHero(pub Option<bota_proto::HeroId>);

#[derive(Component)]
pub(crate) struct HeroImage;
#[derive(Component)]
struct HeroCamera;
#[derive(Component)]
struct HeroLight;
#[derive(Component)]
struct StageEntity;

#[derive(Resource, Default)]
pub(crate) struct HeroStage {
    pub(crate) hero: Option<bota_proto::HeroId>,
    pub(crate) actor: Option<Actor>,
    image: Handle<Image>,
    next_render: f32,
    framed: bool,
    render: bool,
}

pub(super) fn draw(kit: &mut super::kit::Kit, root: Entity) {
    kit.commands.spawn((
        HeroImage,
        ImageNode::default(),
        Node { position_type: PositionType::Absolute, left: px(456), top: px(230),
            width: px(300), height: px(400), ..default() },
        Pickable::IGNORE,
        Visibility::Hidden,
        Name::new("home hero"),
        ChildOf(root),
    ));
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<HeroStage>().init_resource::<LastPlayedHero>()
        .add_systems(Update, (select, pose).chain().after(super::screens::draw))
        .add_systems(PostUpdate, (
            frame.after(bevy::transform::TransformSystems::Propagate).before(bevy::camera::CameraUpdateSystems),
            visible.in_set(VisibilitySystems::CheckVisibility).after(check_visibility_cpu_culling),
            activate.after(VisibilitySystems::CheckVisibility)
                .after(SimulationLightSystems::UpdateDirectionalLightCascades)
                .before(SimulationLightSystems::AssignLightsToClusters)
                .before(SimulationLightSystems::UpdateLightFrusta),
        ));
}

fn select(
    mut commands: Commands,
    core: Option<NonSend<super::MenuCore>>,
    last: Res<LastPlayedHero>,
    mut stage: ResMut<HeroStage>,
    mut images: ResMut<Assets<Image>>,
    mut nodes: Query<(&mut ImageNode, &mut Visibility), With<HeroImage>>,
    mut bank: ResMut<ModelBank>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    cameras: Query<Entity, With<StageEntity>>,
) {
    let hero = last.0.filter(|_| core.as_ref().is_none_or(|c| !c.shows_game()))
        .filter(|_| !nodes.is_empty());
    if hero != stage.hero || hero.is_none() {
        if let Some(actor) = stage.actor.take() { commands.entity(actor.pivot).despawn(); }
        for camera in &cameras { commands.entity(camera).despawn(); }
        stage.image = Handle::default();
        stage.framed = false;
        stage.hero = hero;
    }
    for (mut node, mut shown) in &mut nodes {
        shown.set_if_neq(crate::ui::visible(stage.actor.as_ref().is_some_and(|a| a.posed)));
        if node.image != stage.image { node.image = stage.image.clone(); }
    }
    let Some(hero) = hero else { return; };
    let Some(name) = manifest.dota_name("hero", hero.0) else { return; };
    let Some(model) = bank.request(&format!("hero.{name}.model"), &server, &manifest) else { return; };
    let entry = &bank.0[model];
    if stage.actor.is_none() && entry.ready() && let Some(scenes) = &entry.scenes {
        stage.image = portrait::stage_image(&mut images, SIZE);
        stage.actor = Some(portrait::spawn_actor(&mut commands, scenes, model, entry.model.scale,
            None, portrait::STAGE, LAYER));
        commands.spawn((StageEntity, HeroCamera, Camera3d::default(), Camera {
            order: -3, is_active: false, clear_color: ClearColorConfig::Custom(Color::NONE), ..default()
        }, RenderTarget::Image(ImageRenderTarget { handle: stage.image.clone(), scale_factor: 1.0 }),
            Projection::Orthographic(OrthographicProjection { scaling_mode: bevy::camera::ScalingMode::FixedVertical {
                viewport_height: 250.0 }, near: 0.1, far: 4000.0, ..OrthographicProjection::default_3d() }),
            AmbientLight { color: Color::srgb(0.7, 0.75, 0.85), brightness: 600.0, ..default() },
            RenderLayers::layer(LAYER), ClusterConfig::Single, Msaa::Sample4,
            bevy::core_pipeline::tonemapping::Tonemapping::TonyMcMapface));
        commands.spawn((StageEntity, HeroLight, DirectionalLight { illuminance: 7000.0, shadow_maps_enabled: false, ..default() },
            Transform::default().looking_to(Vec3::new(0.6, -1.0, 0.8), Vec3::Y), RenderLayers::layer(LAYER)));
    }
}

fn pose(
    mut stage: ResMut<HeroStage>, bank: Res<ModelBank>, args: Res<crate::Args>,
    mut faces: ResMut<Faces>, time: Res<Time<bevy::time::Real>>,
    children: Query<&Children>, parts: Query<&ActorPart>, wearing: Query<&Wearable>,
    meshes: Query<(&MeshMorphWeights, &GltfMeshName, &GltfMeshExtras)>,
    mut players: Query<&mut AnimationPlayer>, mut commands: Commands,
) {
    if let Some(actor) = stage.actor.as_mut().filter(|a| !a.posed) {
        portrait::pose_actor(actor, &bank, &args, &mut faces, &time, &children, &parts,
            &wearing, &meshes, &mut players, &mut commands);
    }
}

fn frame(
    mut stage: ResMut<HeroStage>, time: Res<Time<bevy::time::Real>>,
    children: Query<&Children>, bounds: Query<(&Aabb, &GlobalTransform)>,
    mut cameras: Query<(&mut Camera, &mut Transform, &mut Projection), With<HeroCamera>>,
    faces: Res<Faces>, mut weights: Query<&mut MorphWeights>,
) {
    let now = time.elapsed_secs();
    let render = stage.actor.as_ref().is_some_and(|a| a.posed) && now >= stage.next_render;
    if render { stage.next_render = now + 1.0 / RATE; }
    let framed = stage.framed;
    for (mut camera, mut at, mut projection) in &mut cameras {
        camera.is_active = false;
        if !render { continue; }
        let Some(actor) = &stage.actor else { continue; };
        if !framed && let Some((center, half)) = portrait::model_bounds(actor.pivot, &children, &bounds) {
            let center = portrait::STAGE + center;
            *at = Transform::from_translation(center + Vec3::NEG_Z * half.length().max(8.0) * 4.0)
                .looking_at(center, Vec3::Y);
            if let Projection::Orthographic(p) = &mut *projection {
                p.scaling_mode = bevy::camera::ScalingMode::FixedVertical {
                    viewport_height: (half.y.max(half.x * SIZE.1 as f32 / SIZE.0 as f32) * 2.2).max(16.0) };
            }
        }
        if let Some(face) = actor.face.as_ref() { face.pose(now, None, &faces, &mut weights); }
    }
    if render { stage.framed = true; }
    stage.render = render;
}

fn visible(stage: Res<HeroStage>, children: Query<&Children>,
    mut seen: Query<(&InheritedVisibility, &mut ViewVisibility)>, lights: Query<Entity, With<HeroLight>>) {
    if let Some(actor) = stage.actor.as_ref().filter(|_| stage.render) && let Ok(light) = lights.single() {
        portrait::show_actor(actor.pivot, light, &children, &mut seen);
    }
}

fn activate(stage: Res<HeroStage>, children: Query<&Children>,
    classes: Query<(&VisibilityClass, &InheritedVisibility)>, lights: Query<Entity, With<HeroLight>>,
    mut cameras: Query<(&mut Camera, &mut VisibleEntities), With<HeroCamera>>) {
    if let Ok(light) = lights.single() && let Ok((mut camera, mut seen)) = cameras.single_mut() {
        portrait::camera_actor(stage.actor.as_ref().filter(|_| stage.render).map(|a| a.pivot),
            light, &children, &classes, &mut camera, &mut seen);
    }
}
