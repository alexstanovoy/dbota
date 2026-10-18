//! A package built here, read back through the in-memory `Source` the browser's files will
//! back: the VPK layout contract without a Dota install.

use dbota_assets::{Error, SliceSource, Vpk};

struct File {
    ext: &'static str,
    dir: &'static str,
    name: &'static str,
    preload: &'static [u8],
    /// Archive part, or 0x7FFF for the directory file's own data section.
    archive: u16,
    offset: u32,
    data: &'static [u8],
}

/// The directory file: a v2 header, the tree (extension → directory → name), then the data
/// of the files stored in it.
fn directory(files: &[File], in_dir: &[u8]) -> Vec<u8> {
    let mut tree = Vec::new();
    let cstr = |tree: &mut Vec<u8>, s: &str| {
        tree.extend_from_slice(s.as_bytes());
        tree.push(0);
    };
    for ext in ["vmdl_c", " "] {
        cstr(&mut tree, ext);
        for dir in ["models/heroes", " "] {
            let group: Vec<&File> = files
                .iter()
                .filter(|f| f.ext == ext && f.dir == dir)
                .collect();
            if group.is_empty() {
                continue;
            }
            cstr(&mut tree, dir);
            for f in group {
                cstr(&mut tree, f.name);
                let whole = [f.preload, f.data].concat();
                tree.extend_from_slice(&crc32fast::hash(&whole).to_le_bytes());
                tree.extend_from_slice(&(f.preload.len() as u16).to_le_bytes());
                tree.extend_from_slice(&f.archive.to_le_bytes());
                tree.extend_from_slice(&f.offset.to_le_bytes());
                tree.extend_from_slice(&(f.data.len() as u32).to_le_bytes());
                tree.extend_from_slice(&0xFFFFu16.to_le_bytes());
                tree.extend_from_slice(f.preload);
            }
            tree.push(0);
        }
        tree.push(0);
    }
    tree.push(0);
    let mut out = Vec::new();
    for word in [
        0x55AA_1234,
        2,
        tree.len() as u32,
        in_dir.len() as u32,
        0,
        48,
        0,
    ] {
        out.extend_from_slice(&u32::to_le_bytes(word));
    }
    out.extend_from_slice(&tree);
    out.extend_from_slice(in_dir);
    out
}

const FILES: [File; 3] = [
    File {
        ext: "vmdl_c",
        dir: "models/heroes",
        name: "pudge",
        preload: b"pre",
        archive: 1,
        offset: 4,
        data: b"pudge body",
    },
    File {
        ext: "vmdl_c",
        dir: "models/heroes",
        name: "nevermore",
        preload: b"",
        archive: 0x7FFF,
        offset: 2,
        data: b"souls",
    },
    File {
        ext: " ",
        dir: " ",
        name: "readme",
        preload: b"all in the tree",
        archive: 0x7FFF,
        offset: 0,
        data: b"",
    },
];

fn package(part1: &[u8]) -> SliceSource<Vec<u8>> {
    let mut src = SliceSource::default();
    src.insert("pak01_dir.vpk", directory(&FILES, b"..souls"));
    src.insert("pak01_001.vpk", part1.to_vec());
    src
}

#[test]
fn files_read_back_from_parts_the_directory_and_the_tree() {
    let src = package(b"....pudge body..");
    let vpk = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    assert_eq!(vpk.len(), 3);
    let paths: Vec<String> = vpk.entries().map(|e| e.path()).collect();
    assert_eq!(
        paths,
        [
            "models/heroes/pudge.vmdl_c",
            "models/heroes/nevermore.vmdl_c",
            "readme"
        ]
    );
    let read = |path: &str| vpk.read_file(&src, path).unwrap();
    assert_eq!(read("models/heroes/pudge.vmdl_c"), b"prepudge body");
    assert_eq!(read("models\\heroes\\nevermore.vmdl_c"), b"souls");
    assert_eq!(read("readme"), b"all in the tree");
    assert_eq!(vpk.get("models/heroes/pudge.vmdl_c").unwrap().size(), 13);
    assert!(vpk.get("models/heroes/pudge.vmdl").is_none());
    assert!(matches!(
        vpk.read_file(&src, "pudge.vmdl_c"),
        Err(Error::NotFound(_))
    ));
}

#[test]
fn damaged_packages_are_errors() {
    let src = package(b"....pudge bodY..");
    let vpk = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let pudge = "models/heroes/pudge.vmdl_c";
    assert!(matches!(vpk.read_file(&src, pudge), Err(Error::Crc { .. })));

    let src = package(b"....pudge");
    let vpk = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    assert!(matches!(
        vpk.read_file(&src, pudge),
        Err(Error::OutOfRange { .. })
    ));

    let whole = directory(&FILES, b"..souls");
    for len in 0..whole.len() - 7 {
        let mut src = SliceSource::default();
        src.insert("pak01_dir.vpk", whole[..len].to_vec());
        assert!(
            Vpk::open(&src, "pak01_dir.vpk").is_err(),
            "truncated to {len}"
        );
    }
}
