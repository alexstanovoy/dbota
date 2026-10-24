//! Dota's particle systems, as the install defines them (read by the importer into the cache):
//! simulated on the CPU as Source 2 simulates them, drawn as instanced cards.

mod body;
mod draw;
mod eval;
mod ground;
mod init;
mod library;
mod lights;
mod ops;
mod parent;
mod plugin;
mod pre;
mod render;
mod sim;
mod store;
#[cfg(test)]
mod tests;

pub use library::{Effect, Library};
pub use plugin::{Handle, Playing};

use bevy::prelude::App;

pub fn plugin(app: &mut App) {
    app.add_plugins((plugin::plugin, render::plugin, lights::plugin));
}
