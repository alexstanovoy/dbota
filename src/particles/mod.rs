//! Connects the particle renderer to the game's assets, clock, ground and camera.

use std::collections::BTreeSet;

use bevy::{
    camera::primitives::Frustum, prelude::*, transform::TransformSystems,
    world_serialization::WorldAssetRoot,
};

use crate::{camera::RtsCamera, manifest::Manifest};
#[cfg(test)]
pub(crate) use dbota_particles::{Effect, ParticleFrame};
pub use dbota_particles::{Handle, Library, Step as Simulation};
pub use playing::{EffectSight, Playing};

mod playing;

#[cfg(test)]
pub(crate) use tick as gather_frame;

#[cfg(test)]
mod intake_tests;
#[cfg(test)]
mod tests;

/// The particle library is ready for systems that bind game effects.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Loaded;

pub fn plugin(app: &mut App) {
    app.init_resource::<Playing>()
        .add_systems(First, reset.in_set(crate::loading::Begin))
        .add_plugins(dbota_particles::plugin)
        .add_systems(
            Update,
            load.run_if(crate::loading::requested).in_set(Loaded),
        )
        .add_systems(Update, warm_models.after(dbota_particles::LoadModels))
        .add_systems(
            PostUpdate,
            tick.after(TransformSystems::Propagate).in_set(Simulation),
        )
        .add_systems(
            PostUpdate,
            report
                .after(dbota_particles::Report)
                .in_set(crate::loading::Report),
        );
}

pub(crate) struct CacheAssets<'a>(pub(crate) &'a AssetServer);

impl dbota_particles::AssetSource for CacheAssets<'_> {
    fn accepts_image(&self, file: &str) -> bool {
        crate::cache_path::image_format(std::path::Path::new(file)).is_some()
    }

    fn accepts_model(&self, file: &str) -> bool {
        crate::cache_path::is_model(std::path::Path::new(file))
    }

    fn image(&self, file: &str) -> bevy::asset::Handle<Image> {
        self.0.load(crate::manifest::cache_path(file))
    }

    fn model(&self, file: &str) -> bevy::asset::Handle<bevy::gltf::Gltf> {
        crate::manifest::load_unit_model(self.0, crate::manifest::cache_path(file))
    }
}

/// Reads the manifest's effects once; absent and unsupported art is reported as before.
fn load(mut commands: Commands, manifest: Res<Manifest>, server: Res<AssetServer>) {
    let mut library = Library::default();
    let mut unsupported = BTreeSet::new();
    let assets = CacheAssets(&server);
    for (logical, path) in manifest.particles() {
        let Some(bytes) = crate::cachefs::read_small(&manifest.root, path) else {
            continue;
        };
        match library.add(logical, &bytes, &assets) {
            Ok(classes) => unsupported.extend(classes),
            Err(why) => error!("particle effect {logical} ({path}): {why}; drawn without it"),
        }
    }
    if !unsupported.is_empty() {
        let list: Vec<String> = unsupported.into_iter().collect();
        warn!(
            "particle classes not simulated, left out of their systems: {}",
            list.join(", ")
        );
    }
    commands.insert_resource(library);
}

pub(crate) fn tick(
    time: Res<Time>,
    sight: crate::fog::EventSight,
    ground: Res<crate::map::Ground>,
    camera: Query<(&GlobalTransform, &Frustum), With<RtsCamera>>,
    mut playing: ResMut<Playing>,
    library: Option<Res<Library>>,
    mut frame: ResMut<dbota_particles::ParticleFrame>,
) {
    let height = |x: f32, y: f32| ground.surface_at(x, y);
    let height: Option<dbota_particles::HeightAt<'_>> = ground.known().then_some(&height);
    let pace = match &sight.core().source {
        bota_client_core::Source::Replay(player) if player.paused => 0.0,
        bota_client_core::Source::Replay(player) => player.speed,
        _ => 1.0,
    };
    playing.step(
        time.delta_secs() * pace,
        height,
        library.as_deref(),
        camera.single().ok(),
        &mut frame,
        crate::map::UNITS_PER_METRE,
        &sight,
    );
}

fn report(progress: Res<dbota_particles::ModelProgress>, mut parts: ResMut<crate::loading::Parts>) {
    if let Some((done, total)) = progress.0 {
        parts.set("particles", done, total);
    }
}

/// Pipeline preparation belongs to the client, including scenes supplied by the particle pool.
fn warm_models(mut commands: Commands, mut ready: MessageReader<dbota_particles::ModelReady>) {
    for model in ready.read() {
        commands.spawn((
            WorldAssetRoot(model.0.clone()),
            crate::warmup::Warmup,
            crate::warmup::Warmup::transform(),
        ));
    }
}

fn reset(mut playing: ResMut<Playing>) {
    playing.reset();
}
