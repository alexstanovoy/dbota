//! A model's embedded animations (the `ANIM`, `AGRP` and `ASEQ` blocks), every clip its own
//! strings name decoded and written into a `.glb` over a skeleton of as many bones. No input
//! panics.
#![no_main]

#[path = "common/files.rs"]
mod files;

use std::collections::HashMap;

use dbota_assets::{Bone, GltfOptions, Model, Resource, read_clips, write_glb};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let mut names = files::strings(data);
    names.truncate(64);
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let Ok(clips) = read_clips(&resource, &names) else {
        return;
    };
    let bones = clips.iter().map(|c| c.tracks.len()).max().unwrap_or(0);
    let model = Model {
        bones: (0..bones)
            .map(|i| Bone {
                name: format!("b{i}"),
                parent: i.checked_sub(1),
                translation: [0.0; 3],
                rotation: [0.0, 0.0, 0.0, 1.0],
            })
            .collect(),
        meshes: Vec::new(),
        material_groups: Vec::new(),
    };
    for clip in &clips {
        let _ = clip.duration();
    }
    let options = GltfOptions {
        name: "models/m.vmdl_c",
        meshes: None,
        skeleton: true,
        materials: &HashMap::new(),
        texture_uri: &|vtex: &str| format!("{vtex}.png"),
        clips: &clips,
    };
    let _ = write_glb(&model, &options);
});
