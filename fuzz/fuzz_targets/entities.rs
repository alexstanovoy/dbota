//! An entity lump (`.vents_c`): its entities and child lumps, the models it places, and the
//! cache's `trees.txt`, `buildings.txt` and `lighting.txt` made of it. No input panics.
#![no_main]

use dbota_assets::{EntityLump, Resource, buildings_txt, lighting_txt, trees_txt};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(resource) = Resource::parse(data) else {
        return;
    };
    let Ok(lump) = EntityLump::read(&resource) else {
        return;
    };
    let _ = lump.models();
    let _ = trees_txt(&lump.entities);
    let _ = buildings_txt(&lump.entities);
    let _ = lighting_txt(&lump.entities);
});
