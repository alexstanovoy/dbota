//! Source 2 particle simulation and rendering, with assets and world traces supplied by its host.

#![allow(clippy::too_many_arguments, clippy::type_complexity)]

mod body;
mod constraint;
mod draw;
mod eval;
mod ground;
mod init;
mod library;
mod lights;
mod models;
mod ops;
mod parent;
mod path;
mod plugin;
mod pre;
mod remap;
mod render;
mod shape;
mod sim;
mod store;
#[cfg(test)]
mod tests;

use bevy::prelude::*;

pub use eval::HeightAt;
pub use library::{Effect, Library};
pub use plugin::{Handle, Simulation};
pub use render::ParticleFrame;
pub use sim::Instance;

/// Supplies particle assets without coupling the simulator to a cache layout or importer.
pub trait AssetSource {
    /// Checks a reference before any typed image request is made.
    fn accepts_image(&self, file: &str) -> bool;
    /// Checks every model reference, including cached models and a full model pool.
    fn accepts_model(&self, file: &str) -> bool;
    fn image(&self, file: &str) -> bevy::asset::Handle<Image>;
    fn model(&self, file: &str) -> bevy::asset::Handle<bevy::gltf::Gltf>;
}

/// The host's particle step, after camera transforms are propagated.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Step;

/// Model assets are checked and their newly available scenes are published.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LoadModels;

/// The particle model pools' progress is ready for the host's loading screen.
#[derive(SystemSet, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Report;

/// Models resident (or failed), over all particle models, once the library is known.
#[derive(Resource, Default)]
pub struct ModelProgress(pub Option<(usize, usize)>);

/// A model's scene became available for its pool; the host may prepare its rendering pipelines.
#[derive(Message)]
pub struct ModelReady(pub bevy::asset::Handle<bevy::world_serialization::WorldAsset>);

/// Installs presentation resources and systems; the host owns and steps its simulation.
pub fn plugin(app: &mut App) {
    app.add_plugins((render::plugin, lights::plugin, models::plugin));
}
