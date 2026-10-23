//! A material (`.vmat_c`, KV3 or NTRO): its parameters read and converted to a glTF material
//! with the images it names, plain and tinted. No input panics.
#![no_main]

use dbota_assets::fuzzing::material_json;
use dbota_assets::{Material, Resource};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let Some(Ok(value)) = resource.data() else {
        return;
    };
    let Ok(material) = Material::from_vmat("materials/m.vmat", &value) else {
        return;
    };
    for tint in [[1.0; 4], [0.5, f32::NAN, f32::INFINITY, -1.0]] {
        let _ = material_json(&material, tint);
    }
});
