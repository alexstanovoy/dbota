use std::sync::Mutex;

use super::{decode_index_buffer, decode_vertex_buffer};
use crate::kv3::Value;
use crate::{DirSource, Resource, Vpk, find_game_dir};

/// The encoder's index version is a process-wide setting, so index encodings are serialised.
static INDEX_ENCODER: Mutex<()> = Mutex::new(());

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Vertices of `S` bytes: random, a smooth curve with a little noise (positions, uvs), or a
/// few live fields among constant and zero ones (a typical mix).
fn vertices<const S: usize>(count: usize, kind: usize, seed: u64) -> Vec<[u8; S]> {
    let mut rng = Rng(seed | 1);
    (0..count)
        .map(|i| {
            let mut v = [0u8; S];
            for (j, byte) in v.iter_mut().enumerate() {
                let r = rng.next();
                *byte = match kind {
                    0 => r as u8,
                    1 => ((i * (j % 4 + 1)) / (j / 4 + 1) + usize::from(r as u8 & 3)) as u8,
                    _ => match (j / 4) % 3 {
                        0 => 7,
                        1 => 0,
                        _ => (i / 5 + usize::from(r as u8 & 1)) as u8,
                    },
                };
            }
            v
        })
        .collect()
}

fn round_trip_vertices<const S: usize>(counts: &[usize]) -> usize {
    let mut checked = 0;
    for &count in counts {
        for kind in 0..3 {
            let data = vertices::<S>(count, kind, (count * 31 + kind) as u64);
            let raw: Vec<u8> = data.iter().flatten().copied().collect();
            for (version, level) in [(0, 2), (1, 0), (1, 1), (1, 2), (1, 3)] {
                let enc = meshopt::encode_vertex_buffer_level(&data, level, Some(version)).unwrap();
                let dec = decode_vertex_buffer(&enc, count, S).unwrap();
                assert!(
                    dec == raw,
                    "stride {S} count {count} kind {kind} v{version} l{level}"
                );
                checked += 1;
            }
        }
    }
    checked
}

#[test]
fn vertex_buffers_decode_like_the_c_library() {
    let counts = [0, 1, 2, 15, 16, 17, 100, 255, 256, 257, 1000, 4097];
    let mut checked = round_trip_vertices::<4>(&counts)
        + round_trip_vertices::<8>(&counts)
        + round_trip_vertices::<12>(&counts)
        + round_trip_vertices::<16>(&counts)
        + round_trip_vertices::<20>(&counts)
        + round_trip_vertices::<32>(&counts)
        + round_trip_vertices::<48>(&counts)
        + round_trip_vertices::<256>(&counts[..10]);
    checked += round_trip_vertices::<12>(&[200_000]) + round_trip_vertices::<32>(&[60_000]);
    assert!(checked > 700);
}

/// A triangle list with the vertex cache order of a real mesh: a grid of `n * n` quads.
fn grid(n: u32) -> (Vec<u32>, usize) {
    let w = n + 1;
    let mut indices = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let (a, b, c, d) = (
                y * w + x,
                y * w + x + 1,
                (y + 1) * w + x,
                (y + 1) * w + x + 1,
            );
            indices.extend([a, b, c, b, d, c]);
        }
    }
    let vertices = (w * w) as usize;
    (meshopt::optimize_vertex_cache(&indices, vertices), vertices)
}

fn le_bytes(indices: &[u32], size: usize) -> Vec<u8> {
    indices
        .iter()
        .flat_map(|&i| i.to_le_bytes().into_iter().take(size))
        .collect()
}

/// Encodes with the given index codec version; the setting is global to the C library.
fn encode_indices(version: i32, sequence: bool, indices: &[u32], vertices: usize) -> Vec<u8> {
    let _lock = INDEX_ENCODER.lock().unwrap_or_else(|e| e.into_inner());
    // SAFETY: the lock above is the only caller of the encoders in this process.
    unsafe { meshopt::encode_index_version(version) };
    if sequence {
        meshopt::encode_index_sequence(indices, vertices).unwrap()
    } else {
        meshopt::encode_index_buffer(indices, vertices).unwrap()
    }
}

/// The codec may rotate a triangle's corners, never change its winding.
fn rotated_minimum_first(indices: &[u32]) -> Vec<[u32; 3]> {
    indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| {
            let at = (0..3).min_by_key(|&i| t[i]).unwrap();
            [t[at], t[(at + 1) % 3], t[(at + 2) % 3]]
        })
        .collect()
}

fn as_u32(bytes: &[u8], size: usize) -> Vec<u32> {
    bytes
        .chunks_exact(size)
        .map(|c| c.iter().rev().fold(0, |acc, &b| acc << 8 | u32::from(b)))
        .collect()
}

#[test]
fn index_buffers_decode_like_the_c_library() {
    let mut rng = Rng(99);
    let soup: Vec<u32> = (0..3000).map(|_| (rng.next() % 500) as u32).collect();
    let big = (0..90_000u32)
        .map(|i| (i / 3) * 2 + i % 3)
        .collect::<Vec<_>>();
    let cases = [
        grid(1),
        grid(7),
        grid(60),
        (soup, 500),
        (big, 60_001),
        (vec![], 0),
    ];
    for (indices, vertices) in &cases {
        for version in [0, 1] {
            for size in [2, 4] {
                if size == 2 && *vertices > 65_536 {
                    continue;
                }
                let tri = encode_indices(version, false, indices, *vertices);
                let got = decode_index_buffer(&tri, indices.len(), size).unwrap();
                let c_ref = if size == 2 {
                    meshopt::decode_index_buffer::<u16>(&tri, indices.len())
                        .unwrap()
                        .iter()
                        .flat_map(|i| i.to_le_bytes())
                        .collect()
                } else {
                    le_bytes(
                        &meshopt::decode_index_buffer::<u32>(&tri, indices.len()).unwrap(),
                        4,
                    )
                };
                assert!(
                    got == c_ref,
                    "{} indices v{version} size {size}",
                    indices.len()
                );
                assert!(
                    rotated_minimum_first(&as_u32(&got, size)) == rotated_minimum_first(indices)
                );
                let seq = encode_indices(version, true, indices, *vertices);
                let got = decode_index_buffer(&seq, indices.len(), size).unwrap();
                assert!(got == le_bytes(indices, size));
            }
        }
    }
}

/// Every truncation and a bit flip in every byte of a few encodings: errors or data, no panic.
#[test]
fn damaged_buffers_never_panic() {
    let data = vertices::<20>(300, 1, 5);
    let (grid_indices, grid_vertices) = grid(6);
    let mut buffers = vec![(
        false,
        300,
        20,
        meshopt::encode_vertex_buffer(&data).unwrap(),
    )];
    buffers.push((
        false,
        300,
        20,
        meshopt::encode_vertex_buffer_level(&data, 3, Some(0)).unwrap(),
    ));
    for (sequence, version) in [(false, 0), (false, 1), (true, 1)] {
        let enc = encode_indices(version, sequence, &grid_indices, grid_vertices);
        buffers.push((true, grid_indices.len(), 2, enc));
    }
    for (is_index, count, size, enc) in &buffers {
        let decode = |bytes: &[u8]| {
            let _ = if *is_index {
                decode_index_buffer(bytes, *count, *size)
            } else {
                decode_vertex_buffer(bytes, *count, *size)
            };
        };
        for len in 0..enc.len() {
            decode(&enc[..len]);
        }
        for at in 0..enc.len() {
            for bit in 0..8 {
                let mut bad = enc.clone();
                bad[at] ^= 1 << bit;
                decode(&bad);
            }
        }
    }
}

#[test]
fn oversized_requests_are_refused() {
    let enc = meshopt::encode_vertex_buffer(&[[0u8; 4]; 2]).unwrap();
    assert!(decode_vertex_buffer(&enc, usize::MAX / 2, 4).is_err());
    assert!(decode_vertex_buffer(&enc, 1 << 30, 4).is_err());
    assert!(decode_index_buffer(&enc, 1 << 30, 4).is_err());
    assert!(decode_vertex_buffer(&enc, 2, 6).is_err());
}

/// Totals of the real-data run.
#[derive(Default, Debug)]
struct Totals {
    models: usize,
    skipped_models: usize,
    meshes: usize,
    vertex_buffers: usize,
    index_buffers: usize,
    sequences: usize,
    raw_buffers: usize,
    zstd_buffers: usize,
    vertices: u64,
    indices: u64,
    encoded_bytes: u64,
    decoded_bytes: u64,
}

fn field(v: &Value, key: &str) -> u64 {
    v.get(key)
        .and_then(Value::as_u64)
        .unwrap_or_else(|| panic!("no {key}"))
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

/// The bytes of one buffer description of a mesh: its block, unzstd-ed, then unmeshopt-ed
/// (`index` buffers only: whether the stream is an index sequence is checked on the way).
fn buffer_bytes(res: &Resource, buf: &Value, index: bool, totals: &mut Totals) -> Vec<u8> {
    let (count, size) = (
        field(buf, "m_nElementCount") as usize,
        field(buf, "m_nElementSizeInBytes") as usize,
    );
    let block = &res.blocks()[field(buf, "m_nBlockIndex") as usize];
    assert_eq!(&block.kind, if index { b"MIDX" } else { b"MVTX" });
    let mut bytes = res.bytes(block).to_vec();
    totals.encoded_bytes += bytes.len() as u64;
    if flag(buf, "m_bCompressedZSTD") {
        let mut plain = Vec::new();
        let mut zstd = ruzstd::decoding::StreamingDecoder::new(bytes.as_slice()).unwrap();
        std::io::Read::read_to_end(
            &mut std::io::Read::take(&mut zstd, super::MAX_OUTPUT as u64),
            &mut plain,
        )
        .unwrap();
        bytes = plain;
        totals.zstd_buffers += 1;
    }
    if flag(buf, "m_bMeshoptCompressed") {
        if index {
            let sequence = bytes[0] & 0xf0 == super::SEQUENCE_HEADER;
            assert_eq!(sequence, flag(buf, "m_bMeshoptIndexSequence"));
            totals.sequences += usize::from(sequence);
            bytes = decode_index_buffer(&bytes, count, size).unwrap();
        } else {
            bytes = decode_vertex_buffer(&bytes, count, size).unwrap();
        }
    } else {
        totals.raw_buffers += 1;
    }
    assert_eq!(bytes.len(), count * size);
    totals.decoded_bytes += bytes.len() as u64;
    bytes
}

/// Decodes every buffer of every embedded mesh of the manifest's models, and checks each index
/// points into its mesh's vertex buffer. Needs the Dota 2 install (`DBOTA_DOTA_PATH`).
#[test]
#[ignore = "needs the Dota 2 install"]
fn every_model_buffer_decodes() {
    let dota = std::env::var_os("DBOTA_DOTA_PATH").map(std::path::PathBuf::from);
    let src = DirSource::new(
        find_game_dir(dota.as_deref()).expect("Dota 2 not found; set DBOTA_DOTA_PATH"),
    );
    let (pak, map) = (
        Vpk::open(&src, "pak01_dir.vpk").unwrap(),
        Vpk::open(&src, "maps/dota.vpk").unwrap(),
    );
    let manifest = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets-manifest/manifest.txt"
    ))
    .unwrap();
    let mut totals = Totals::default();
    for path in manifest.lines().filter_map(|l| {
        let mut words = l.split('#').next()?.split_whitespace();
        (words.next()? == "model").then(|| words.nth(1)).flatten()
    }) {
        let vpk = if pak.get(path).is_some() { &pak } else { &map };
        let bytes = vpk.read_file(&src, path).unwrap();
        let res = Resource::parse(&bytes).unwrap();
        let ctrl = res.kv3(b"CTRL").unwrap().unwrap().root;
        let meshes = ctrl
            .get("embedded_meshes")
            .and_then(Value::as_array)
            .unwrap();
        if meshes.iter().any(|m| m.get("m_vertexBuffers").is_none()) {
            totals.skipped_models += 1; // VBIB blocks, another reader's job
            continue;
        }
        totals.models += 1;
        for mesh in meshes {
            totals.meshes += 1;
            let vbs = mesh
                .get("m_vertexBuffers")
                .and_then(Value::as_array)
                .unwrap();
            let ibs = mesh
                .get("m_indexBuffers")
                .and_then(Value::as_array)
                .unwrap();
            for vb in vbs {
                buffer_bytes(&res, vb, false, &mut totals);
                totals.vertex_buffers += 1;
                totals.vertices += field(vb, "m_nElementCount");
            }
            let vertex_count = field(&vbs[0], "m_nElementCount");
            for ib in ibs {
                let size = field(ib, "m_nElementSizeInBytes") as usize;
                let indices = as_u32(&buffer_bytes(&res, ib, true, &mut totals), size);
                assert!(
                    indices.iter().all(|&i| u64::from(i) < vertex_count),
                    "{path}: index past {vertex_count} vertices"
                );
                totals.index_buffers += 1;
                totals.indices += indices.len() as u64;
            }
        }
    }
    eprintln!("{totals:#?}");
    assert!(totals.index_buffers > 0 && totals.vertex_buffers > 0);
}
