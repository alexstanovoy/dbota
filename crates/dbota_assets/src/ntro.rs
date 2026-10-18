//! NTRO: older resources describe their `DATA` block's C structs in an introspection manifest
//! (struct and enum layouts) instead of writing KV3. This reads such a `DATA` block into the
//! same [`Value`] tree KV3 gives, with the same member names.

use std::collections::HashMap;

use crate::budget::Budget;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};
use crate::kv3::{Object, Value};

/// The struct flag of a struct whose first 4 bytes name its concrete type.
const POLYMORPHIC: u8 = 1;
/// How far up a chain of base structs fields are read; a cycle stops here.
const MAX_BASES: usize = 16;

// Indirections.
const RESOURCE_POINTER: u8 = 3;
const RESOURCE_ARRAY: u8 = 4;

/// Field types (`SchemaFieldType`).
mod ty {
    pub const STRUCT: i16 = 1;
    pub const ENUM: i16 = 2;
    pub const EXTERNAL_REFERENCE: i16 = 3;
    pub const CHAR: i16 = 4;
    pub const UCHAR: i16 = 5;
    pub const INT: i16 = 6;
    pub const UINT: i16 = 7;
    pub const FLOAT_8: i16 = 8;
    pub const DOUBLE: i16 = 9;
    pub const SBYTE: i16 = 10;
    pub const BYTE: i16 = 11;
    pub const INT16: i16 = 12;
    pub const UINT16: i16 = 13;
    pub const INT32: i16 = 14;
    pub const UINT32: i16 = 15;
    pub const INT64: i16 = 16;
    pub const UINT64: i16 = 17;
    pub const FLOAT: i16 = 18;
    pub const FLOAT64: i16 = 19;
    pub const VECTOR2D: i16 = 21;
    pub const VECTOR3D: i16 = 22;
    pub const VECTOR4D: i16 = 23;
    pub const QANGLE: i16 = 24;
    pub const QUATERNION: i16 = 25;
    pub const VMATRIX: i16 = 26;
    pub const FLTX4: i16 = 27;
    pub const COLOR: i16 = 28;
    pub const UNIQUE_ID: i16 = 29;
    pub const BOOLEAN: i16 = 30;
    pub const RESOURCE_STRING: i16 = 31;
    pub const MATRIX3X4: i16 = 33;
    pub const MATRIX3X4A: i16 = 36;
    pub const TRANSFORM: i16 = 40;
    /// A deferred resource reference, stored as an external one.
    pub const UNUSED: i16 = 41;
    pub const RADIAN_EULER: i16 = 42;
    pub const DEGREE_EULER: i16 = 43;
    pub const FOUR_VECTORS: i16 = 44;
    pub const ROTATION_VECTOR: i16 = 45;
    pub const AABB: i16 = 46;
}

struct Field {
    name: String,
    count: i16,
    offset: i16,
    indirections: Vec<u8>,
    type_data: u32,
    ty: i16,
}

struct DiskStruct {
    id: u32,
    name: String,
    size: u16,
    base: u32,
    flags: u8,
    fields: Vec<Field>,
}

/// Reads the `DATA` struct at `data_at` of `file`, laid out by the `NTRO` block at `ntro_at`;
/// `refs` names the external references by id (the `RERL` block).
pub(crate) fn read(
    file: &[u8],
    ntro_at: usize,
    data_at: usize,
    refs: &HashMap<u64, String>,
) -> Result<Value> {
    // One budget for the manifest and the tree: lists and strings may overlap, so a small file
    // can describe far more than it holds.
    let mut budget = Budget::new("NTRO");
    let structs = read_structs(file, ntro_at, &mut budget)?;
    let enums = read_enums(file, ntro_at, &mut budget)?;
    let root = structs
        .first()
        .ok_or_else(|| malformed("NTRO", "no structs"))?;
    let mut reader = Reader {
        file,
        by_id: structs.iter().map(|s| (s.id, s)).collect(),
        enums,
        refs,
        budget,
    };
    reader.structure(root, data_at, 0)
}

/// A cursor at `at` of `file`.
fn at(file: &[u8], at: usize) -> Result<Cursor<'_>> {
    let mut c = Cursor::new(file, "NTRO");
    c.take(at)?;
    Ok(c)
}

/// A string written as an offset from the offset field itself, charged to `budget`; 0 is the
/// empty string.
fn offset_string(file: &[u8], c: &mut Cursor, budget: &mut Budget) -> Result<String> {
    let field = c.pos();
    let offset = c.i32()?;
    if offset == 0 {
        return Ok(String::new());
    }
    let target = field
        .checked_add_signed(offset as isize)
        .ok_or_else(|| malformed("NTRO", "string before the file"))?;
    let text = at(file, target)?.cstr()?;
    budget.bytes(text.len())?;
    Ok(String::from_utf8_lossy(text).into_owned())
}

/// A list written as (offset from the offset field, count): a cursor at its first item. Its
/// items, `held` bytes each once read, are charged to `budget`.
fn list<'a>(
    file: &'a [u8],
    c: &mut Cursor,
    item_len: usize,
    held: usize,
    budget: &mut Budget,
) -> Result<(Cursor<'a>, usize)> {
    let field = c.pos();
    let offset = c.i32()?;
    let count = c.u32()? as usize;
    if count.saturating_mul(item_len) > file.len() {
        return Err(malformed("NTRO", format!("a list of {count}")));
    }
    budget.bytes(count.saturating_mul(held))?;
    let start = field
        .checked_add_signed(offset as isize)
        .ok_or_else(|| malformed("NTRO", "list before the file"))?;
    Ok((at(file, if count == 0 { 0 } else { start })?, count))
}

fn read_structs(file: &[u8], ntro_at: usize, budget: &mut Budget) -> Result<Vec<DiskStruct>> {
    let held = size_of::<DiskStruct>();
    let (mut c, count) = list(file, &mut at(file, ntro_at + 4)?, 40, held, budget)?;
    let mut structs = Vec::with_capacity(count);
    for _ in 0..count {
        c.u32()?;
        let id = c.u32()?;
        let name = offset_string(file, &mut c, budget)?;
        c.take(8)?;
        let size = c.u16()?;
        c.u16()?;
        let base = c.u32()?;
        let (mut f, field_count) = list(file, &mut c, 24, size_of::<Field>(), budget)?;
        let mut fields = Vec::with_capacity(field_count);
        for _ in 0..field_count {
            let field = read_field(file, &mut f, budget)?;
            // A field at a negative offset is not in the struct's bytes; dropped here, every
            // field read later yields a value the budget is charged for.
            if field.offset >= 0 {
                fields.push(field);
            }
        }
        let flags = c.u8()?;
        c.take(3)?;
        structs.push(DiskStruct {
            id,
            name,
            size,
            base,
            flags,
            fields,
        });
    }
    Ok(structs)
}

fn read_field(file: &[u8], c: &mut Cursor, budget: &mut Budget) -> Result<Field> {
    let name = offset_string(file, c, budget)?;
    let count = c.u16()? as i16;
    let offset = c.u16()? as i16;
    let (mut i, indirection_count) = list(file, c, 1, 1, budget)?;
    let indirections = i.take(indirection_count)?.to_vec();
    let type_data = c.u32()?;
    let ty = c.u16()? as i16;
    c.take(2)?;
    Ok(Field {
        name,
        count,
        offset,
        indirections,
        type_data,
        ty,
    })
}

/// Enumerator names by (enum id, value).
fn read_enums(
    file: &[u8],
    ntro_at: usize,
    budget: &mut Budget,
) -> Result<HashMap<(u32, i32), String>> {
    let (mut c, count) = list(file, &mut at(file, ntro_at + 12)?, 28, 28, budget)?;
    let mut names = HashMap::new();
    for _ in 0..count {
        c.u32()?;
        let id = c.u32()?;
        offset_string(file, &mut c, budget)?;
        c.take(8)?;
        let held = size_of::<((u32, i32), String)>();
        let (mut v, value_count) = list(file, &mut c, 8, held, budget)?;
        for _ in 0..value_count {
            let name = offset_string(file, &mut v, budget)?;
            names.entry((id, v.i32()?)).or_insert(name);
        }
    }
    Ok(names)
}

struct Reader<'a> {
    file: &'a [u8],
    by_id: HashMap<u32, &'a DiskStruct>,
    enums: HashMap<(u32, i32), String>,
    refs: &'a HashMap<u64, String>,
    budget: Budget,
}

impl<'a> Reader<'a> {
    fn node(&mut self, depth: usize) -> Result<()> {
        self.budget.node(depth)
    }

    /// A string written as an offset at `at`, charged.
    fn string_at(&mut self, at: usize) -> Result<Value> {
        let text = offset_string(self.file, &mut self::at(self.file, at)?, &mut self.budget)?;
        Ok(Value::String(text))
    }

    fn structure(&mut self, declared: &'a DiskStruct, at: usize, depth: usize) -> Result<Value> {
        self.node(depth)?;
        let mut object = Object::default();
        let mut current = declared;
        if declared.flags & POLYMORPHIC != 0 {
            let id = self::at(self.file, at)?.u32()?;
            current = self.by_id.get(&id).copied().unwrap_or(declared);
            let class = self.budget.copy(&current.name)?;
            object.push("_class".to_owned(), Value::String(class));
        }
        for _ in 0..MAX_BASES {
            for field in &current.fields {
                let value = self.field(field, at + field.offset as usize, depth)?;
                object.push(self.budget.copy(&field.name)?, value);
            }
            match self.by_id.get(&current.base) {
                Some(base) if current.base != 0 => current = base,
                _ => break,
            }
        }
        Ok(Value::Object(object))
    }

    fn field(&mut self, field: &Field, at: usize, depth: usize) -> Result<Value> {
        if field.ty == ty::CHAR && field.count > 1 && field.indirections.is_empty() {
            // An inline char array is written as one string offset; the count is its capacity.
            return self.string_at(at);
        }
        if field.count > 1 {
            return self.elements(field, 0, field.count as usize, at, depth);
        }
        self.indirection(field, 0, at, depth)
    }

    fn indirection(
        &mut self,
        field: &Field,
        level: usize,
        at: usize,
        depth: usize,
    ) -> Result<Value> {
        let Some(&kind) = field.indirections.get(level) else {
            return self.value(field, at, depth);
        };
        // Each level nests one deeper: a chain of pointers must not outgrow the stack.
        self.node(depth)?;
        let mut c = self::at(self.file, at)?;
        let offset = c.i32()?;
        let target = at.checked_add_signed(offset as isize);
        match kind {
            RESOURCE_POINTER if offset == 0 => Ok(Value::Null),
            RESOURCE_POINTER => {
                let target = target.ok_or_else(|| malformed("NTRO", "pointer before the file"))?;
                self.indirection(field, level + 1, target, depth + 1)
            }
            RESOURCE_ARRAY => {
                let count = c.u32()? as usize;
                let target = target.ok_or_else(|| malformed("NTRO", "array before the file"))?;
                self.elements(field, level + 1, count, target, depth + 1)
            }
            _ => Err(malformed("NTRO", format!("indirection {kind}"))),
        }
    }

    fn elements(
        &mut self,
        field: &Field,
        level: usize,
        count: usize,
        at: usize,
        depth: usize,
    ) -> Result<Value> {
        let size = self.size(field, level)?;
        if count.saturating_mul(size) > self.file.len().saturating_sub(at) {
            return Err(malformed("NTRO", format!("{count} elements past the end")));
        }
        if level == field.indirections.len() && matches!(field.ty, ty::BYTE | ty::UCHAR) {
            self.budget.bytes(count)?;
            return Ok(Value::Blob(self::at(self.file, at)?.take(count)?.to_vec()));
        }
        // Elements of an empty struct take no bytes, so the count bounds nothing.
        let mut items = Vec::with_capacity(count.min(4096));
        for i in 0..count {
            items.push(self.indirection(field, level, at + i * size, depth + 1)?);
        }
        Ok(Value::Array(items))
    }

    /// The bytes one element of `field` takes at indirection `level`.
    fn size(&self, field: &Field, level: usize) -> Result<usize> {
        match field.indirections.get(level) {
            Some(&RESOURCE_POINTER) => return Ok(4),
            Some(&RESOURCE_ARRAY) => return Ok(8),
            Some(&kind) => return Err(malformed("NTRO", format!("indirection {kind}"))),
            None => {}
        }
        use ty::*;
        Ok(match field.ty {
            STRUCT => self.struct_of(field)?.size as usize,
            CHAR | UCHAR | SBYTE | BYTE | BOOLEAN => 1,
            INT16 | UINT16 => 2,
            ENUM | INT | UINT | FLOAT_8 | INT32 | UINT32 | FLOAT | COLOR | RESOURCE_STRING => 4,
            EXTERNAL_REFERENCE | DOUBLE | INT64 | UINT64 | FLOAT64 | UNUSED | VECTOR2D => 8,
            VECTOR3D | QANGLE | RADIAN_EULER | DEGREE_EULER | ROTATION_VECTOR => 12,
            VECTOR4D | QUATERNION | FLTX4 | UNIQUE_ID => 16,
            AABB => 24,
            TRANSFORM => 32,
            MATRIX3X4 | MATRIX3X4A | FOUR_VECTORS => 48,
            VMATRIX => 64,
            ty => {
                return Err(malformed(
                    "NTRO",
                    format!("field type {ty} of {}", field.name),
                ));
            }
        })
    }

    fn struct_of(&self, field: &Field) -> Result<&'a DiskStruct> {
        self.by_id.get(&field.type_data).copied().ok_or_else(|| {
            malformed(
                "NTRO",
                format!("no struct {} for {}", field.type_data, field.name),
            )
        })
    }

    fn value(&mut self, field: &Field, at: usize, depth: usize) -> Result<Value> {
        use ty::*;
        self.node(depth)?;
        if field.ty == STRUCT {
            let declared = self.struct_of(field)?;
            return self.structure(declared, at, depth + 1);
        }
        let file = self.file;
        let mut c = self::at(file, at)?;
        let floats = |c: &mut Cursor, n: usize| -> Result<Value> {
            (0..n)
                .map(|_| Ok(Value::Float(f64::from(f32::from_bits(c.u32()?)))))
                .collect::<Result<_>>()
                .map(Value::Array)
        };
        Ok(match field.ty {
            ENUM => {
                let value = c.i32()?;
                match self.enums.get(&(field.type_data, value)) {
                    Some(name) => Value::String(self.budget.copy(name)?),
                    None => Value::Int(i64::from(value)),
                }
            }
            CHAR | SBYTE => Value::Int(i64::from(c.u8()? as i8)),
            UCHAR | BYTE => Value::UInt(u64::from(c.u8()?)),
            BOOLEAN => Value::Bool(c.u8()? != 0),
            INT16 => Value::Int(i64::from(c.u16()? as i16)),
            UINT16 => Value::UInt(u64::from(c.u16()?)),
            INT | INT32 => Value::Int(i64::from(c.i32()?)),
            UINT | UINT32 => Value::UInt(u64::from(c.u32()?)),
            FLOAT_8 | FLOAT => Value::Float(f64::from(f32::from_bits(c.u32()?))),
            DOUBLE | FLOAT64 => Value::Float(f64::from_bits(c.u64()?)),
            INT64 => Value::Int(c.u64()? as i64),
            UINT64 => Value::UInt(c.u64()?),
            EXTERNAL_REFERENCE | UNUSED => match self.refs.get(&c.u64()?) {
                Some(name) => Value::String(self.budget.copy(name)?),
                None => Value::Null,
            },
            VECTOR2D => floats(&mut c, 2)?,
            VECTOR3D | QANGLE | RADIAN_EULER | DEGREE_EULER | ROTATION_VECTOR => floats(&mut c, 3)?,
            VECTOR4D | QUATERNION | FLTX4 => floats(&mut c, 4)?,
            AABB => floats(&mut c, 6)?,
            TRANSFORM => floats(&mut c, 8)?,
            MATRIX3X4 | MATRIX3X4A | FOUR_VECTORS => floats(&mut c, 12)?,
            VMATRIX => floats(&mut c, 16)?,
            COLOR => Value::Array(
                c.take(4)?
                    .iter()
                    .map(|&b| Value::Int(i64::from(b)))
                    .collect(),
            ),
            UNIQUE_ID => Value::Blob(c.take(16)?.to_vec()),
            RESOURCE_STRING => self.string_at(at)?,
            ty => {
                return Err(malformed(
                    "NTRO",
                    format!("field type {ty} of {}", field.name),
                ));
            }
        })
    }
}
