//! What the fuzz targets (`fuzz/`) reach besides the crate's API: the mesh buffer codecs and
//! the material conversion, which only a model's or a world's export takes otherwise. Built
//! only with `--cfg fuzzing`.

use serde_json::Value as Json;

use crate::budget::Budget;
use crate::error::Result;
use crate::material::{Images, Material, convert};
use crate::vertices::Vertices;
use crate::{meshopt, vbib, vertices};

/// `count` vertices of `stride` bytes decoded from meshopt's vertex codec.
pub fn meshopt_vertices(data: &[u8], count: usize, stride: usize) -> Result<Vec<u8>> {
    meshopt::decode_vertex_buffer(data, count, stride)
}

/// `count` indices of `size` bytes decoded from meshopt's index codec.
pub fn meshopt_indices(data: &[u8], count: usize, size: usize) -> Result<Vec<u8>> {
    meshopt::decode_index_buffer(data, count, size)
}

/// The buffers of a binary `VBIB` block as a model takes them: each vertex buffer converted
/// (skinned through `joints` with `weights` weights a vertex), each index buffer's indices.
pub fn vbib(
    block: &[u8],
    joints: Option<&[u16]>,
    weights: usize,
) -> Result<(Vec<Vertices>, Vec<Vec<u32>>)> {
    let mut budget = Budget::with_limit("fuzz", 128 << 20);
    let (vertex_buffers, index_buffers) = vbib::from_vbib(block, &mut budget)?;
    let converted = vertex_buffers
        .iter()
        .map(|b| vertices::convert(b, joints, weights, &mut budget))
        .collect::<Result<_>>()?;
    let indices = index_buffers
        .iter()
        .map(vbib::Buffer::indices)
        .collect::<Result<_>>()?;
    Ok((converted, indices))
}

/// A material drawn with `tint` as the glTF writer puts it, and the images it names.
pub fn material_json(material: &Material, tint: [f32; 4]) -> Result<(Json, usize)> {
    let mut images = Images::default();
    let uri = |vtex: &str| format!("{vtex}.png");
    let gltf = convert(material, tint, &mut images, &uri)?;
    Ok((gltf.to_json(|image| image), images.list.len()))
}
