//! The importer takes every item and ability of the install, not a list of its own: each has its
//! icon in the cache (when the install has one), and its tooltip text is in `text/tooltips.json`.
//! Needs the install, so it runs only when asked:
//!
//! ```sh
//! cargo test --release -p dbota_import --test everything -- --ignored
//! DBOTA_IMPORTED=/path/to/cache cargo test ...   # check a cache `dbota import` made
//! ```

use std::path::PathBuf;

use dbota_import::cache_dir::import_into;
use dbota_import::import::{Flow, plan};
use heatel::scripts::entries;
use heatel::{DirSource, Vpk, find_game_dir};

#[test]
#[ignore = "needs a local Dota 2 install"]
fn every_item_and_ability_of_the_install_has_its_icon_and_text() {
    let game = find_game_dir(None).expect("Dota 2 not found");
    let src = DirSource::new(&game);
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let (cache, scratch) = match std::env::var_os("DBOTA_IMPORTED") {
        Some(dir) => (PathBuf::from(dir), false),
        None => {
            let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
                .join(format!("everything-{}", std::process::id()));
            let report = import_into(
                &game,
                &dir,
                plan().expect("this build's plan"),
                "test",
                &mut |_| Flow::Continue,
            )
            .unwrap();
            assert!(report.failed.is_empty(), "{:#?}", report.failed);
            (dir, true)
        }
    };
    let read = |path: &str| entries(&pak.read_file(&src, path).unwrap()).unwrap();
    let mut missing = Vec::new();
    let mut checked = 0;
    let mut check = |entries: Vec<heatel::scripts::Entry>, item: bool| {
        for entry in entries {
            let name = entry.icon_name();
            let (from, to) = if item {
                ("items", "items")
            } else {
                ("spellicons", "abilities")
            };
            if pak
                .get(&format!("panorama/images/{from}/{name}_png.vtex_c"))
                .is_none()
            {
                continue;
            }
            checked += 1;
            if !cache.join(format!("icons/{to}/{name}.png")).is_file() {
                missing.push(format!("{to}/{name}"));
            }
        }
    };
    check(read("scripts/npc/items.txt"), true);
    check(read("scripts/npc/npc_abilities.txt"), false);
    for hero in pak
        .entries()
        .filter(|e| e.dir() == "scripts/npc/heroes" && e.ext() == "txt")
    {
        check(entries(&pak.read(&src, hero).unwrap()).unwrap(), false);
    }
    assert!(checked > 1000, "{checked} icons checked");
    assert!(missing.is_empty(), "icons missing: {missing:?}");
    let tooltips: serde_json::Value =
        serde_json::from_slice(&std::fs::read(cache.join("text/tooltips.json")).unwrap()).unwrap();
    let names = tooltips.as_object().expect("an object");
    assert!(names.len() > 1000, "{} tooltips", names.len());
    for key in ["item_blink", "nevermore_requiem", "pudge_dismember"] {
        assert!(names.contains_key(key), "no text for {key}");
    }
    if scratch {
        let _ = std::fs::remove_dir_all(&cache);
    }
}
