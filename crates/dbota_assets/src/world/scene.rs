//! The whole world as Source2Viewer 20 exported it (`ExportToFile(VWorld)`): every node of the
//! world nodes' scene objects, their aggregates' fragments and the entities' models, in its
//! order; each mesh made once per name, by its first placement's tint; materials made once
//! per path and tint; images once per texture. Vertex data is not kept: the meshes record
//! where it comes from and its bounds.

use std::collections::HashMap;

use super::entities::EntityLump;
use super::place::{self, Affine, IDENTITY};
use crate::error::{Error, Result, malformed};
use crate::export::read_from;
use crate::kv3::Value;
use crate::material::{GltfMaterial, Images, Material, convert};
use crate::model::Model;
use crate::resource::Resource;
use crate::source::Source;
use crate::vertices::Vertices;
use crate::vpk::Vpk;

/// How far an overlay is lifted off what it lies on, along its normals (Source2Viewer meant
/// one centimetre but applied it to geometry already in metres).
pub(super) const OVERLAY_LIFT: f32 = 0.01 / 0.0254;
/// Most nodes, meshes and materials a world may have.
const MAX_ITEMS: usize = 1 << 20;

/// Smallest and largest corner.
pub(super) type Bounds = [[f32; 3]; 2];

pub(super) struct Scene {
    /// Model paths (`.vmdl`, without `_c`).
    pub models: Vec<String>,
    pub meshes: Vec<SceneMesh>,
    pub nodes: Vec<SceneNode>,
    pub materials: Vec<SceneMaterial>,
    pub images: Images,
}

pub(super) struct SceneNode {
    pub name: String,
    pub mesh: usize,
    /// glTF node matrix, column major.
    pub matrix: [f32; 16],
}

pub(super) struct SceneMesh {
    pub name: String,
    pub model: usize,
    /// Index into the model's meshes.
    pub mesh: usize,
    /// The vertex accessors its whole-buffer primitives share: one set per mesh made, one per
    /// aggregate for all its fragments.
    pub set: usize,
    pub primitives: Vec<ScenePrimitive>,
    /// Over its primitives' whole vertex buffers (as the exported accessors' bounds).
    pub bounds: Option<Bounds>,
}

pub(super) struct ScenePrimitive {
    /// Index into the model mesh's draw calls.
    pub draw_call: usize,
    pub material: Option<usize>,
    /// Lifted off what it lies on by [`OVERLAY_LIFT`].
    pub overlay: bool,
}

pub(super) struct SceneMaterial {
    pub gltf: GltfMaterial,
    pub shader: String,
    /// The textures it names (`.vtex`, without `_c`).
    pub textures: Vec<String>,
}

/// A mesh to make: draw calls of a model's mesh, tinted, maybe with a skin's material.
struct MeshSpec {
    model: usize,
    mesh: usize,
    name: String,
    set: usize,
    draw_calls: Vec<usize>,
    tint: [f32; 4],
    skin_material: Option<String>,
}

/// A model's meshes, kept without vertex data.
struct ModelInfo {
    meshes: Vec<MeshInfo>,
    groups: Vec<(String, Vec<String>)>,
}

struct MeshInfo {
    name: String,
    draw_calls: Vec<DrawCall>,
    /// Per vertex buffer, its bounds as it is and lifted as an overlay.
    buffers: Vec<[Option<Bounds>; 2]>,
}

struct DrawCall {
    vertices: usize,
    material: String,
    tint: Option<[f32; 4]>,
}

/// A material path and a tint's bits.
type MaterialKey = (String, [u32; 4]);

/// Reads the world and what it places.
pub(super) struct Builder<'a, S: Source + ?Sized> {
    src: &'a S,
    packages: &'a [&'a Vpk],
    texture_uri: &'a dyn Fn(&str) -> String,
    scene: Scene,
    infos: Vec<ModelInfo>,
    model_index: HashMap<String, Option<usize>>,
    mesh_index: HashMap<String, usize>,
    material_index: HashMap<MaterialKey, Option<(usize, bool)>>,
    sets: usize,
}

impl<'a, S: Source + ?Sized> Builder<'a, S> {
    pub(super) fn new(
        src: &'a S,
        packages: &'a [&'a Vpk],
        texture_uri: &'a dyn Fn(&str) -> String,
    ) -> Self {
        Self {
            src,
            packages,
            texture_uri,
            scene: Scene {
                models: Vec::new(),
                meshes: Vec::new(),
                nodes: Vec::new(),
                materials: Vec::new(),
                images: Images::default(),
            },
            infos: Vec::new(),
            model_index: HashMap::new(),
            mesh_index: HashMap::new(),
            material_index: HashMap::new(),
            sets: 0,
        }
    }

    /// The scene of the world at `path` (`maps/.../world.vwrld_c`).
    pub(super) fn build(mut self, path: &str) -> Result<Scene> {
        let world = self
            .data(path)?
            .ok_or_else(|| Error::NotFound(path.to_owned()))?;
        for node in array(world.get("m_worldNodes")) {
            let Some(prefix) = node.get("m_worldNodePrefix").and_then(Value::as_str) else {
                continue;
            };
            if let Some(node) = self.data(&format!("{prefix}.vwnod_c"))? {
                self.world_node(&node)?;
            }
        }
        for lump in array(world.get("m_entityLumps")) {
            let Some(lump) = lump.as_str() else {
                continue;
            };
            if let Some(lump) = self.data(&compiled(lump))? {
                self.entities(&lump)?;
            }
        }
        Ok(self.scene)
    }

    /// The decoded `DATA` of file `path`; `None` when no package has it.
    fn data(&self, path: &str) -> Result<Option<Value>> {
        let bytes = match read_from(self.src, self.packages, path) {
            Ok(bytes) => bytes,
            Err(Error::NotFound(_)) => return Ok(None),
            Err(e) => return Err(e),
        };
        let resource = Resource::parse(&bytes)?;
        let data = resource
            .data()
            .ok_or_else(|| malformed("world", format!("{path} has no DATA")))??;
        Ok(Some(data))
    }

    fn world_node(&mut self, node: &Value) -> Result<()> {
        for object in array(node.get("m_sceneObjects")) {
            let Some(model) = object.get("m_renderableModel").and_then(Value::as_str) else {
                continue;
            };
            let Some(model) = self.model(model)? else {
                continue;
            };
            let affine = place::affine(object.get("m_vTransform")).unwrap_or(IDENTITY);
            let mut tint = vector4(object.get("m_vTintColor"));
            if tint == [0.0; 4] {
                tint = [1.0; 4];
            }
            self.place_model(model, &affine, tint, None)?;
        }
        for object in array(node.get("m_aggregateSceneObjects")) {
            let Some(model) = object.get("m_renderableModel").and_then(Value::as_str) else {
                continue;
            };
            let Some(model) = self.model(model)? else {
                continue;
            };
            let fragments = array(object.get("m_aggregateMeshes"));
            if fragments
                .first()
                .is_some_and(|f| f.get("m_nDrawCallIndex").is_none())
            {
                self.place_model(model, &IDENTITY, [1.0; 4], None)?;
            } else {
                self.fragments(model, object, fragments)?;
            }
        }
        Ok(())
    }

    /// An aggregate's fragments: each one draw call of the model's one mesh, a mesh of its own.
    fn fragments(&mut self, model: usize, object: &Value, fragments: &[Value]) -> Result<()> {
        if self.infos[model].meshes.len() != 1 {
            return Err(malformed(
                "world",
                format!(
                    "{}: an aggregate of other than one mesh",
                    self.scene.models[model]
                ),
            ));
        }
        let name = stem(&self.scene.models[model]).to_owned();
        let transforms = array(object.get("m_fragmentTransforms"));
        let (mut next_transform, set) = (0, self.new_set());
        for (id, fragment) in fragments.iter().enumerate() {
            let mut affine = IDENTITY;
            if fragment.get("m_bHasTransform").and_then(Value::as_bool) == Some(true) {
                affine = place::affine(transforms.get(next_transform)).ok_or_else(|| {
                    malformed(
                        "world",
                        format!("{name}: fragment transform {next_transform}"),
                    )
                })?;
                next_transform += 1;
                if place::is_flat(&affine) {
                    continue;
                }
            }
            let tint = match fragment.get("m_vTintColor") {
                Some(t) => {
                    let [r, g, b, _] = vector4(Some(t));
                    [r / 255.0, g / 255.0, b / 255.0, 1.0]
                }
                None => [1.0; 4],
            };
            let draw_call = fragment
                .get("m_nDrawCallIndex")
                .and_then(Value::as_u64)
                .and_then(|i| usize::try_from(i).ok())
                .filter(|&i| i < self.infos[model].meshes[0].draw_calls.len())
                .ok_or_else(|| malformed("world", format!("{name}: fragment draw call")))?;
            let mesh = self.make_mesh(MeshSpec {
                model,
                mesh: 0,
                name: format!("{name}_fragment{}", id + 1),
                set,
                draw_calls: vec![draw_call],
                tint,
                skin_material: None,
            })?;
            self.push_node(name.clone(), mesh, &affine)?;
        }
        Ok(())
    }

    /// The entities' models (the lump's own entities: the world layers it includes are
    /// switched on by the game, and the export left them out).
    fn entities(&mut self, lump: &Value) -> Result<()> {
        for values in EntityLump::from_data(lump)?.entities {
            let text = |key| values.get(key).and_then(Value::as_str);
            let Some(model) = text("model").filter(|m| !m.is_empty()) else {
                continue;
            };
            if text("classname") == Some("csgo_player_previewmodel") {
                continue;
            }
            let Some(model) = self.model(model)? else {
                continue;
            };
            let skin = text("skin").filter(|s| !matches!(*s, "0" | "default"));
            let affine = place::entity_affine(&values);
            self.place_model(model, &affine, place::entity_tint(&values), skin)?;
        }
        Ok(())
    }

    /// The model at `path` (without `_c`), read once; `None` when no package has it.
    fn model(&mut self, path: &str) -> Result<Option<usize>> {
        if let Some(&known) = self.model_index.get(path) {
            return Ok(known);
        }
        let bytes = match read_from(self.src, self.packages, &compiled(path)) {
            Ok(bytes) => Some(bytes),
            Err(Error::NotFound(_)) => None,
            Err(e) => return Err(e),
        };
        let index = match bytes {
            Some(bytes) => {
                let model = Model::read(&Resource::parse(&bytes)?)
                    .map_err(|e| malformed("world", format!("{path}: {e}")))?;
                self.infos.push(info(&model));
                self.scene.models.push(path.to_owned());
                Some(self.infos.len() - 1)
            }
            None => None,
        };
        self.model_index.insert(path.to_owned(), index);
        Ok(index)
    }

    /// A node per mesh of `model` (`<stem>.<mesh>`, with `.<skin>` when skinned), placed by
    /// `affine`; each mesh made on its name's first placement, with `tint`.
    fn place_model(
        &mut self,
        model: usize,
        affine: &Affine,
        tint: [f32; 4],
        skin: Option<&str>,
    ) -> Result<()> {
        let skin_material = skin.and_then(|skin| {
            let groups = &self.infos[model].groups;
            let group = groups.iter().find(|(name, _)| name == skin)?;
            group.1.first().cloned()
        });
        let stem = stem(&self.scene.models[model]).to_owned();
        for m in 0..self.infos[model].meshes.len() {
            let info = &self.infos[model].meshes[m];
            if info.draw_calls.is_empty() {
                continue;
            }
            let mut name = format!("{stem}.{}", info.name);
            if let Some(skin) = skin {
                name = format!("{name}.{skin}");
            }
            let mesh = match self.mesh_index.get(&name) {
                Some(&mesh) => mesh,
                None => {
                    let draw_calls = (0..info.draw_calls.len()).collect();
                    let set = self.new_set();
                    let mesh = self.make_mesh(MeshSpec {
                        model,
                        mesh: m,
                        name: name.clone(),
                        set,
                        draw_calls,
                        tint,
                        skin_material: skin_material.clone(),
                    })?;
                    self.mesh_index.insert(name.clone(), mesh);
                    mesh
                }
            };
            self.push_node(name, mesh, affine)?;
        }
        Ok(())
    }

    fn make_mesh(&mut self, spec: MeshSpec) -> Result<usize> {
        let MeshSpec {
            model,
            mesh,
            name,
            set,
            draw_calls,
            tint,
            skin_material,
        } = spec;
        let mut primitives = Vec::with_capacity(draw_calls.len());
        let mut bounds: Option<Bounds> = None;
        for d in draw_calls {
            let call = &self.infos[model].meshes[mesh].draw_calls[d];
            let path = skin_material
                .clone()
                .unwrap_or_else(|| call.material.clone());
            let tint = match call.tint {
                Some(t) => [0, 1, 2, 3].map(|i| tint[i] * t[i]),
                None => tint,
            };
            let vertices = call.vertices;
            let (material, overlay) = match self.material(&path, tint)? {
                Some((m, overlay)) => (Some(m), overlay),
                None => (None, false),
            };
            let buffer = self.infos[model].meshes[mesh]
                .buffers
                .get(vertices)
                .ok_or_else(|| malformed("world", format!("{name}: vertex buffer {vertices}")))?;
            bounds = union(bounds, buffer[usize::from(overlay)]);
            primitives.push(ScenePrimitive {
                draw_call: d,
                material,
                overlay,
            });
        }
        check_count(self.scene.meshes.len(), "meshes")?;
        self.scene.meshes.push(SceneMesh {
            name,
            model,
            mesh,
            set,
            primitives,
            bounds,
        });
        Ok(self.scene.meshes.len() - 1)
    }

    fn push_node(&mut self, name: String, mesh: usize, affine: &Affine) -> Result<()> {
        check_count(self.scene.nodes.len(), "nodes")?;
        self.scene.nodes.push(SceneNode {
            name,
            mesh,
            matrix: place::node_matrix(affine),
        });
        Ok(())
    }

    fn new_set(&mut self) -> usize {
        self.sets += 1;
        self.sets - 1
    }

    /// The material `path` drawn with `tint`, and whether it is an overlay; made once per path
    /// and tint. `None` when no package has it.
    fn material(&mut self, path: &str, tint: [f32; 4]) -> Result<Option<(usize, bool)>> {
        let key = (path.to_owned(), tint.map(f32::to_bits));
        if let Some(&known) = self.material_index.get(&key) {
            return Ok(known);
        }
        let made = match self.data(&compiled(path))? {
            Some(data) => {
                let material = Material::from_vmat(path, &data);
                let gltf = convert(&material, tint, &mut self.scene.images, self.texture_uri)?;
                check_count(self.scene.materials.len(), "materials")?;
                self.scene.materials.push(SceneMaterial {
                    gltf,
                    textures: material.textures.iter().map(|(_, t)| t.clone()).collect(),
                    shader: material.shader.clone(),
                });
                Some((self.scene.materials.len() - 1, is_overlay(&material)))
            }
            None => None,
        };
        self.material_index.insert(key, made);
        Ok(made)
    }
}

/// What the scene needs of a model.
fn info(model: &Model) -> ModelInfo {
    let meshes = model
        .meshes
        .iter()
        .map(|mesh| MeshInfo {
            name: mesh.name.clone(),
            draw_calls: mesh
                .primitives
                .iter()
                .map(|p| DrawCall {
                    vertices: p.vertices,
                    material: p.material.clone(),
                    tint: p.tint,
                })
                .collect(),
            buffers: mesh
                .vertices
                .iter()
                .map(|v| {
                    let lifted = overlay_positions(v);
                    [bounds_of(&v.positions), bounds_of(&lifted)]
                })
                .collect(),
        })
        .collect();
    ModelInfo {
        meshes,
        groups: model.material_groups.clone(),
    }
}

/// The positions of `vertices` lifted along their normals, as an overlay's are.
pub(super) fn overlay_positions(vertices: &Vertices) -> Vec<[f32; 3]> {
    let Some(normals) = &vertices.normals else {
        return vertices.positions.clone();
    };
    vertices
        .positions
        .iter()
        .zip(normals)
        .map(|(p, n)| {
            std::array::from_fn(|i| {
                let v = p[i] + n[i] * OVERLAY_LIFT;
                if v.is_finite() { v } else { 0.0 }
            })
        })
        .collect()
}

/// Whether the material is drawn over other geometry (decals, path borders).
fn is_overlay(material: &Material) -> bool {
    material.int("F_OVERLAY") == 1
        || material.int("F_DEPTHBIAS") == 1
        || material.int("F_DEPTH_BIAS") == 1
        || material.shader.ends_with("static_overlay.vfx")
        || material.shader == "citadel_overlay.vfx"
}

pub(super) fn bounds_of(points: &[[f32; 3]]) -> Option<Bounds> {
    let mut out: Option<Bounds> = None;
    for p in points {
        let b = out.get_or_insert([*p, *p]);
        for i in 0..3 {
            b[0][i] = b[0][i].min(p[i]);
            b[1][i] = b[1][i].max(p[i]);
        }
    }
    out
}

fn union(a: Option<Bounds>, b: Option<Bounds>) -> Option<Bounds> {
    match (a, b) {
        (Some(a), Some(b)) => Some([
            std::array::from_fn(|i| a[0][i].min(b[0][i])),
            std::array::from_fn(|i| a[1][i].max(b[1][i])),
        ]),
        (a, b) => a.or(b),
    }
}

fn check_count(count: usize, what: &str) -> Result<()> {
    if count >= MAX_ITEMS {
        return Err(malformed("world", format!("more than {MAX_ITEMS} {what}")));
    }
    Ok(())
}

pub(super) fn array(value: Option<&Value>) -> &[Value] {
    value.and_then(Value::as_array).unwrap_or_default()
}

fn vector4(value: Option<&Value>) -> [f32; 4] {
    let items = array(value);
    std::array::from_fn(|i| items.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32)
}

/// The compiled file of resource `path` (`x.vmdl` is `x.vmdl_c`).
fn compiled(path: &str) -> String {
    if path.ends_with("_c") {
        path.to_owned()
    } else {
        format!("{path}_c")
    }
}

/// A path's file name without its extension.
fn stem(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.rsplit_once('.').map_or(file, |(stem, _)| stem)
}
