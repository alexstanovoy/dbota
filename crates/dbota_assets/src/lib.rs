//! Reads Valve's Source 2 formats straight from a local Dota 2 install: VPK packages, compiled
//! resource containers and binary KV3, through a [`Source`] that is a folder on disk natively
//! and the user's picked files in a browser. Every parser is bounded: the input is an untrusted
//! local file, and a malformed one is an [`Error`], never a panic or an unbounded allocation.

mod anim;
mod budget;
mod bytes;
#[cfg(any(unix, windows))]
pub mod cache_dir;
mod error;
mod export;
mod gltf;
pub mod import;
mod keyvalues;
pub mod kv3;
#[cfg(any(unix, windows))]
mod locate;
mod material;
mod meshopt;
mod model;
mod ntro;
mod portraits;
mod resource;
mod shops;
mod sound;
mod source;
mod texture;
mod tooltips;
mod vbib;
mod vertices;
mod vpk;
mod world;

pub use anim::{Clip, Keys, Track, read_clips};
pub use error::{Error, Result};
pub use export::{ExportedClip, ExportedModel, ExportedTexture, export_model, read_from};
pub use gltf::{GltfOptions, write_glb};
#[cfg(any(unix, windows))]
pub use locate::find_game_dir;
pub use material::{Alpha, Material, Pack, orm_uri};
pub use model::{Bone, Mesh, Model, Primitive};
pub use portraits::{background_png, framings, portraits_txt};
pub use resource::{Block, ExternalRef, Resource};
pub use shops::shop_json;
pub use sound::{Sound, SoundFormat};
#[cfg(any(unix, windows))]
pub use source::DirSource;
pub use source::{Slice, SliceSource, Source};
pub use texture::{Encoding, PixelFormat, Texture};
pub use tooltips::{Wanted, labels_json, tooltips_json};
pub use vbib::{Buffer, Field};
pub use vertices::{Attribute, Skin, Vertices};
pub use vpk::{Entry, Vpk};
pub use world::{
    Band, EntityLump, ExportedWorld, buildings_txt, export_world, lighting_txt, trees_txt,
};
