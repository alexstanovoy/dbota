//! Dota's `portraits.txt`: the framings of the models the client's manifest lists, and the
//! cache's `portraits.txt` made of them. No input panics.
#![no_main]

use dbota_assets::import::{Item, Plan};
use dbota_assets::{background_png, framings, portraits_txt};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let plan = Plan::parse(include_str!("../../assets-manifest/manifest.txt"), "");
    let Some(Item::Portraits { models, .. }) = plan.items.last() else {
        return;
    };
    let wanted: Vec<(&str, &str)> = models
        .iter()
        .map(|(logical, vmdl)| (logical.as_str(), vmdl.as_str()))
        .collect();
    let Ok(framed) = framings(data, &wanted) else {
        return;
    };
    let _ = portraits_txt(&framed, |vmat| Some(background_png(vmat)));
});
