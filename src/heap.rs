//! Gives the heap's free memory back to the system once the scene is whole.
//! Loading decodes most of a gigabyte of textures and meshes on every core,
//! and glibc keeps what they freed in its per-thread arenas: about three
//! gigabytes held for nothing for the rest of the run.

use bevy::prelude::*;

pub fn plugin(app: &mut App) {
    app.add_systems(Last, trim_once);
}

/// Trims the heap once the minimap is photographed (the ground and the
/// forest are in) and every unit model has loaded.
fn trim_once(
    photo: Res<crate::minimap::PhotoTaken>,
    bank: Res<crate::units::ModelBank>,
    mut done: Local<bool>,
) {
    if *done || !photo.0 || !bank.0.iter().all(|m| m.loaded && m.graph.is_some()) {
        return;
    }
    *done = true;
    trim();
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn trim() {
    unsafe extern "C" {
        fn malloc_trim(pad: usize) -> std::ffi::c_int;
    }
    // SAFETY: malloc_trim only returns free pages of glibc's own heap to the
    // system; it takes no pointers and leaves every allocation in place.
    let released = unsafe { malloc_trim(0) };
    info!(
        "heap: free memory given back after loading: {}",
        released != 0
    );
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn trim() {}
