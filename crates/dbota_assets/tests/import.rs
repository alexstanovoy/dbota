//! The importer: its guards against a hostile install run anywhere; the real import of the
//! manifest (ignored: needs a Dota 2 install) writes into a folder and is compared with the
//! asset cache `tools/import_assets.sh` made: `cargo test --release -p dbota_assets --test
//! import -- --ignored`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dbota_assets::import::{Flow, IMPORT_REV, Item, Plan, Sink, is_small, run, safe_path};
use dbota_assets::{DirSource, Error, SliceSource, find_game_dir};

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
fn the_plan_takes_what_exists_and_defers_the_rest() {
    let plan = Plan::parse(
        "model a models/a.vmdl_c models/a/a.glb anims=idle\n\
         model b models/b.vmdl_c models/b/b.glb meshes=m,n attach=a # c\n\
         texture t t.vtex_c textures/t.png\n\
         sound s s.vsnd_c sounds/s.wav event=e\n\
         world w w.vwrld_c map/w.gltf vpk=maps/dota.vpk\n\
         clip a idle idle 1.0\n",
    );
    assert_eq!(plan.deferred, ["a", "w"]);
    assert_eq!(plan.items.len(), 3);
    assert!(matches!(
        &plan.items[0],
        Item::Model { meshes: Some(m), skeleton: true, .. } if m == &["m", "n"]
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
    let plan = Plan::parse("texture bad textures/bad.vtex_c textures/bad.png\n");
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
    let plan = Plan::parse("sound s s.vsnd_c sounds/s.wav\n");
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

fn repo() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn cache() -> PathBuf {
    let version = std::fs::read_to_string(repo().join("assets-manifest/version")).unwrap();
    std::env::var_os("DBOTA_ASSETS").map_or_else(
        || {
            let home = PathBuf::from(std::env::var_os("HOME").unwrap());
            home.join(".cache/dbota")
                .join(format!("v{}", version.trim()))
        },
        PathBuf::from,
    )
}

#[test]
#[ignore = "needs a local Dota 2 install and its asset cache"]
fn the_manifest_imports_and_the_sounds_match_the_exported_files() {
    let src = DirSource::new(find_game_dir(None).expect("Dota 2 not found"));
    let plan =
        Plan::parse(&std::fs::read_to_string(repo().join("assets-manifest/manifest.txt")).unwrap());
    let mut sink = Memory::default();
    let mut last = 0;
    let report = run(&src, &plan, &mut sink, &mut |p| {
        last = p.done;
        Flow::Continue
    })
    .unwrap();
    println!(
        "{} files, {} bytes, {} failed, {} deferred, last item {last}",
        report.files.len(),
        sink.0.values().map(Vec::len).sum::<usize>(),
        report.failed.len(),
        report.deferred.len()
    );
    assert!(report.failed.is_empty(), "{:#?}", report.failed);
    let mut checked = 0;
    for item in &plan.items {
        let Item::Sound { out, .. } = item else {
            continue;
        };
        let theirs = std::fs::read(cache().join(out)).unwrap();
        assert_eq!(sink.0[out], theirs, "{out}");
        checked += 1;
    }
    assert!(checked > 90);
    for item in &plan.items {
        let (Item::Model { out, .. } | Item::Texture { out, .. }) = item else {
            continue;
        };
        assert!(sink.0.contains_key(out), "{out}");
        assert!(!sink.0[out].is_empty());
    }
    assert!(
        report
            .index()
            .contains("models/hero/nevermore_head/nevermore_head.glb")
    );
}
