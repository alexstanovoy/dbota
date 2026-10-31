//! A prepared asset cache end to end in the real binary: `dbota import-cache` takes a folder or a
//! zip that someone made from a real import (the stamp and every file are the importer's own;
//! the cache here is the real one's small files), and `dbota export-cache` makes such a zip.
//! Everything in a prepared cache is untrusted: another build's stamp, a path that climbs out,
//! an index that does not match the files are all refused with their reason, and the cache in
//! place stays as it was. (The client starting on a cache taken this way draws on the GPU:
//! `menu_quit.rs`.)

mod common;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use common::prepared::{Scratch, dbota, read_folder, said, small_cache, write_folder, write_zip};
use dbota_import::import::{INDEX, STAMP, fingerprint};

/// What a case of the index tests does to the files of a prepared cache.
type Change = Box<dyn Fn(&mut BTreeMap<String, Vec<u8>>)>;

/// A prepared folder of the small cache in `scratch`.
fn prepared_folder(scratch: &Scratch) -> (std::path::PathBuf, BTreeMap<String, Vec<u8>>) {
    let files = small_cache();
    let folder = scratch.path("prepared");
    write_folder(&folder, &files);
    (folder, files)
}

fn succeeded(output: &std::process::Output) {
    assert!(output.status.success(), "{}", said(output));
}

/// Nothing but `cache` itself may have been made: no staging folder, no stray file.
fn assert_untouched(scratch: &Scratch, cache: &Path) {
    assert!(
        !cache.exists(),
        "{} was made by a refused cache",
        cache.display()
    );
    let names: Vec<_> = fs::read_dir(&scratch.0)
        .expect("the scratch folder")
        .map(|e| {
            e.expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.starts_with('.') || name.contains("escape"))
        .collect();
    assert!(names.is_empty(), "a refused cache left {names:?}");
}

#[test]
fn import_cache_of_a_folder_installs_every_file() {
    let scratch = Scratch::new("folder");
    let (folder, files) = prepared_folder(&scratch);
    let cache = scratch.path("assets");

    let output = dbota(&cache, "import-cache", &folder);

    succeeded(&output);
    assert_eq!(read_folder(&cache), files);
    assert!(
        !scratch.path(".assets.importing").exists(),
        "the staging folder is gone"
    );
}

#[test]
fn a_zip_exported_from_a_cache_loads_into_the_same_cache() {
    let scratch = Scratch::new("zip");
    let (folder, files) = prepared_folder(&scratch);
    let (made, loaded) = (scratch.path("made"), scratch.path("loaded"));
    succeeded(&dbota(&made, "import-cache", &folder));
    let zip = scratch.path("cache.zip");

    succeeded(&dbota(&made, "export-cache", &zip));
    let output = dbota(&loaded, "import-cache", &zip);

    succeeded(&output);
    assert_eq!(read_folder(&loaded), files);
}

#[test]
fn export_cache_does_not_overwrite_a_file() {
    let scratch = Scratch::new("overwrite");
    let (folder, _) = prepared_folder(&scratch);
    let zip = scratch.path("cache.zip");
    fs::write(&zip, b"mine").expect("a file");

    let output = dbota(&folder, "export-cache", &zip);

    assert!(!output.status.success());
    assert!(
        said(&output).contains("exists already"),
        "{}",
        said(&output)
    );
    assert_eq!(fs::read(&zip).expect("the file"), b"mine");
}

/// A zip another tool makes (Python's, deflated, with the cache's folder around it and folder
/// entries, as a file manager's "compress" does) is taken as well.
#[test]
fn a_zip_made_by_another_tool_with_a_folder_around_it_loads() {
    let scratch = Scratch::new("python");
    let (folder, files) = prepared_folder(&scratch);
    let zip = scratch.path("friend.zip");
    let script = "import os, sys, zipfile\n\
        with zipfile.ZipFile(sys.argv[2], 'w', zipfile.ZIP_DEFLATED) as z:\n\
        \x20   z.mkdir('dbota cache'.replace(' ', '_'))\n\
        \x20   for root, dirs, names in os.walk(sys.argv[1]):\n\
        \x20       for name in sorted(names):\n\
        \x20           path = os.path.join(root, name)\n\
        \x20           z.write(path, 'dbota_cache/' + os.path.relpath(path, sys.argv[1]))\n";
    let made = Command::new("python3")
        .args(["-I", "-c", script])
        .arg(&folder)
        .arg(&zip)
        .status()
        .expect("python3 runs");
    assert!(made.success());
    let cache = scratch.path("assets");

    let output = dbota(&cache, "import-cache", &zip);

    succeeded(&output);
    assert_eq!(read_folder(&cache), files);
}

#[test]
fn a_cache_of_another_build_is_refused_naming_both_builds() {
    for zipped in [false, true] {
        let scratch = Scratch::new(if zipped { "stale-zip" } else { "stale-folder" });
        let mut files = small_cache();
        files.insert(STAMP.to_owned(), b"rev1.1/0000000000000000/1/2".to_vec());
        let source = scratch.path("prepared");
        if zipped {
            let source = scratch.path("prepared.zip");
            write_zip(
                &source,
                files.iter().map(|(n, b)| (n.as_str(), b.as_slice())),
            );
        } else {
            write_folder(&source, &files);
        }
        let from = if zipped {
            scratch.path("prepared.zip")
        } else {
            source
        };
        let cache = scratch.path("assets");

        let output = dbota(&cache, "import-cache", &from);

        assert!(!output.status.success());
        let said = said(&output);
        assert!(
            said.contains("is for assets rev1.1/0000000000000000"),
            "{said}"
        );
        assert!(
            said.contains(&format!("needs assets {}", fingerprint())),
            "{said}"
        );
        assert_untouched(&scratch, &cache);
    }
}

/// A refused cache leaves the one in place alone.
#[test]
fn a_refused_cache_leaves_the_cache_in_place() {
    let scratch = Scratch::new("keeps");
    let (folder, files) = prepared_folder(&scratch);
    let cache = scratch.path("assets");
    succeeded(&dbota(&cache, "import-cache", &folder));
    fs::write(folder.join(STAMP), "rev1.1/0/1/2").expect("a stamp");

    let output = dbota(&cache, "import-cache", &folder);

    assert!(!output.status.success());
    assert_eq!(read_folder(&cache), files);
}

/// Names a cache must not hold, each in a zip whose stamp is right and whose index lists the
/// file, so only its name is wrong.
#[test]
fn a_zip_with_a_file_it_must_not_hold_is_refused_and_nothing_is_written() {
    let bad = [
        "../escape.png",
        "/escape.png",
        "hud/../../escape.png",
        "hud/../escape.png",
        "hud\\escape.png",
        "hud/escape.exe",
        "hud/escape",
        "escape.png",
        "evil/escape.png",
    ];
    for name in bad {
        let scratch = Scratch::new("unsafe");
        let mut files = small_cache();
        files.insert(name.to_owned(), vec![7; 16]);
        let index = format!(
            "{}file 16 {name}\n",
            String::from_utf8(files[INDEX].clone()).expect("text")
        );
        files.insert(INDEX.to_owned(), index.into_bytes());
        let zip = scratch.path("evil.zip");
        write_zip(&zip, files.iter().map(|(n, b)| (n.as_str(), b.as_slice())));
        let cache = scratch.path("assets");

        let output = dbota(&cache, "import-cache", &zip);

        assert!(!output.status.success(), "{name} was taken");
        let said = said(&output);
        let shown: String = name
            .chars()
            .map(|c| if c.is_ascii_graphic() { c } else { '?' })
            .collect();
        assert!(
            said.contains(&format!("holds a file it must not: {shown}")),
            "{name}: {said}"
        );
        assert_untouched(&scratch, &cache);
        assert!(
            !scratch
                .0
                .parent()
                .expect("a parent")
                .join("escape.png")
                .exists()
        );
    }
}

/// Each way the index can disagree with the files present, with what the player is told.
#[test]
fn an_index_that_does_not_match_the_files_is_refused() {
    let small = small_cache();
    let some = small
        .keys()
        .find(|p| p.starts_with("hud/"))
        .expect("a hud file")
        .clone();
    let size = small[&some].len();
    let index = String::from_utf8(small[INDEX].clone()).expect("text");
    let cases: Vec<(&str, Change, String)> = vec![
        (
            "a file the index does not list",
            Box::new(|f| {
                f.insert("hud/unlisted.png".to_owned(), vec![1]);
            }),
            "hud/unlisted.png is in it, but index.txt does not list it".to_owned(),
        ),
        (
            "a listed file that is missing",
            Box::new({
                let some = some.clone();
                move |f| {
                    f.remove(&some);
                }
            }),
            format!("index.txt lists {some}, which is not in it"),
        ),
        (
            "a file of another size than listed",
            Box::new({
                let some = some.clone();
                move |f| {
                    f.get_mut(&some).expect("the file").push(0);
                }
            }),
            format!("{some} is {} bytes, but index.txt says {size}", size + 1),
        ),
        (
            "a line twice",
            Box::new({
                let line = index.lines().nth(1).expect("a line").to_owned();
                move |f| {
                    let text = String::from_utf8(f[INDEX].clone()).expect("text");
                    f.insert(INDEX.to_owned(), format!("{text}{line}\n").into_bytes());
                }
            }),
            "again".to_owned(),
        ),
        (
            "a wrong header",
            Box::new(|f| {
                f.insert(INDEX.to_owned(), b"dbota-cache 9 rev 1\n".to_vec());
            }),
            "index.txt starts with".to_owned(),
        ),
        (
            "no index",
            Box::new(|f| {
                f.remove(INDEX);
            }),
            "it has no index.txt at its top".to_owned(),
        ),
        (
            "no stamp",
            Box::new(|f| {
                f.remove(STAMP);
            }),
            "it has no stamp.txt at its top".to_owned(),
        ),
        (
            "an index that is not text",
            Box::new(|f| {
                f.insert(INDEX.to_owned(), vec![0xff, 0xfe, 0]);
            }),
            "index.txt is not text".to_owned(),
        ),
    ];
    for (what, change, message) in cases {
        let scratch = Scratch::new("index");
        let mut files = small.clone();
        change(&mut files);
        let source = scratch.path("prepared");
        write_folder(&source, &files);
        let cache = scratch.path("assets");

        let output = dbota(&cache, "import-cache", &source);

        assert!(!output.status.success(), "{what} was taken");
        assert!(
            said(&output).contains(&message),
            "{what}: {}",
            said(&output)
        );
        assert_untouched(&scratch, &cache);
    }
}

#[test]
fn a_file_that_is_not_a_cache_is_refused() {
    let scratch = Scratch::new("garbage");
    let junk = scratch.path("junk.zip");
    fs::write(&junk, vec![0x50; 4096]).expect("a file");

    let output = dbota(&scratch.path("assets"), "import-cache", &junk);

    assert!(!output.status.success());
    assert!(
        said(&output).contains("it is not a zip file"),
        "{}",
        said(&output)
    );
    assert_untouched(&scratch, &scratch.path("assets"));
}

/// A run nobody watches (a capture) has no asset screen to show: without a cache of this build it
/// ends at once with the reason and the way to a cache, drawing nothing.
#[test]
fn an_unattended_run_without_assets_ends_with_the_reason() {
    let scratch = Scratch::new("unattended");
    let cache = scratch.path("assets");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dbota"))
        .arg("--cache-dir")
        .arg(&cache)
        .args(["--replay", "none.brp", "--screenshot-at", "10"])
        .env_remove("DBOTA_ASSETS")
        .output()
        .expect("dbota runs");

    assert_eq!(output.status.code(), Some(2), "{}", said(&output));
    let said = said(&output);
    assert!(said.contains("no asset cache at"), "{said}");
    assert!(said.contains("dbota import-cache"), "{said}");
    assert!(!cache.exists());
}
