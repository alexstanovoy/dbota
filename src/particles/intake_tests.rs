//! Cache intake must reject unsupported typed loads before requesting assets.

use super::{CacheAssets, Library};
use bevy::prelude::*;
use heatel::particle::System;

const VERSION: u32 = dbota_import::text::PARTICLE_VERSION;

fn asset_app() -> App {
    crate::io_pool::make_for_tests();
    let mut app = App::new();
    app.add_plugins(MinimalPlugins);
    let source = crate::cache_source(&mut app, std::path::Path::new("temp/typed_intake"));
    app.register_asset_source(crate::CACHE_SOURCE, source)
        .add_plugins(bevy::asset::AssetPlugin::default())
        .init_asset::<Image>()
        .init_asset::<bevy::gltf::Gltf>();
    app
}

fn particle_file(kind: &str, path: &str) -> Vec<u8> {
    let system = System::from_document(&heatel::kv3::Document {
        format: [0; 16],
        root: heatel::kv3::Value::Object(Default::default()),
    })
    .expect("an empty definition has the schema defaults");
    let mut file = serde_json::json!({
        "version": VERSION,
        "systems": [{ "path": "root", "system": system }],
    });
    file[kind] = match kind {
        "textures" => serde_json::json!([{ "vtex": "texture.vtex", "file": path }]),
        "models" => serde_json::json!([{ "vmdl": "model.vmdl", "file": path }]),
        _ => panic!("the test asks for a texture or model"),
    };
    serde_json::to_vec(&file).unwrap()
}

#[test]
fn typed_asset_intake_rejects_particle_images_before_requesting_them() {
    let app = asset_app();
    let server = app.world().resource::<AssetServer>();
    for path in [
        "effects/image.png",
        "effects/image.PNG",
        "effects/image.ktx2",
    ] {
        let mut library = Library::default();
        assert!(
            library
                .add(
                    "allowed",
                    &particle_file("textures", path),
                    &CacheAssets(server)
                )
                .is_ok()
        );
    }
    for path in [
        "effects/image",
        "effects/image.",
        "effects/image.glb",
        "effects/image.bin",
    ] {
        let mut library = Library::default();
        let why = library
            .add(
                "refused",
                &particle_file("textures", path),
                &CacheAssets(server),
            )
            .expect_err("a typed image load needs a supported cache image path");
        assert_eq!(why, format!("texture file {path:?}"));
        assert!(
            server
                .get_path_ids(crate::manifest::cache_path(path))
                .is_empty()
        );
    }
}

#[test]
fn typed_asset_intake_rejects_particle_models_before_requesting_them() {
    let app = asset_app();
    let server = app.world().resource::<AssetServer>();
    for path in [
        "effects/model.glb",
        "effects/model.GLB",
        "effects/model.gltf",
    ] {
        let mut library = Library::default();
        assert!(
            library
                .add(
                    "allowed",
                    &particle_file("models", path),
                    &CacheAssets(server)
                )
                .is_ok()
        );
    }
    for path in [
        "effects/model",
        "effects/model.",
        "effects/model.png",
        "effects/model.bin",
    ] {
        let mut library = Library::default();
        let why = library
            .add(
                "refused",
                &particle_file("models", path),
                &CacheAssets(server),
            )
            .expect_err("a typed model load must keep the model-vetting boundary");
        assert_eq!(why, format!("model file {path:?}"));
        assert!(library.models.is_empty());
        assert!(
            server
                .get_path_ids(crate::manifest::cache_path(path))
                .is_empty()
        );
    }
}
