//! Where the map places what the client spawns itself (trees it can cut, buildings it can
//! destroy), as the asset cache's `trees.txt` and `buildings.txt`.

use std::collections::HashSet;

use crate::kv3::Value;

const TREES: [&str; 2] = ["tree_", "dire_tree"];
const BUILDINGS: [&str; 12] = [
    "radiant_tower",
    "dire_tower",
    "radiant_ancient001",
    "dire_ancient_base001",
    "radiant_melee_barracks001",
    "radiant_ranged_barracks001",
    "dire_barracks_melee001",
    "dire_barracks_ranged001",
    "radiant_fountain002",
    "dire_fountain002",
    "radiant_statue001",
    "dire_column001",
];

/// An entity that places a model: the model's file stem, Dota position, yaw (degrees, in
/// (-180, 180]) and scale.
struct Placed {
    model: String,
    origin: [f64; 3],
    yaw: f64,
    scale: f64,
}

fn placed(entity: &Value) -> Option<Placed> {
    let model = entity.get("model")?.as_str()?;
    let file = model.rsplit('/').next()?;
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let numbers = |key, default: f64| -> [f64; 3] {
        let list = entity
            .get(key)
            .and_then(Value::as_array)
            .unwrap_or_default();
        std::array::from_fn(|i| list.get(i).and_then(Value::as_f64).unwrap_or(default))
    };
    let finite = |v: f64| if v.is_finite() { v } else { 0.0 };
    let yaw = numbers("angles", 0.0)[1].to_radians();
    Some(Placed {
        model: stem.to_owned(),
        origin: numbers("origin", 0.0).map(finite),
        yaw: finite(yaw.sin().atan2(yaw.cos()).to_degrees()),
        scale: finite(numbers("scales", 1.0)[0]),
    })
}

/// Every tree `entities` place, once per model and whole Dota unit:
/// `model dota_x dota_y height yaw_deg scale`.
pub fn trees_txt(entities: &[Value]) -> String {
    let mut out = String::from(
        "# model dota_x dota_y height yaw_deg scale (bota = dota + 9216 on x and y)\n",
    );
    let mut seen = HashSet::new();
    for p in entities.iter().filter_map(placed) {
        let key = (
            p.model.clone(),
            cell(p.origin[0], 1.0),
            cell(p.origin[1], 1.0),
        );
        if TREES.iter().any(|t| p.model.starts_with(t)) && seen.insert(key) {
            let [x, y, z] = p.origin;
            out += &format!(
                "{} {x:.1} {y:.1} {z:.1} {:.1} {:.3}\n",
                p.model, p.yaw, p.scale
            );
        }
    }
    out
}

/// Every building `entities` place, once per model and 32-unit cell:
/// `model dota_x dota_y yaw_deg`.
pub fn buildings_txt(entities: &[Value]) -> String {
    let mut out = String::from("# model dota_x dota_y yaw_deg (bota = dota + 9216 on x and y)\n");
    let mut seen = HashSet::new();
    for p in entities.iter().filter_map(placed) {
        let key = (
            p.model.clone(),
            cell(p.origin[0], 32.0),
            cell(p.origin[1], 32.0),
        );
        if BUILDINGS.iter().any(|b| p.model.starts_with(b)) && seen.insert(key) {
            let [x, y, _] = p.origin;
            out += &format!("{} {x:.1} {y:.1} {:.1}\n", p.model, p.yaw);
        }
    }
    out
}

/// `v / size` rounded half to even, as the old exporter keyed its duplicates.
fn cell(v: f64, size: f64) -> i64 {
    (v / size).round_ties_even() as i64
}
