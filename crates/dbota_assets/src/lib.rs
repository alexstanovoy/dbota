//! Reads Valve's Source 2 formats straight from a local Dota 2 install: VPK packages, compiled
//! resource containers and binary KV3, through a [`Source`] that is a folder on disk natively
//! and the user's picked files in a browser. Every parser is bounded: the input is an untrusted
//! local file, and a malformed one is an [`Error`], never a panic or an unbounded allocation.

mod budget;
mod bytes;
mod error;
mod export;
mod gltf;
pub mod kv3;
#[cfg(any(unix, windows))]
mod locate;
mod material;
mod meshopt;
mod model;
mod ntro;
mod resource;
mod sound;
mod source;
mod texture;
mod vbib;
mod vpk;

pub use error::{Error, Result};
pub use export::{ExportedModel, ExportedTexture, export_model, read_from};
pub use gltf::{GltfOptions, write_glb};
#[cfg(any(unix, windows))]
pub use locate::find_game_dir;
pub use material::{Alpha, Material, Pack, orm_uri};
pub use model::{Bone, Mesh, Model, Primitive, Skin, Vertices};
pub use resource::{Block, ExternalRef, Resource};
pub use sound::{Sound, SoundFormat};
#[cfg(any(unix, windows))]
pub use source::DirSource;
pub use source::{Slice, SliceSource, Source};
pub use texture::{Encoding, PixelFormat, Texture};
pub use vbib::{Buffer, Field};
pub use vpk::{Entry, Vpk};
