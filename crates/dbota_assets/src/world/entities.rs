//! A compiled entity lump (`.vents_c`): the map's entities as key/value objects and the lumps
//! it includes (the world layers a game mode switches on).

use crate::error::{Result, malformed};
use crate::kv3::Value;
use crate::resource::Resource;

pub struct EntityLump {
    /// Each entity's key values (`classname`, `origin`, `angles`, `scales`, `model`, ...).
    pub entities: Vec<Value>,
    /// The included lumps' paths (`maps/.../x.vents`, without `_c`).
    pub children: Vec<String>,
}

impl EntityLump {
    /// The lump of `resource`. Entities stored as KV3 only: the older binary key-value form
    /// (`m_keyValuesData`) is an error.
    pub fn read(resource: &Resource) -> Result<Self> {
        let data = resource
            .data()
            .ok_or_else(|| malformed("entity lump", "no DATA"))??;
        let list = |key| data.get(key).and_then(Value::as_array).unwrap_or_default();
        let mut entities = Vec::new();
        for entity in list("m_entityKeyValues") {
            match entity.get("keyValues3Data").and_then(|k| k.get("values")) {
                Some(values) => entities.push(values.clone()),
                None if entity
                    .get("m_keyValuesData")
                    .and_then(Value::as_blob)
                    .is_some_and(|b| !b.is_empty()) =>
                {
                    return Err(malformed(
                        "entity lump",
                        "binary key values are not supported",
                    ));
                }
                None => {}
            }
        }
        let children = list("m_childLumps")
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        Ok(Self { entities, children })
    }

    /// The models the entities place (`models/.../x.vmdl`, without `_c`), each once, in entity
    /// order: with the world nodes', what the world's textures come from.
    pub fn models(&self) -> Vec<&str> {
        let mut seen = std::collections::HashSet::new();
        self.entities
            .iter()
            .filter_map(|e| e.get("model")?.as_str())
            .filter(|m| !m.is_empty() && seen.insert(*m))
            .collect()
    }
}
