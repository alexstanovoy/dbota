//! Resources built here that once blew up a reader: NTRO and RERL blocks whose lists and strings
//! overlap (gigabytes from a small file), NTRO arrays of empty structs and pointer cycles (an
//! allocation of the declared count, the stack), and KV3 claiming more than its bytes can hold.
//! Each is an error now.

use dbota_assets::{Resource, kv3};

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// The offset from the field at `field` to `to`, as the formats write it.
fn rel(to: usize, field: usize) -> u32 {
    (to as i64 - field as i64) as i32 as u32
}

/// A compiled resource of `blocks`; each block's offset counts from its own field.
fn resource(blocks: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
    let table_end = 16 + 12 * blocks.len();
    let size = table_end + blocks.iter().map(|(_, b)| b.len()).sum::<usize>();
    let mut out = words(&[size as u32]);
    out.extend_from_slice(&12u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&words(&[8, blocks.len() as u32]));
    let mut at = table_end;
    for (kind, block) in blocks {
        let field = out.len() + 4;
        out.extend_from_slice(*kind);
        out.extend_from_slice(&words(&[rel(at, field), block.len() as u32]));
        at += block.len();
    }
    for (_, block) in blocks {
        out.extend_from_slice(block);
    }
    out
}

/// A struct of an NTRO block: its fields are a range of the block's fields, which may overlap
/// another struct's.
struct Struct {
    id: u32,
    size: u16,
    fields: (usize, usize),
}

/// A field of an NTRO block; its name and indirections are offsets into the block's tail.
struct Field {
    name: Option<usize>,
    indirections: (usize, u32),
    type_data: u32,
    ty: u16,
}

/// An NTRO block: its header, the structs, the fields (each one element at offset 0), the tail.
fn ntro(structs: &[Struct], fields: &[Field], tail: &[u8]) -> Vec<u8> {
    let fields_at = 20 + 40 * structs.len();
    let tail_at = fields_at + 24 * fields.len();
    let mut out = words(&[0, rel(20, 4), structs.len() as u32, 0, 0]);
    for s in structs {
        let at = out.len();
        out.extend(words(&[0, s.id, 0, 0, 0]));
        out.extend(words(&[u32::from(s.size)]));
        let first = fields_at + 24 * s.fields.0;
        out.extend(words(&[0, rel(first, at + 28), s.fields.1 as u32, 0]));
    }
    for f in fields {
        let at = out.len();
        let name = f.name.map_or(0, |name| rel(tail_at + name, at));
        let indirections = rel(tail_at + f.indirections.0, at + 8);
        out.extend(words(&[
            name,
            1,
            indirections,
            f.indirections.1,
            f.type_data,
        ]));
        out.extend(words(&[u32::from(f.ty)]));
    }
    out.extend_from_slice(tail);
    out
}

const STRUCT: u16 = 1;
const INT: u16 = 6;
const POINTER: u8 = 3;
const ARRAY: u8 = 4;

fn data_of(ntro: Vec<u8>, data: Vec<u8>) -> bool {
    let file = resource(&[(b"NTRO", ntro), (b"DATA", data)]);
    let resource = Resource::parse(&file).unwrap();
    resource
        .data()
        .expect("a DATA block laid out by NTRO")
        .is_err()
}

#[test]
fn an_ntro_array_of_four_billion_empty_structs_is_an_error() {
    let structs = [
        Struct {
            id: 1,
            size: 8,
            fields: (0, 1),
        },
        Struct {
            id: 2,
            size: 0,
            fields: (0, 0),
        },
    ];
    let array = Field {
        name: None,
        indirections: (0, 1),
        type_data: 2,
        ty: STRUCT,
    };
    let block = ntro(&structs, &[array], &[ARRAY]);
    assert!(data_of(block, words(&[0, u32::MAX])));
}

#[test]
fn an_ntro_pointer_cycle_a_hundred_thousand_deep_is_an_error() {
    let pointers = 100_000;
    let field = Field {
        name: None,
        indirections: (0, pointers),
        type_data: 0,
        ty: INT,
    };
    let block = ntro(
        &[Struct {
            id: 1,
            size: 16,
            fields: (0, 1),
        }],
        &[field],
        &vec![POINTER; pointers as usize],
    );
    assert!(data_of(block, words(&[8, 0, -8i32 as u32, 0])));
}

#[test]
fn ntro_structs_sharing_fields_and_names_are_charged_for_every_copy() {
    // 160 structs of the same 128 fields, each named by the same 16 KiB: 320 MiB of names.
    let structs: Vec<Struct> = (1..=160)
        .map(|id| Struct {
            id,
            size: 4,
            fields: (0, 128),
        })
        .collect();
    let fields: Vec<Field> = (0..128)
        .map(|_| Field {
            name: Some(0),
            indirections: (0, 0),
            type_data: 0,
            ty: INT,
        })
        .collect();
    let mut name = vec![b'x'; 16 << 10];
    name.push(0);
    assert!(data_of(ntro(&structs, &fields, &name), words(&[0])));
}

#[test]
fn rerl_references_sharing_one_name_are_charged_for_every_copy() {
    // 300 references to the same 1 MiB name.
    let count = 300;
    let mut block = words(&[8, count as u32]);
    let name_at = 8 + 16 * count;
    for i in 0..count {
        let field = 8 + 16 * i + 8;
        block.extend(words(&[i as u32, 0, rel(name_at, field), 0]));
    }
    block.extend(vec![b'x'; 1 << 20]);
    block.push(0);
    let file = resource(&[(b"RERL", block)]);
    assert!(Resource::parse(&file).unwrap().external_refs().is_err());
}

#[test]
fn kv3_that_cannot_hold_what_it_claims_is_refused_before_allocating() {
    const LZ4: [u8; 16] = [
        0x8A, 0x34, 0x47, 0x68, 0xA1, 0x63, 0x5C, 0x4F, 0xA1, 0x97, 0x53, 0x80, 0x6F, 0xD9, 0xB1,
        0x19,
    ];
    const BLOCK_COMPRESSED: [u8; 16] = [
        0x46, 0x1A, 0x79, 0x95, 0xBC, 0x95, 0x6C, 0x4F, 0xA7, 0x0B, 0x05, 0xBC, 0xA1, 0xB7, 0xDF,
        0xD2,
    ];
    for encoding in [LZ4, BLOCK_COMPRESSED] {
        let mut doc = b"VKV\x03".to_vec();
        doc.extend_from_slice(&encoding);
        doc.extend_from_slice(&[0; 16]);
        doc.extend(words(&[200 << 20, 0, 0]));
        let err = kv3::decode(&doc).unwrap_err().to_string();
        assert!(err.contains("cannot expand"), "{err}");
    }
}
