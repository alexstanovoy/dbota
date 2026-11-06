//! The importer's guards against a hostile install, without an install. The real import is
//! checked against the exported cache in `cache.rs`.

use std::collections::BTreeMap;

use dbota_import::import::{Flow, IMPORT_REV, Item, Plan, Sink, is_small, run, safe_path};
use dbota_import::text::{labels_json, shop_json, tooltips_json, wanted};
use heatel::{Error, SliceSource};

#[derive(Default)]
struct Memory(BTreeMap<String, Vec<u8>>);

impl Sink for Memory {
    fn write(&mut self, path: &str, data: &[u8]) -> heatel::Result<()> {
        self.0.insert(path.to_owned(), data.to_vec());
        Ok(())
    }
}

/// A package directory (VPK v2) with one `ext`-typed file `dir/name` whose bytes are in the
/// directory's preload.
fn package(dir: &str, name: &str, ext: &str, data: &[u8]) -> Vec<u8> {
    package_of(&[(dir, name, ext, data)])
}

/// A package directory (VPK v2) of several files `(dir, name, ext, bytes)`, all in the
/// directory's preload.
fn package_of(files: &[(&str, &str, &str, &[u8])]) -> Vec<u8> {
    let mut sorted = files.to_vec();
    sorted.sort_by_key(|(dir, name, ext, _)| (*ext, *dir, *name));
    let mut tree = Vec::new();
    let text = |tree: &mut Vec<u8>, part: &str| {
        tree.extend_from_slice(part.as_bytes());
        tree.push(0);
    };
    for (n, (dir, name, ext, data)) in sorted.iter().enumerate() {
        let before = n.checked_sub(1).map(|n| sorted[n]);
        if before.is_none_or(|(_, _, e, _)| e != *ext) {
            text(&mut tree, ext);
        }
        if before.is_none_or(|(d, _, e, _)| e != *ext || d != *dir) {
            text(&mut tree, dir);
        }
        text(&mut tree, name);
        tree.extend_from_slice(&crc32fast::hash(data).to_le_bytes());
        tree.extend_from_slice(&(data.len() as u16).to_le_bytes());
        tree.extend_from_slice(&0x7FFFu16.to_le_bytes());
        tree.extend_from_slice(&0u32.to_le_bytes());
        tree.extend_from_slice(&0u32.to_le_bytes());
        tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
        tree.extend_from_slice(data);
        let after = sorted.get(n + 1);
        // The end of a folder's files, then of a type's folders, then of the types.
        let same_folder = after.is_some_and(|(d, _, e, _)| e == ext && d == dir);
        let same_type = after.is_some_and(|(_, _, e, _)| e == ext);
        if !same_folder {
            tree.push(0);
            if !same_type {
                tree.push(0);
                if after.is_none() {
                    tree.push(0);
                }
            }
        }
    }
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
         label loc/dota_english.txt DOTA_HUD_Armor\n",
        "item 0 boots\nitem 9 recipe\nability 2 nevermore_requiem\nhero 1 nevermore\n\
         text ability 3 pudge_innate_graft_flesh\n",
    )
    .expect("a plan");
    assert_eq!(plan.items.len(), 13);
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
        Item::SoundEvents { events, out, .. }
            if events.iter().eq(["e"]) && out == "sounds/events.txt"
    ));
    assert!(matches!(
        &plan.items[8],
        Item::SoundMixer { out, .. } if out == "sounds/mixer.txt"
    ));
    assert!(matches!(
        &plan.items[9],
        Item::Tooltips { wanted, out, .. }
            if wanted.names == ["item_boots", "nevermore_requiem", "pudge_innate_graft_flesh"]
                && wanted.heroes == ["nevermore"] && out == "text/tooltips.json"
    ));
    assert!(matches!(
        &plan.items[11],
        Item::Icons {
            abilities: false,
            ..
        }
    ));
    assert!(matches!(
        &plan.items[10],
        Item::Labels { keys, out, .. }
            if keys == &[("loc/dota_english.txt".to_owned(), "DOTA_HUD_Armor".to_owned())]
                && out == "text/labels.json"
    ));
}

/// The index of `text` in the string table, added when it is not there.
fn intern(strings: &mut Vec<String>, text: &str) -> i32 {
    let at = strings.iter().position(|s| s == text).unwrap_or_else(|| {
        strings.push(text.to_owned());
        strings.len() - 1
    });
    at as i32
}

/// The few KV3 values the fake sound event files need.
enum Kv3 {
    Text(&'static str),
    Number(f64),
    List(Vec<Kv3>),
    Object(Vec<(&'static str, Kv3)>),
}

impl Kv3 {
    fn write(&self, strings: &mut Vec<String>, out: &mut Vec<u8>) {
        match self {
            Kv3::Text(text) => {
                out.push(6);
                out.extend_from_slice(&intern(strings, text).to_le_bytes());
            }
            Kv3::Number(n) => {
                out.push(5);
                out.extend_from_slice(&n.to_bits().to_le_bytes());
            }
            Kv3::List(items) => {
                out.push(8);
                out.extend_from_slice(&(items.len() as u32).to_le_bytes());
                for item in items {
                    item.write(strings, out);
                }
            }
            Kv3::Object(members) => {
                out.push(9);
                out.extend_from_slice(&(members.len() as u32).to_le_bytes());
                for (key, value) in members {
                    out.extend_from_slice(&intern(strings, key).to_le_bytes());
                    value.write(strings, out);
                }
            }
        }
    }
}

/// A compiled resource whose only block is `DATA`, the KV3 document (`VKV\x03`, stored).
fn kv3_resource(root: &Kv3) -> Vec<u8> {
    let mut strings: Vec<String> = Vec::new();
    let mut body = Vec::new();
    root.write(&mut strings, &mut body);
    let mut data = Vec::new();
    data.extend_from_slice(&0x0356_4B56u32.to_le_bytes());
    data.extend_from_slice(&[
        0x00, 0x05, 0x86, 0x1B, 0xD8, 0xF7, 0xC1, 0x40, 0xAD, 0x82, 0x75, 0xA4, 0x82, 0x67, 0xE7,
        0x14,
    ]);
    data.extend_from_slice(&[0; 16]);
    data.extend_from_slice(&(strings.len() as u32).to_le_bytes());
    for string in &strings {
        data.extend_from_slice(string.as_bytes());
        data.push(0);
    }
    data.extend_from_slice(&body);
    data.extend_from_slice(&u32::MAX.to_le_bytes());
    let words =
        |values: &[u32]| -> Vec<u8> { values.iter().flat_map(|v| v.to_le_bytes()).collect() };
    let table_end = 16 + 12;
    let mut out = words(&[(table_end + data.len()) as u32]);
    out.extend_from_slice(&12u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&words(&[8, 1]));
    let field = out.len() + 4;
    out.extend_from_slice(b"DATA");
    out.extend_from_slice(&words(&[(table_end - field) as u32, data.len() as u32]));
    out.extend_from_slice(&data);
    out
}

/// An install holding the event types and sound event files `(file, event names)`.
fn sound_install(files: &[(String, Vec<&'static str>)]) -> SliceSource<Vec<u8>> {
    let variable = |value: &'static str| {
        Kv3::Object(vec![
            ("data_type", Kv3::Text("float")),
            ("value", Kv3::Text(value)),
        ])
    };
    let stacks = kv3_resource(&Kv3::Object(vec![(
        "dota_src1_3d",
        Kv3::Object(vec![(
            "operators",
            Kv3::List(vec![Kv3::Object(vec![
                ("name", Kv3::Text("public")),
                (
                    "operator_variables",
                    Kv3::Object(vec![("block_duration", variable("0.05"))]),
                ),
            ])]),
        )]),
    )]));
    let events: Vec<Vec<u8>> = files
        .iter()
        .map(|(_, names)| {
            kv3_resource(&Kv3::Object(
                names
                    .iter()
                    .map(|name| {
                        (
                            *name,
                            Kv3::Object(vec![
                                ("type", Kv3::Text("dota_src1_3d")),
                                ("volume", Kv3::Number(1.0)),
                            ]),
                        )
                    })
                    .collect(),
            ))
        })
        .collect();
    let mut all: Vec<(&str, &str, &str, &[u8])> = vec![(
        "soundstacks",
        "soundstacks_dota_core",
        "vsndstck_c",
        &stacks,
    )];
    for ((name, _), bytes) in files.iter().zip(&events) {
        all.push(("soundevents", name, "vsndevts_c", bytes));
    }
    install(&[("pak01_dir.vpk", package_of(&all))])
}

fn sound_events_plan(names: &[&str]) -> Plan {
    Plan {
        items: vec![Item::SoundEvents {
            logical: "sound.events".to_owned(),
            events: names.iter().map(|n| (*n).to_owned()).collect(),
            out: "sounds/events.txt".to_owned(),
        }],
    }
}

/// An event a sound event file has is written and not reported; one no file has is reported
/// once, by its name.
#[test]
fn a_found_sound_event_is_written_and_a_missing_one_is_reported_once() {
    let src = sound_install(&[(
        "game_sounds_t".to_owned(),
        vec!["Found.Event", "Other.Event"],
    )]);
    let plan = sound_events_plan(&["Found.Event", "Missing.Event"]);
    let mut sink = Memory::default();
    let report = run(&src, &plan, &mut sink, &mut |_| Flow::Continue).expect("an import");
    let text = String::from_utf8(sink.0["sounds/events.txt"].clone()).expect("text");
    assert!(text.contains("\nevent Found.Event 0 "), "{text}");
    assert!(!text.contains("Other.Event"), "{text}");
    let missing = report
        .failed
        .iter()
        .filter(|(name, _)| name == "Missing.Event")
        .count();
    assert_eq!(missing, 1, "{:?}", report.failed);
    assert!(
        report.failed.iter().all(|(name, _)| name != "Found.Event"),
        "{:?}",
        report.failed
    );
}

/// An install with more sound event files than any install has ends the import with an error
/// that says so, and drops none of them.
#[test]
fn more_sound_event_files_than_any_install_has_is_an_error() {
    let files: Vec<(String, Vec<&'static str>)> = (0..2049)
        .map(|n| (format!("game_sounds_{n}"), vec!["E.Event"]))
        .collect();
    let src = sound_install(&files);
    let mut sink = Memory::default();
    let plan = sound_events_plan(&["E.Event"]);
    let report = run(&src, &plan, &mut sink, &mut |_| Flow::Continue).expect("a report");
    assert!(
        sink.0.is_empty(),
        "events were written from a truncated install"
    );
    assert_eq!(
        report.failed,
        [(
            "sound.events".to_owned(),
            "soundevents: more event files than any install has".to_owned()
        )]
    );
}

/// A voice line's name is a scene of the voice scripts, not a sound event: the plan asks the
/// sound event files for the sound events only, so that no real import reports every voice line
/// as missing.
#[test]
fn the_plan_asks_for_sound_events_and_not_for_voice_lines() {
    let plan = Plan::parse(
        "sound a sounds/weapons/a.vsnd_c sounds/weapons/a.wav event=Hero_A.Attack\n\
         sound b sounds/weapons/b.vsnd_c sounds/weapons/b.wav event=Hero_A.Attack\n\
         sound v sounds/vo/pudge/v.vsnd_c sounds/vo/pudge/v.mp3 event=pudge_pud_move_14\n\
         sound n sounds/vo/announcer/n.vsnd_c sounds/vo/announcer/n.mp3 event=announcer_n\n",
        "",
    )
    .expect("a plan");
    let asked: Vec<&std::collections::BTreeSet<String>> = plan
        .items
        .iter()
        .filter_map(|item| match item {
            Item::SoundEvents { events, .. } => Some(events),
            _ => None,
        })
        .collect();
    assert_eq!(asked.len(), 1);
    let events: Vec<&str> = asked[0].iter().map(String::as_str).collect();
    assert_eq!(events, ["Hero_A.Attack"]);
}

/// A line the importer cannot read is told, never dropped: a shop line without its words.
#[test]
fn a_line_that_cannot_be_read_stops_the_plan() {
    let errors = Plan::parse("shop t scripts/shops.txt text/t.json\n", "").expect_err("no strings");
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert!(
        errors[0].to_string().contains("manifest.txt line 1"),
        "{errors:?}"
    );
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
    )
    .expect("a plan");
    // Only the textures: the install here has no scripts for the rest to read.
    let plan = Plan {
        items: plan
            .items
            .into_iter()
            .filter(|i| matches!(i, Item::Texture { .. }))
            .collect(),
    };
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
    let plan =
        Plan::parse("texture bad textures/bad.vtex_c textures/bad.png\n", "").expect("a plan");
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

impl heatel::Source for Huge {
    fn size(&self, _: &str) -> heatel::Result<u64> {
        Ok(1 << 40)
    }

    fn read_at(&self, _: &str, offset: u64, len: usize) -> heatel::Result<Vec<u8>> {
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
    let plan = Plan::parse("sound s s.vsnd_c sounds/s.wav event=e\n", "").expect("a plan");
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
    let wanted = wanted(
        &dbota_import::catalog::parse("", "item 0 boots\nability 1 x\nhero 0 h\n")
            .expect("ids")
            .ids,
    );
    let localization = "\u{feff}\"lang\" { \"Tokens\" {\n\
        \"DOTA_Tooltip_ability_item_boots\" \"Boots\"\n\
        \"DOTA_Tooltip_ability_item_boots_speed\" \"$move \\\"speed\\\"\"\n\
        \"dota_ability_variable_move\" \"Move\"\n\
        \"DOTA_SearchAlias_Ability_item_boots\" \"boots of speed; ;bos\"\n } }\n";
    let script = "\"DOTAAbilities\" { // c\n \"item_boots\" { \"ItemCost\" \"500\" \
        \"AbilityValues\" { \"speed\" { \"value\" \"45\" } } } /* x */ }";
    let json = tooltips_json(&wanted, localization.as_bytes(), &[script.as_bytes()]).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(json["item_boots"]["cost"], "500");
    assert_eq!(json["item_boots"]["labels"][0][1], "Move \"speed\"");
    assert_eq!(
        json["item_boots"]["aliases"],
        serde_json::json!(["boots of speed", "bos"])
    );
    // A label is its key's text, a key no file has is left out, and so is
    // a text past any interface line's length.
    let keys = [
        ("loc", "DOTA_HUD_Armor"),
        ("loc", "missing"),
        ("loc", "long"),
    ]
    .map(|(f, k)| (f.to_owned(), k.to_owned()));
    let long = "x".repeat(5000);
    let loc = format!("\"DOTA_HUD_Armor\" \"Armor:\"\n\"long\" \"{long}\"\n");
    let labels = labels_json(&keys, &[("loc", loc.as_bytes())]).unwrap();
    let labels: serde_json::Value = serde_json::from_slice(&labels).unwrap();
    assert_eq!(labels, serde_json::json!({ "dota_hud_armor": "Armor:" }));
    let deep = "\"a\" {".repeat(10_000);
    assert!(tooltips_json(&wanted, b"", &[deep.as_bytes()]).is_err());
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
        let _ = tooltips_json(&wanted, &text, &[&text, script.as_bytes()]);
        let _ = labels_json(&keys, &[("loc", &text)]);
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
    let json = shop_json(shops.as_bytes(), localization.as_bytes()).unwrap();
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
    assert!(shop_json(b"} {", b"").is_err());
    assert!(shop_json(&[0xff, 0xfe], b"").is_err());
    let deep = "\"a\" {".repeat(100_000);
    assert!(shop_json(deep.as_bytes(), b"").is_ok());
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
        let _ = shop_json(&text, &text);
    }
}
