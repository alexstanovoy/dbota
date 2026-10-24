//! Dota's particle systems, as the install defines them (read by the importer into the cache):
//! simulated on the CPU as Source 2 simulates them, drawn as instanced cards.

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

pub use library::Library;
pub use plugin::{Handle, Playing};

use bevy::prelude::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        plugin::plugin,
        render::plugin,
        lights::plugin,
        models::plugin,
    ));
}
