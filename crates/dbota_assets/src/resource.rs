use crate::budget::Budget;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::kv3::{self, Value};
use crate::ntro;

/// The only header version of compiled Source 2 resources (`*_c` files).
const HEADER_VERSION: u16 = 12;
const BLOCK_ENTRY_LEN: usize = 12;

/// A compiled Source 2 resource (`.vmdl_c`, `.vmat_c`, `.vtex_c`, ...): a header and a table of
/// typed blocks (`RERL`, `REDI`/`RED2`, `NTRO`, `DATA`, `CTRL`, `MBUF`, `VBIB`, `ANIM`, `AGRP`,
/// `ASEQ`, `MRPH`, `PHYS`, ...). Borrows the file's bytes.
pub struct Resource<'a> {
    bytes: &'a [u8],
    file_size: u32,
    version: u16,
    blocks: Vec<Block>,
}

/// A block of a [`Resource`]: its four-character type and where its bytes are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: [u8; 4],
    pub offset: u32,
    pub size: u32,
}

/// A file named in a resource's `RERL` block (its external references).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalRef {
    pub id: u64,
    pub name: String,
}

impl<'a> Resource<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut c = Cursor::new(bytes, "resource header");
        let file_size = c.u32()?;
        let header_version = c.u16()?;
        if header_version != HEADER_VERSION {
            return Err(malformed(
                "resource header",
                format!("header version {header_version}, not {HEADER_VERSION}"),
            ));
        }
        let version = c.u16()?;
        let table_at = c.pos() + c.u32()? as usize;
        let count = c.u32()? as usize;
        if count > bytes.len() / BLOCK_ENTRY_LEN {
            return Err(malformed("resource header", format!("{count} blocks")));
        }
        let mut c = Cursor::new(bytes, "resource block table");
        c.take(table_at)?;
        let mut blocks = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = c.array()?;
            // Offsets count from the offset field itself.
            let at = c.pos() as u64;
            let offset = at + u64::from(c.u32()?);
            let size = c.u32()?;
            if offset + u64::from(size) > bytes.len() as u64 {
                return Err(malformed(
                    "resource block table",
                    format!(
                        "block {} at {offset}+{size} past the end ({})",
                        kind_name(&kind),
                        bytes.len()
                    ),
                ));
            }
            blocks.push(Block {
                kind,
                offset: offset as u32,
                size,
            });
        }
        Ok(Self {
            bytes,
            file_size,
            version,
            blocks,
        })
    }

    /// The resource type's own version (the header's second field).
    pub fn version(&self) -> u16 {
        self.version
    }

    /// The size the header gives; textures and sounds keep streamed data after it.
    pub fn file_size(&self) -> u32 {
        self.file_size
    }

    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    pub fn bytes(&self, block: &Block) -> &'a [u8] {
        let start = block.offset as usize;
        &self.bytes[start..start + block.size as usize]
    }

    /// The bytes of the first block of `kind`.
    pub fn block(&self, kind: &[u8; 4]) -> Option<&'a [u8]> {
        self.blocks
            .iter()
            .find(|b| &b.kind == kind)
            .map(|b| self.bytes(b))
    }

    /// The bytes after the header's file size (texture mips, sound samples).
    pub fn trailing(&self) -> &'a [u8] {
        self.bytes
            .get(self.file_size as usize..)
            .unwrap_or_default()
    }

    /// The first block of `kind` decoded as KV3, or `None` when it is absent or not KV3 (an
    /// old resource describes its `DATA` with `NTRO` instead).
    pub fn kv3(&self, kind: &[u8; 4]) -> Option<Result<kv3::Document>> {
        let bytes = self.block(kind)?;
        kv3::is_kv3(bytes).then(|| kv3::decode(bytes))
    }

    /// The `DATA` block as a tree: KV3, or the struct an `NTRO` block describes in older
    /// resources. `None` when `DATA` is plain binary (textures, sounds) or absent.
    pub fn data(&self) -> Option<Result<Value>> {
        let data = self.blocks.iter().find(|b| &b.kind == b"DATA")?;
        let bytes = self.bytes(data);
        if kv3::is_kv3(bytes) {
            return Some(kv3::decode(bytes).map(|doc| doc.root));
        }
        let ntro = self.blocks.iter().find(|b| &b.kind == b"NTRO")?;
        Some(self.external_refs().and_then(|refs| {
            let names = refs.into_iter().map(|r| (r.id, r.name)).collect();
            ntro::read(
                self.bytes,
                ntro.offset as usize,
                data.offset as usize,
                &names,
            )
        }))
    }

    /// The files this resource references (`RERL`), empty when it has none.
    pub fn external_refs(&self) -> Result<Vec<ExternalRef>> {
        let Some(block) = self.blocks.iter().find(|b| &b.kind == b"RERL") else {
            return Ok(Vec::new());
        };
        // Offsets count from their own field and may point anywhere in the file, so names may
        // overlap: their copies are charged, or a small file would name gigabytes.
        let mut budget = Budget::new("RERL");
        let mut c = Cursor::new(self.bytes, "RERL");
        c.take(block.offset as usize)?;
        let list_at = c.pos() + c.u32()? as usize;
        let count = c.u32()? as usize;
        if count > block.size as usize / 16 {
            return Err(malformed("RERL", format!("{count} references")));
        }
        let mut c = Cursor::new(self.bytes, "RERL");
        c.take(list_at)?;
        let mut refs = Vec::with_capacity(count);
        for _ in 0..count {
            let id = c.u64()?;
            let name_at = c.pos() + c.u32()? as usize;
            c.take(4)?;
            let mut name = Cursor::new(self.bytes, "RERL name");
            name.take(name_at)?;
            let name = name.cstr()?;
            budget.bytes(name.len())?;
            let name = String::from_utf8_lossy(name).into_owned();
            refs.push(ExternalRef { id, name });
        }
        Ok(refs)
    }
}

pub(crate) fn kind_name(kind: &[u8; 4]) -> String {
    String::from_utf8_lossy(kind).into_owned()
}
