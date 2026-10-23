//! A whole import, as `dbota import` and the browser's worker run it, from an install made of
//! the input: one manifest line of every kind, each source a file of the input, then the files
//! they name (materials, textures, world nodes, models, portrait backgrounds). Every file the
//! importer hands its sink has a safe cache path and the import stays within its limits. No
//! input panics.
#![no_main]

#[path = "common/files.rs"]
mod files;

use dbota_assets::import::{Flow, Plan, Sink, run, safe_path};
use libfuzzer_sys::fuzz_target;

const MANIFEST: &str = "\
model hero.m.model models/m.vmdl_c models/m/m.glb anims=idle,run,attack attach=x
model unit.n.model models/n.vmdl_c models/n/n.glb meshes=n_lod0
texture icon.t materials/t.vtex_c icons/t.png
sound sound.s sounds/s.vsnd_c sounds/s.wav event=s
world map.mid maps/w/world.vwrld_c map/mid/mid.gltf vpk=maps/w.vpk half_width=1e9 v_min=-1e9 v_max=1e9 layers=textures/ground trees=map/mid/trees.txt lighting=map/mid/lighting.txt
portraits models.portraits scripts/npc/portraits.txt models/portraits.txt
shop text.shop scripts/shops.txt text/shop.json strings=resource/localization/dota_english.txt
";
const IDS: &str = "item 1 blink\nability 2 nevermore_shadowraze1\nhero 3 nevermore\n";
/// The sources, the input's first files.
const ROOTS: [&str; 12] = [
    "models/m.vmdl_c",
    "models/n.vmdl_c",
    "materials/t.vtex_c",
    "sounds/s.vsnd_c",
    "maps/w/world.vwrld_c",
    "scripts/npc/portraits.txt",
    "resource/localization/abilities_english.txt",
    "scripts/npc/items.txt",
    "scripts/npc/npc_abilities.txt",
    "scripts/npc/heroes/npc_dota_hero_nevermore.txt",
    "scripts/shops.txt",
    "resource/localization/dota_english.txt",
];
/// Most bytes an import writes (the importer's own limit).
const WRITE_LIMIT: u64 = 1536 << 20;

/// Checks what the importer writes, keeping only the sizes.
#[derive(Default)]
struct Checked {
    written: u64,
}

impl Sink for Checked {
    fn write(&mut self, path: &str, data: &[u8]) -> dbota_assets::Result<()> {
        assert!(safe_path(path), "the importer wrote to {path:?}");
        self.written += data.len() as u64;
        assert!(
            self.written <= WRITE_LIMIT,
            "the import wrote past its limit"
        );
        Ok(())
    }
}

fuzz_target!(|data: &[u8]| {
    let parts = files::split(data);
    let pack = files::resolve(&ROOTS, &parts);
    let src = files::install(&[("pak01_dir.vpk", &pack), ("maps/w.vpk", &pack)]);
    let plan = Plan::parse(MANIFEST, IDS);
    let _ = run(src, &plan, &mut Checked::default(), &mut |_| Flow::Continue);
});
