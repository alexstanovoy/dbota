//! glTF hero: load from an absolute/cache path, build an AnimationGraph from named clips,
//! and drive the attack clip's time from (fake) simulator attack timing.

use bevy::{
    asset::RecursiveDependencyLoadState, ecs::system::SystemParam, gltf::GltfAssetLabel,
    prelude::*, world_serialization::WorldInstanceReady,
};

use crate::{
    Args,
    coords::{sim_facing_to_rotation, sim_to_world},
    fallback::{FALLBACK_ATTACK_IMPACT, FoundAssets},
};

/// Fake simulator attack: Shadow Fiend-like attack point and base attack time.
const SIM_ATTACK_POINT: f32 = 0.5;
const SIM_ATTACK_PERIOD: f32 = 1.7;

/// Clip names tried in order (fallback model: "idle"/"attack"; Shadow Fiend: "attack01").
const IDLE_CLIPS: &[&str] = &["idle"];
const ATTACK_CLIPS: &[&str] = &["attack", "attack01"];

/// Source2Viewer exports meters (Source units x 0.0254); the simulator uses Source units.
const S2V_UNITS_PER_METER: f32 = 1.0 / 0.0254;
const HERO_SIM_POS: Vec2 = Vec2::new(0.0, -250.0);
/// Facing south-east-ish so the model's side is visible from the camera.
const HERO_SIM_FACING: f32 = -1.0;

#[derive(Resource)]
struct HeroGltf(Handle<Gltf>);

#[derive(Resource)]
struct HeroAnimations {
    graph: Handle<AnimationGraph>,
    idle: AnimationNodeIndex,
    attack: AnimationNodeIndex,
    attack_clip: Handle<AnimationClip>,
    /// Time of the hit frame inside the attack clip.
    attack_impact: f32,
}

#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum AnimMode {
    /// Clip plays on its own clock (speed adjustable with +/-).
    IdleLoop,
    /// Clip time is set every frame from the simulator's attack phase.
    SimAttack,
}

/// Fired when the simulated attack reaches its attack point (consumed by `audio`).
#[derive(Message, Debug, Clone, Copy)]
pub struct AttackImpact;

#[derive(Component)]
struct HeroPlayer;

pub fn plugin(app: &mut App) {
    app.add_message::<AttackImpact>()
        .add_systems(Startup, (init_mode, load_hero))
        .add_systems(
            Update,
            (
                spawn_hero_when_loaded,
                (switch_mode, drive_animation)
                    .chain()
                    .run_if(resource_exists::<HeroAnimations>),
            ),
        );
}

fn init_mode(mut commands: Commands, args: Res<Args>) {
    commands.insert_resource(args.anim);
}

fn load_hero(mut commands: Commands, server: Res<AssetServer>, found: Res<FoundAssets>) {
    // `override_unapproved` lets a raw absolute path through `UnapprovedPathMode::Deny`;
    // it is a no-op for `cache://` paths, which are inside an approved source.
    let gltf = server
        .load_builder()
        .override_unapproved()
        .load(found.model.clone());
    commands.insert_resource(HeroGltf(gltf));
}

/// Asset collections needed to turn a loaded glTF into an animated instance.
#[derive(SystemParam)]
struct AnimAssets<'w> {
    gltfs: Res<'w, Assets<Gltf>>,
    clips: Res<'w, Assets<AnimationClip>>,
    graphs: ResMut<'w, Assets<AnimationGraph>>,
}

fn spawn_hero_when_loaded(
    mut commands: Commands,
    server: Res<AssetServer>,
    hero: Res<HeroGltf>,
    found: Res<FoundAssets>,
    mut assets: AnimAssets,
    time: Res<Time>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    // Report a failed glTF or texture once instead of waiting forever.
    if let RecursiveDependencyLoadState::Failed(err) =
        server.recursive_dependency_load_state(&hero.0)
    {
        *done = true;
        error!("hero glTF (or a dependency) failed to load: {err}");
        return;
    }
    // Wait for the root only: in 0.19.1 `is_loaded_with_dependencies` intermittently stays
    // false for Source2Viewer exports (~40% of runs) although every texture reports Loaded
    // (a dependency-state race). The root brings the scene and clips; textures pop in later.
    if !server.is_loaded(&hero.0) {
        return;
    }
    *done = true;
    let gltf = assets
        .gltfs
        .get(&hero.0)
        .expect("loaded glTF is in Assets<Gltf>");
    let names: Vec<_> = gltf.named_animations.keys().collect();
    info!(
        "hero glTF loaded after {:.1} s; animations: {names:?}",
        time.elapsed_secs()
    );
    match build_animations(gltf, &assets.clips, found.model_is_fallback) {
        Some((graph, anims)) => commands.insert_resource(HeroAnimations {
            graph: assets.graphs.add(graph),
            ..anims
        }),
        None => warn!("hero model has no animations; spawning it static"),
    }
    spawn_hero_scene(&mut commands, &server, &found);
}

/// Graph with the idle and attack clips (by name, falling back to the first clip).
fn build_animations(
    gltf: &Gltf,
    clips: &Assets<AnimationClip>,
    is_fallback: bool,
) -> Option<(AnimationGraph, HeroAnimations)> {
    let first = gltf.animations.first()?;
    let clip = |names: &[&str]| {
        names
            .iter()
            .find_map(|n| gltf.named_animations.get(*n))
            .unwrap_or(first)
            .clone()
    };
    let (idle_clip, attack_clip) = (clip(IDLE_CLIPS), clip(ATTACK_CLIPS));
    let attack_duration = clips.get(&attack_clip).map_or(1.0, AnimationClip::duration);
    let attack_impact = if is_fallback {
        FALLBACK_ATTACK_IMPACT
    } else {
        // Real models: take the impact from the vmdl attack event; assume 40% meanwhile.
        0.4 * attack_duration
    };
    let (graph, nodes) = AnimationGraph::from_clips([idle_clip, attack_clip.clone()]);
    let anims = HeroAnimations {
        graph: Handle::default(),
        idle: nodes[0],
        attack: nodes[1],
        attack_clip,
        attack_impact,
    };
    Some((graph, anims))
}

fn spawn_hero_scene(commands: &mut Commands, server: &AssetServer, found: &FoundAssets) {
    // The scene is a labeled sub-asset of the same file: "<path>#Scene0".
    let scene = server
        .load_builder()
        .override_unapproved()
        .load(GltfAssetLabel::Scene(0).from_asset(found.model.clone()));
    let scale = if found.model_is_fallback {
        1.0
    } else {
        S2V_UNITS_PER_METER
    };
    // Unit root carries position only; the model child carries facing and scale, so
    // billboards and selection rings parented to the root never inherit the model's yaw.
    let root = commands
        .spawn((
            Name::new("hero"),
            Transform::from_translation(sim_to_world(HERO_SIM_POS)),
            Visibility::default(),
        ))
        .id();
    // WorldInstanceReady is an EntityEvent on the WorldAssetRoot entity and does not
    // propagate, so the observer must sit on that entity, not on the unit root.
    commands
        .spawn((
            WorldAssetRoot(scene),
            Transform::from_rotation(sim_facing_to_rotation(HERO_SIM_FACING))
                .with_scale(Vec3::splat(scale)),
            ChildOf(root),
        ))
        .observe(on_hero_scene_ready);
}

fn on_hero_scene_ready(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    args: Res<Args>,
    mode: Res<AnimMode>,
    anims: Option<Res<HeroAnimations>>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let Some(anims) = anims else {
        return;
    };
    // glTF puts the AnimationPlayer on the animation root node, somewhere below the scene
    // root; search the instance's descendants for it.
    let Some(player_entity) = children
        .iter_descendants(ready.entity)
        .find(|e| players.contains(*e))
    else {
        warn!("hero scene has no AnimationPlayer");
        return;
    };
    let mut player = players.get_mut(player_entity).expect("checked above");
    match *mode {
        AnimMode::IdleLoop => player.play(anims.idle).repeat().set_speed(args.idle_speed),
        // Paused: time only moves through set_seek_time in `drive_animation`.
        AnimMode::SimAttack => player.play(anims.attack).repeat().pause(),
    };
    commands
        .entity(player_entity)
        .insert((AnimationGraphHandle(anims.graph.clone()), HeroPlayer));
}

fn switch_mode(
    keys: Res<ButtonInput<KeyCode>>,
    anims: Res<HeroAnimations>,
    mut mode: ResMut<AnimMode>,
    mut player: Single<&mut AnimationPlayer, With<HeroPlayer>>,
) {
    if keys.just_pressed(KeyCode::Space) {
        *mode = match *mode {
            AnimMode::IdleLoop => AnimMode::SimAttack,
            AnimMode::SimAttack => AnimMode::IdleLoop,
        };
        player.stop_all();
        match *mode {
            AnimMode::IdleLoop => player.play(anims.idle).repeat(),
            AnimMode::SimAttack => player.play(anims.attack).repeat().pause(),
        };
        info!("hero animation mode: {:?}", *mode);
    }
    let Some(active) = player.animation_mut(anims.idle) else {
        return;
    };
    let factor = if keys.just_pressed(KeyCode::Equal) {
        1.25
    } else if keys.just_pressed(KeyCode::Minus) {
        0.8
    } else {
        return;
    };
    active.set_speed(active.speed() * factor);
    info!("idle speed: {}", active.speed());
}

/// Maps the simulator's attack phase onto the clip so the hit frame lands exactly on the
/// simulator's attack point, regardless of the clip's own timing.
fn drive_animation(
    time: Res<Time>,
    mode: Res<AnimMode>,
    anims: Res<HeroAnimations>,
    clips: Res<Assets<AnimationClip>>,
    mut player: Single<&mut AnimationPlayer, With<HeroPlayer>>,
    mut impacts: MessageWriter<AttackImpact>,
    mut last_phase: Local<f32>,
) {
    if *mode != AnimMode::SimAttack {
        return;
    }
    let phase = time.elapsed_secs() % SIM_ATTACK_PERIOD;
    if *last_phase < SIM_ATTACK_POINT && phase >= SIM_ATTACK_POINT {
        impacts.write(AttackImpact);
    }
    *last_phase = phase;
    let duration = clips
        .get(&anims.attack_clip)
        .map_or(1.0, AnimationClip::duration);
    let clip_time = (phase * anims.attack_impact / SIM_ATTACK_POINT).min(duration);
    if let Some(active) = player.animation_mut(anims.attack) {
        // set_seek_time: jump without firing clip events in between (seek_to would fire them).
        active.set_seek_time(clip_time);
    }
}
