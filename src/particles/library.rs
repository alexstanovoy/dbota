//! The particle effects of the cache: each file a root system and its children, read back into
//! heatel's definitions and checked, their textures loaded once and shared as looks.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::math::Vec3;
use bevy::prelude::*;
use heatel::particle::{Blend, System};
use serde_json::Value;

use super::store::{self, Defaults};

/// The layout of the particle files this client reads (dbota_import's `PARTICLE_VERSION`).
const VERSION: u64 = 1;
/// Most systems in one effect, as the importer bounds them.
const MAX_SYSTEMS: usize = 256;
/// Most looks (texture and blend) of all effects together.
pub const MAX_LOOKS: usize = 256;
/// Most sequences of a sprite sheet the client keeps, and frames of one.
const MAX_SEQUENCES: usize = 1024;
const MAX_FRAMES: usize = 4096;

/// A texture and how it is blended: one shared GPU pipeline and bind group each.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Look {
    pub image: Handle<Image>,
    /// Multiplies what is behind (`MOD2X`) rather than blending over it.
    pub modulate: bool,
    pub depth_test: bool,
}

/// One frame of a sprite sheet: where it starts in its sequence (0..1), its texture
/// coordinates cropped to its content and of its whole cell.
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub start: f32,
    pub cropped: [f32; 4],
    pub uncropped: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct Sequence {
    pub clamp: bool,
    /// What its frames' display times add up to.
    pub total: f32,
    pub frames: Vec<Frame>,
}

/// A renderer of a system: its definition's index, its look and its texture's sheet.
pub struct Draw {
    pub renderer: usize,
    pub look: u16,
    pub sheet: Option<Arc<Vec<Sequence>>>,
}

/// A system as the simulation runs it.
pub struct Def {
    pub system: System,
    /// Its children's indices in the effect, in the definition's order (a child the file lacks
    /// is left out).
    pub children: Vec<(usize, bool, f32)>,
    pub draws: Vec<Draw>,
    pub defaults: Defaults,
}

/// An effect: its root system first, then every system under it.
pub struct Effect {
    pub defs: Vec<Def>,
}

impl Effect {
    /// The effect of `systems` (path and definition, the root first), drawing nothing yet.
    pub fn new(systems: &[(String, System)]) -> Self {
        let defs = systems
            .iter()
            .map(|(_, system)| Def {
                children: children(system, systems),
                draws: Vec::new(),
                defaults: defaults(system),
                system: system.clone(),
            })
            .collect();
        Self { defs }
    }
}

/// Every effect of the cache, by logical name, and the looks they draw with.
#[derive(Resource, Default)]
pub struct Library {
    pub effects: HashMap<String, Arc<Effect>>,
    pub looks: Vec<Look>,
}

impl Library {
    pub fn effect(&self, name: &str) -> Option<&Arc<Effect>> {
        self.effects.get(name)
    }

    /// Reads the effect `name` of file `bytes`; a file that is not one is an error.
    pub fn add(
        &mut self,
        name: &str,
        bytes: &[u8],
        server: &AssetServer,
    ) -> Result<Vec<String>, String> {
        let root: Value = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
        if root.get("version").and_then(Value::as_u64) != Some(VERSION) {
            return Err(format!("not a version {VERSION} particle file"));
        }
        let systems = root
            .get("systems")
            .and_then(Value::as_array)
            .ok_or("no systems")?;
        if systems.is_empty() || systems.len() > MAX_SYSTEMS {
            return Err(format!("{} systems", systems.len()));
        }
        let textures = self.textures(&root, server)?;
        let mut parsed = Vec::with_capacity(systems.len());
        for entry in systems {
            let path = entry
                .get("path")
                .and_then(Value::as_str)
                .ok_or("a system without a path")?;
            let system: System =
                serde_json::from_value(entry.get("system").cloned().unwrap_or_default())
                    .map_err(|e| format!("{path}: {e}"))?;
            system.validate().map_err(|e| format!("{path}: {e}"))?;
            parsed.push((path.to_owned(), system));
        }
        let mut effect = Effect::new(&parsed);
        let mut unsupported = Vec::new();
        for def in &mut effect.defs {
            unsupported.extend(
                def.system
                    .unsupported_classes()
                    .into_iter()
                    .map(str::to_owned),
            );
            def.draws = self.draws(&def.system, &textures);
        }
        self.effects.insert(name.to_owned(), Arc::new(effect));
        Ok(unsupported)
    }

    /// The looks of the file's textures (by `.vtex` path, with their blends added as needed
    /// later) and their sheets.
    fn textures(
        &mut self,
        root: &Value,
        server: &AssetServer,
    ) -> Result<HashMap<String, Texture>, String> {
        let mut out = HashMap::new();
        for texture in root
            .get("textures")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let vtex = texture
                .get("vtex")
                .and_then(Value::as_str)
                .ok_or("a texture without a name")?;
            let file = texture
                .get("file")
                .and_then(Value::as_str)
                .ok_or("a texture without a file")?;
            if !dbota_safe_path(file) {
                return Err(format!("texture file {file:?}"));
            }
            let sheet = texture
                .get("sheet")
                .and_then(Value::as_array)
                .map(|s| sheet(s))
                .transpose()?;
            out.insert(
                vtex.to_owned(),
                Texture {
                    image: server.load(crate::manifest::cache_path(file)),
                    sheet: sheet.map(Arc::new),
                },
            );
        }
        Ok(out)
    }

    /// The renderers of `system` the client draws, each with its look; one whose texture the
    /// cache lacks draws nothing.
    fn draws(&mut self, system: &System, textures: &HashMap<String, Texture>) -> Vec<Draw> {
        let mut out = Vec::new();
        for (index, renderer) in system.renderers.iter().enumerate() {
            let Some(look) = renderer.class.look() else {
                continue;
            };
            let Some(texture) = look.texture.as_ref().and_then(|t| textures.get(t)) else {
                continue;
            };
            let key = Look {
                image: texture.image.clone(),
                modulate: modulates(look.blend),
                depth_test: !look.no_depth_test,
            };
            if let Some(look) = self.look(key) {
                out.push(Draw {
                    renderer: index,
                    look,
                    sheet: texture.sheet.clone(),
                });
            }
        }
        out
    }

    /// The index of a look, added when new.
    pub fn look(&mut self, look: Look) -> Option<u16> {
        if let Some(at) = self.looks.iter().position(|l| *l == look) {
            return Some(at as u16);
        }
        if self.looks.len() == MAX_LOOKS {
            return None;
        }
        self.looks.push(look);
        Some((self.looks.len() - 1) as u16)
    }
}

struct Texture {
    image: Handle<Image>,
    sheet: Option<Arc<Vec<Sequence>>>,
}

/// A cache path: relative, plain, no `..`.
fn dbota_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 200
        && path.split('/').all(|part| {
            !part.is_empty()
                && part != ".."
                && part != "."
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
        })
}

fn sheet(sequences: &[Value]) -> Result<Vec<Sequence>, String> {
    if sequences.len() > MAX_SEQUENCES {
        return Err(format!("{} sheet sequences", sequences.len()));
    }
    let mut out = Vec::with_capacity(sequences.len());
    for sequence in sequences {
        let frames = sequence
            .get("frames")
            .and_then(Value::as_array)
            .ok_or("a sequence without frames")?;
        if frames.len() > MAX_FRAMES {
            return Err(format!("{} frames", frames.len()));
        }
        let mut list = Vec::with_capacity(frames.len());
        let mut start = 0.0;
        for frame in frames {
            let values: Vec<f32> = frame
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_f64().map(|f| f as f32).filter(|f| f.is_finite()))
                .collect();
            let [time, c0, c1, c2, c3, u0, u1, u2, u3] = values[..] else {
                return Err("a frame is not nine numbers".to_owned());
            };
            list.push(Frame {
                start,
                cropped: [c0, c1, c2, c3],
                uncropped: [u0, u1, u2, u3],
            });
            start += time.max(0.0);
        }
        // The display times are shares of the sequence; a sheet whose times do not add up to 1
        // is spread over what they add up to.
        if start > 0.0 {
            list.iter_mut().for_each(|f| f.start /= start);
        }
        out.push(Sequence {
            clamp: sequence
                .get("clamp")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            total: start,
            frames: list,
        });
    }
    Ok(out)
}

fn children(system: &System, parsed: &[(String, System)]) -> Vec<(usize, bool, f32)> {
    let mut seen = HashSet::new();
    system
        .children
        .iter()
        .filter_map(|c| {
            let at = parsed.iter().position(|(path, _)| *path == c.path)?;
            // The root is its own ancestor: a cycle is left out, as Source leaves it.
            (at != 0 && seen.insert((at, c.end_cap))).then_some((at, c.end_cap, c.delay))
        })
        .collect()
}

/// A system's constants as a new particle's attributes.
fn defaults(system: &System) -> Defaults {
    let c = &system.constants;
    let mut defaults = Defaults {
        scalars: [0.0; 16],
        vectors: [Vec3::ZERO; 6],
    };
    defaults.scalars[store::LIFETIME] = c.lifetime;
    defaults.scalars[store::RADIUS] = c.radius;
    defaults.scalars[store::ROLL] = c.rotation;
    defaults.scalars[store::ROLL_SPEED] = c.rotation_speed;
    defaults.scalars[store::ALPHA] = c.color[3];
    defaults.scalars[store::SEQUENCE] = c.sequence;
    defaults.scalars[store::TRAIL_LENGTH] = 0.1;
    defaults.scalars[10] = 1.0;
    defaults.scalars[store::FORCE_SCALE] = 1.0;
    defaults.vectors[store::COLOR] = Vec3::new(c.color[0], c.color[1], c.color[2]);
    defaults.vectors[store::NORMAL] = Vec3::from(c.normal);
    defaults
}

/// Whether a blend multiplies what is behind.
fn modulates(blend: Blend) -> bool {
    blend == Blend::Mod2x
}
