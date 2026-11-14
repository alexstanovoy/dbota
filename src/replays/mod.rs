//! The replays this client keeps for its player, by default: the library and its rules, and
//! the two places they are kept (a folder on a desktop, the browser's private storage).

mod backend;
#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(not(target_arch = "wasm32"))]
mod disk;
mod index;
mod library;
mod name;

#[cfg(target_arch = "wasm32")]
pub use browser::open as open_browser;
#[cfg(all(test, not(target_arch = "wasm32")))]
pub use disk::keep_in_for_tests;
#[cfg(not(target_arch = "wasm32"))]
pub use disk::{Disk, default_dir, recording_path};
pub(crate) use index::valid_name;
pub use index::{MAX_BYTES, MIN_BYTES, Policy, Record, Wanted};
pub use library::{Library, Notice, Saving, Standing};
#[cfg(test)]
pub(crate) use name::pass_time;
pub use name::{now, wall_clock};
