//! The images the game draws once it is on screen, asked for while it loads and kept resident:
//! the interface's art, the icon of every item and ability the match knows, the portraits'
//! backgrounds. Each of them used to be asked for the frame its panel or mark first showed, so
//! that frame loaded a file; with the screen lifting before the hero is picked, all of them are
//! resident when it lifts. (The unit models, the trees, the particles and the sounds load in
//! their own modules.)

use bevy::prelude::*;

pub use crate::cachefs::RESIDENT_BUDGET;
use crate::manifest::Manifest;

/// The handles that keep the images resident.
#[derive(Resource, Default)]
struct Art(Vec<Handle<Image>>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Art>()
        .add_systems(Update, ask.run_if(crate::loading::requested))
        .add_systems(PostUpdate, report.in_set(crate::loading::Report));
}

/// The paths in the cache to hold: every texture of the manifest but the map's ground (its own
/// loader reads that) and the icons of the install's every item and ability (the manifest lists
/// them all, a thousand the match never shows), the icons of the ids bota knows instead (items,
/// abilities, buffs), the portraits' backgrounds.
pub fn paths(manifest: &Manifest) -> Vec<String> {
    let textures = manifest
        .files
        .iter()
        .filter(|(logical, path)| {
            let installed_icon =
                logical.starts_with("icon.item.") || logical.starts_with("icon.ability.");
            !logical.starts_with("ground.") && !installed_icon && path.ends_with(".png")
        })
        .map(|(_, path)| path.clone());
    let icons = manifest
        .ids
        .iter()
        .filter(|(kind, _, _)| matches!(kind.as_str(), "item" | "ability"))
        .filter_map(|(kind, _, name)| manifest.path(&format!("icon.{kind}.{name}")))
        .map(str::to_owned);
    // A buff's icon is named by the logical name its effect's line gives.
    let buffs = manifest
        .ids
        .iter()
        .filter(|(kind, _, _)| kind == "effect")
        .filter_map(|(_, _, icon)| manifest.path(icon))
        .map(str::to_owned);
    let backgrounds = crate::portrait::background_paths(&manifest.root);
    let mut paths: Vec<String> = textures
        .chain(icons)
        .chain(buffs)
        .chain(backgrounds)
        .collect();
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// The cache's requested image sizes, for the export's resident-art regression.
/// An unreadable or malformed header makes the measurement fail.
#[cfg(all(test, not(target_arch = "wasm32")))]
pub fn decoded_bytes(root: &std::path::Path, paths: &[String]) -> Option<usize> {
    use std::io::Read;
    paths.iter().try_fold(0usize, |total, path| {
        let mut head = [0; crate::texture_file::HEAD_BYTES];
        let read = std::fs::File::open(root.join(path))
            .ok()?
            .read(&mut head)
            .ok()?;
        let bytes = crate::texture_file::png_size(&head[..read]).ok()?;
        total.checked_add(bytes)
    })
}

fn ask(manifest: Res<Manifest>, server: Res<AssetServer>, mut art: ResMut<Art>) {
    let paths = paths(&manifest);
    art.0 = paths
        .iter()
        .map(|path| server.load(crate::manifest::cache_path(path)))
        .collect();
    info!(
        "loading: {} interface images requested within a {} MiB decoded residency limit",
        art.0.len(),
        RESIDENT_BUDGET >> 20
    );
}

fn report(art: Res<Art>, server: Res<AssetServer>, mut parts: ResMut<crate::loading::Parts>) {
    let ready = art
        .0
        .iter()
        .filter(|image| crate::loading::settled(&server, image))
        .count();
    parts.set("art", ready, art.0.len());
}
