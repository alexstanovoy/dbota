//! The importer's guards against a hostile install, without an install. The real import is
//! checked against the exported cache in `cache.rs`.

use std::collections::BTreeMap;

use dbota_assets::import::{Flow, IMPORT_REV, Item, Plan, Sink, is_small, run, safe_path};
use dbota_assets::{Error, SliceSource};

#[derive(Default)]
struct Memory(BTreeMap<String, Vec<u8>>);

impl Sink for Memory {
    fn write(&mut self, path: &str, data: &[u8]) -> dbota_assets::Result<()> {
        self.0.insert(path.to_owned(), data.to_vec());
        Ok(())
    }
}

/// A package directory (VPK v2) with one `ext`-typed file `dir/name` whose bytes are in the
/// directory's preload.
fn package(dir: &str, name: &str, ext: &str, data: &[u8]) -> Vec<u8> {
    let mut tree = Vec::new();
    for part in [ext, dir, name] {
        tree.extend_from_slice(part.as_bytes());
        tree.push(0);
    }
    tree.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
    tree.extend_from_slice(&(data.len() as u16).to_le_bytes());
    tree.extend_from_slice(&0x7FFFu16.to_le_bytes());
    tree.extend_from_slice(&0u32.to_le_bytes());
    tree.extend_from_slice(&0u32.to_le_bytes());
    tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
    tree.extend_from_slice(data);
    tree.extend_from_slice(&[0, 0, 0]);
    let mut out = Vec::new();
    for word in [0x55AA_1234, 2, tree.len() as u32, 0, 0, 48, 0] {
        out.extend_from_slice(&u32::to_le_bytes(word));
    }
    out.extend_from_slice(&tree);
    out
}

fn install(files: &[(&str, Vec<u8>)]) -> SliceSource<Vec<u8>> {
    let mut src = SliceSource::default();
    for (path, bytes) in files {
        src.insert(*path, bytes.clone());
    }
    src
}

#[test]
fn output_paths_cannot_leave_the_cache() {
    for good in ["models/hero/x.glb", "a.png", "textures/ground/x_orm.png"] {
        assert!(safe_path(good), "{good}");
    }
    for bad in [
        "",
        "/etc/passwd",
        "../x",
        "a/../x",
        "a//b",
        "./a",
        "a\\b",
        "C:/x",
        "a/b/",
        "nul\0",
        "é.png",
        &"a/".repeat(150),
    ] {
        assert!(!safe_path(bad), "{bad:?}");
    }
}

#[test]
fn the_plan_takes_every_line_and_the_tooltips() {
    let plan = Plan::parse(
        "model a models/a.vmdl_c models/a/a.glb anims=idle\n\
         model b models/b.vmdl_c models/b/b.glb meshes=m,n attach=a # c\n\
         texture t t.vtex_c textures/t.png\n\
         sound s s.vsnd_c sounds/s.wav event=e\n\
         world w w.vwrld_c map/w/w.gltf vpk=maps/dota.vpk half_width=1 v_min=-2 v_max=3 \
         layers=textures/ground trees=map/w/trees.txt\n\
         clip a idle idle 1.0\n\
         shop s scripts/shops.txt text/shop.json strings=loc.txt\n\
         shop t scripts/shops.txt text/t.json\n",
        "item 0 boots\nitem 9 recipe\nability 2 nevermore_requiem\nhero 1 nevermore\n\
         text ability 3 pudge_innate_graft_flesh\n",
    );
    assert_eq!(plan.items.len(), 8);
    assert!(matches!(
        &plan.items[0],
        Item::Model { anims, skeleton: true, .. } if anims == &["idle"]
    ));
    assert!(matches!(
        &plan.items[1],
        Item::Model { meshes: Some(m), skeleton: true, .. } if m == &["m", "n"]
    ));
    assert!(matches!(
        &plan.items[4],
        Item::Placements { trees_out, buildings_out, .. }
            if trees_out == "map/w/trees.txt" && buildings_out == "map/w/buildings.txt"
    ));
    assert!(matches!(
        &plan.items[5],
        Item::World { band, layers, .. }
            if band.half_width == 1.0 && band.v_min == -2.0 && band.v_max == 3.0
                && layers == "textures/ground"
    ));
    assert!(matches!(
        &plan.items[6],
        Item::Shop { src, strings, out, .. }
            if src == "scripts/shops.txt" && strings == "loc.txt" && out == "text/shop.json"
    ));
    assert!(matches!(
        &plan.items[7],
        Item::Tooltips { wanted, out, .. }
            if wanted.names == ["item_boots", "nevermore_requiem", "pudge_innate_graft_flesh"]
                && wanted.heroes == ["nevermore"] && out == "text/tooltips.json"
    ));
}

#[test]
fn a_hostile_asset_is_a_failure_in_the_report_not_a_panic() {
    let garbage = [0xA5u8; 300];
    let src = install(&[(
        "pak01_dir.vpk",
        package("textures", "bad", "vtex_c", &garbage),
    )]);
    let plan = Plan::parse(
        "texture bad textures/bad.vtex_c textures/bad.png\n\
         texture gone textures/gone.vtex_c textures/gone.png\n",
        "",
    );
    let mut sink = Memory::default();
    let report = run(&src, &plan, &mut sink, &mut |_| Flow::Continue).unwrap();
    assert_eq!(report.failed.len(), 2, "{report:?}");
    assert!(sink.0.is_empty());
    assert!(
        report
            .index()
            .starts_with(&format!("dbota-cache 1 rev {IMPORT_REV}\n"))
    );
}

#[test]
fn a_package_that_is_not_one_ends_the_import_and_a_stop_is_honoured() {
    let plan = Plan::parse("texture bad textures/bad.vtex_c textures/bad.png\n", "");
    let mut sink = Memory::default();
    let src = install(&[("pak01_dir.vpk", vec![7; 100])]);
    assert!(run(&src, &plan, &mut sink, &mut |_| Flow::Continue).is_err());
    let src = install(&[("pak01_dir.vpk", package("textures", "bad", "vtex_c", b"x"))]);
    let report = run(&src, &plan, &mut sink, &mut |_| Flow::Stop).unwrap();
    assert!(report.stopped && report.failed.is_empty());
    let none = SliceSource::<Vec<u8>>::default();
    assert!(matches!(
        run(&none, &plan, &mut sink, &mut |_| Flow::Continue),
        Err(Error::NotFound(_))
    ));
}

/// A "file" of absurd size whose header promises a directory tree of nearly 4 GB.
struct Huge;

impl dbota_assets::Source for Huge {
    fn size(&self, _: &str) -> dbota_assets::Result<u64> {
        Ok(1 << 40)
    }

    fn read_at(&self, _: &str, offset: u64, len: usize) -> dbota_assets::Result<Vec<u8>> {
        assert!(
            offset == 0 && len <= 28,
            "a read of {len} bytes at {offset} got through"
        );
        let mut head = Vec::new();
        for word in [0x55AA_1234u32, 2, 0xFFFF_FFF0, 0, 0, 48, 0] {
            head.extend_from_slice(&word.to_le_bytes());
        }
        Ok(head[..len].to_vec())
    }
}

#[test]
fn a_package_that_claims_a_huge_directory_is_refused_before_it_is_read() {
    let plan = Plan::parse("sound s s.vsnd_c sounds/s.wav\n", "");
    let mut sink = Memory::default();
    assert!(run(Huge, &plan, &mut sink, &mut |_| Flow::Continue).is_err());
}

#[test]
fn the_index_marks_the_files_read_at_start() {
    assert!(is_small("map/mid/trees.txt", 10));
    assert!(is_small("textures/ground/grass.png", 1 << 20));
    assert!(!is_small("textures/ground/big.png", 5 << 20));
    assert!(!is_small("models/x.glb", 10));
}

/// Tooltip texts out of damaged or hostile localization and script files: a text or an error,
/// never a panic, however deep the blocks nest.
#[test]
fn hostile_tooltip_texts_are_texts_or_errors() {
    let wanted = dbota_assets::Wanted::parse("item 0 boots\nability 1 x\nhero 0 h\n");
    let localization = "\u{feff}\"lang\" { \"Tokens\" {\n\
        \"DOTA_Tooltip_ability_item_boots\" \"Boots\"\n\
        \"DOTA_Tooltip_ability_item_boots_speed\" \"$move \\\"speed\\\"\"\n\
        \"dota_ability_variable_move\" \"Move\"\n\
        \"DOTA_SearchAlias_Ability_item_boots\" \"boots of speed; ;bos\"\n } }\n";
    let script = "\"DOTAAbilities\" { // c\n \"item_boots\" { \"ItemCost\" \"500\" \
        \"AbilityValues\" { \"speed\" { \"value\" \"45\" } } } /* x */ }";
    let json = dbota_assets::tooltips_json(&wanted, localization.as_bytes(), &[script.as_bytes()])
        .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(json["item_boots"]["cost"], "500");
    assert_eq!(json["item_boots"]["labels"][0][1], "Move \"speed\"");
    assert_eq!(
        json["item_boots"]["aliases"],
        serde_json::json!(["boots of speed", "bos"])
    );
    let deep = "\"a\" {".repeat(10_000);
    assert!(dbota_assets::tooltips_json(&wanted, b"", &[deep.as_bytes()]).is_err());
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let alphabet = b"\"{}\\/*$ \n\tab_0";
    for round in 0..2000 {
        let text: Vec<u8> = (0..(round % 300))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                alphabet[(seed % alphabet.len() as u64) as usize]
            })
            .collect();
        let _ = dbota_assets::tooltips_json(&wanted, &text, &[&text, script.as_bytes()]);
    }
}

/// The shop's categories keep Dota's order and repeated `item` keys, and its words come from the
/// localization; damaged or hostile files are a text or an error, never a panic.
#[test]
fn shop_categories_keep_their_order_and_hostile_files_are_texts_or_errors() {
    let shops = "\"dota_shops\" { \"consumables\" { \"item\" \"item_tpscroll\" // x\n \
        \"item\" \"item_clarity\" \"other\" { \"item\" \"item_no\" } }\n \
        \"misc\" { \"item\" \"item_boots\" } \"consumables\" { \"item\" \"item_again\" } }";
    let localization = "\"lang\" { \"Tokens\" {\n \"DOTA_SUBSHOP_MISC\" \"Miscellaneous\"\n\
        \"DOTA_Inventory_Grab_All\" \"GRAB ALL\"\n \"DOTA_Other\" \"x\"\n } }";
    let json = dbota_assets::shop_json(shops.as_bytes(), localization.as_bytes()).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(
        json["shops"],
        serde_json::json!([
            ["consumables", ["item_tpscroll", "item_clarity"]],
            ["misc", ["item_boots"]]
        ])
    );
    assert_eq!(json["strings"]["dota_subshop_misc"], "Miscellaneous");
    assert_eq!(json["strings"]["dota_inventory_grab_all"], "GRAB ALL");
    assert!(json["strings"].get("dota_other").is_none());
    assert!(dbota_assets::shop_json(b"} {", b"").is_err());
    assert!(dbota_assets::shop_json(&[0xff, 0xfe], b"").is_err());
    let deep = "\"a\" {".repeat(100_000);
    assert!(dbota_assets::shop_json(deep.as_bytes(), b"").is_ok());
    let mut seed = 0x9E37_79B9_7F4A_7C15u64;
    let alphabet = b"\"{}\\/*item_ \n\tab";
    for round in 0..2000 {
        let text: Vec<u8> = (0..(round % 300))
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                alphabet[(seed % alphabet.len() as u64) as usize]
            })
            .collect();
        let _ = dbota_assets::shop_json(&text, &text);
    }
}
