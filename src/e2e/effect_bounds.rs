//! How far an effect's particles and cards may go, from its own data and the units its operators
//! are implemented in: each system's reach from its control point 0 over a particle's life, from
//! where its initializers put a particle (`spawn`), how fast they and its operators move it
//! (`speed`: an additive position noise or a position offset oscillation moves a particle by its
//! value times the step, a drift of its value a second) and how hard its operators and forces
//! accelerate it (`push`): `spawn + speed * life + push * life² / 2`, or at most its velocity
//! cap times its life where a `MaxVelocity` caps it; velocities and forces move nothing in a
//! system without a movement operator. A card drawn of a particle reaches past it
//! by its renderer's extent (a trail's longest length, a sprite's corner).
//!
//! A system placed about a control point the line does not set (Dota's data leaves it at the
//! world's origin) has no reach, and neither has one made from its particles. One that only
//! reads such a point's value (an emission rate, a velocity) reads zero.
//!
//! Shadow Fiend's worn effects, played with every control point their systems read set about
//! control point 0, as the portrait's stage steps them (on the frames it is drawn, by the time
//! since) and as the world steps them, over frames of uneven length, frame hitches (a 1 ms frame,
//! then a 200 ms one) and frames of two lengths taking turns, keep every particle within its
//! system's reach. The portrait test holds the stage's particles and cards to these reaches too.

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use serde_json::Value;

use crate::particles::{Effect, EffectSight, Playing};

/// How far apart a worn effect's control points may be: a hero's height (its hitloc, eyes and
/// head are within it).
pub(crate) const CP_SPREAD: f32 = 300.0;

/// Where the line puts the control points its effect reads.
#[derive(Clone, Copy)]
pub(crate) struct Points<'a> {
    /// The control points it sets.
    pub fed: &'a [u64],
    /// The longest a set control point's position is, read as a value (a velocity, a rate).
    pub value: f32,
}

impl Points<'_> {
    /// The longest control point `cp`'s position is as a value: an unset one's is zero.
    fn value_of(self, cp: u64) -> f32 {
        if self.fed.contains(&cp) {
            self.value
        } else {
            0.0
        }
    }
}

/// A vector range's farther end, component by component, as a length: the farthest corner.
fn farther(min: &Value, max: &Value) -> Result<f32, String> {
    let (lo, hi) = (as_vec(min)?, as_vec(max)?);
    Ok(Vec3::from_array(std::array::from_fn(|i| lo[i].abs().max(hi[i].abs()))).length())
}

fn as_vec(v: &Value) -> Result<[f32; 3], String> {
    let v = v.get("Literal").unwrap_or(v);
    let items = v.as_array().ok_or_else(|| format!("no vector: {v}"))?;
    Ok(std::array::from_fn(|i| {
        items.get(i).and_then(Value::as_f64).unwrap_or(0.0) as f32
    }))
}

fn number(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0).abs() as f32
}

/// The largest magnitude an input can take: a literal (a number, or a vector's length), a random
/// range's farthest corner, or a control point's component or value (as `points` sets it).
fn most(input: &Value, points: Points<'_>) -> Result<f32, String> {
    let of = |v: &Value| -> Result<f32, String> {
        match v {
            Value::Number(_) => Ok(number(v)),
            Value::Array(_) => Ok(Vec3::from_array(as_vec(v)?).length()),
            other => Err(format!("an input of no known bound: {other}")),
        }
    };
    let Value::Object(o) = input else {
        return of(input);
    };
    if let Some(literal) = o.get("Literal") {
        return of(literal);
    }
    if let Some(random) = o.get("Random") {
        return match (&random["min"], &random["max"]) {
            (min @ Value::Array(_), max @ Value::Array(_)) => farther(min, max),
            (min, max) => Ok(of(min)?.max(of(max)?)),
        };
    }
    if let Some(c) = o.get("ControlPointComponent") {
        let value = points.value_of(c["control_point"].as_u64().unwrap_or(0));
        let mapping = &c["mapping"];
        return match mapping {
            Value::String(direct) if direct == "Direct" => Ok(value),
            Value::Object(m) if m.contains_key("Mult") => Ok(number(&m["Mult"]) * value),
            Value::Object(m) if m.contains_key("Remap") => {
                let out = m["Remap"]["output"]
                    .as_array()
                    .ok_or("a remap of no output")?;
                Ok(out.iter().map(number).fold(0.0, f32::max))
            }
            other => Err(format!(
                "a control point mapping of no known bound: {other}"
            )),
        };
    }
    if let Some(c) = o.get("ControlPointValue") {
        let value = points.value_of(c["control_point"].as_u64().unwrap_or(0));
        return Ok(value * Vec3::from_array(as_vec(&c["scale"])?).abs().max_element());
    }
    Err(format!("an input of no known bound: {input}"))
}

/// The class name and fields of an item of a system's list.
fn class(item: &Value) -> (&str, &Value) {
    match &item["class"] {
        Value::String(name) => (name.as_str(), &Value::Null),
        Value::Object(o) => o
            .iter()
            .next()
            .map_or(("", &Value::Null), |(k, v)| (k.as_str(), v)),
        _ => ("", &Value::Null),
    }
}

/// What a system does with a control point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    /// Places particles or forces about it.
    Place,
    /// Reads its position as a number or a vector (a rate, a velocity).
    Value,
    /// Sets it (a pre-emission operator), about another it places.
    Set,
}

/// Every control point a system uses, and how.
fn uses(system: &Value) -> Vec<(u64, Use)> {
    fn walk(v: &Value, parent: &str, out: &mut Vec<(u64, Use)>) {
        match v {
            Value::Object(o) => {
                for (key, child) in o {
                    if key.ends_with("control_point")
                        && let Some(cp) = child.as_u64()
                    {
                        let how = match (key.as_str(), parent) {
                            ("control_point", "ControlPointComponent" | "ControlPointValue") => {
                                Use::Value
                            }
                            ("control_point", "SetSingleControlPointPosition") => Use::Set,
                            _ => Use::Place,
                        };
                        out.push((cp, how));
                    }
                    walk(child, key, out);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| walk(item, parent, out)),
            _ => {}
        }
    }
    let mut out = Vec::new();
    walk(system, "", &mut out);
    out
}

/// How far from its control point 0 a system's control points lie: those the line sets within
/// [`CP_SPREAD`], those it sets itself as far again as the offset it sets them at.
fn spread(system: &Value, points: Points<'_>) -> Result<f32, String> {
    let mut set = 0.0f32;
    for item in system["pre_emission"].as_array().into_iter().flatten() {
        if let ("SetSingleControlPointPosition", f) = class(item) {
            set = set.max(most(&f["position"], points)?);
        }
    }
    Ok(CP_SPREAD + set)
}

/// The radius a system's particles reach: its constant and what its initializers and operators
/// set or scale it by.
fn radius(system: &Value, points: Points<'_>) -> Result<f32, String> {
    let mut radius = number(&system["constants"]["radius"]);
    let mut scale = 1.0f32;
    for item in system["initializers"].as_array().into_iter().flatten() {
        match class(item) {
            ("InitFloat", f) if f["field"] == 3 => {
                let value = most(&f["value"], points)?;
                match f["set_method"].as_str() {
                    Some("Replace") => radius = radius.max(value),
                    Some("Add") => radius += value,
                    _ => scale *= value.max(1.0),
                }
            }
            ("RemapParticleCountToScalar", f) if f["field"] == 3 => {
                let out = f["output"].as_array().ok_or("a remap of no output")?;
                let value = out.iter().map(number).fold(0.0, f32::max);
                match f["set_method"].as_str() {
                    Some("Replace") => radius = radius.max(value),
                    _ => scale *= value.max(1.0),
                }
            }
            (other, f) if f["field"] == 3 => {
                return Err(format!("no bound for the radius {other} sets"));
            }
            _ => {}
        }
    }
    for item in system["operators"].as_array().into_iter().flatten() {
        match class(item) {
            ("InterpolateRadius", f) => {
                scale *= most(&f["start_scale"], points)?
                    .max(most(&f["end_scale"], points)?)
                    .max(1.0);
            }
            (other, f) if f["field"] == 3 || f["output_field"] == 3 => {
                return Err(format!("no bound for the radius {other} sets"));
            }
            _ => {}
        }
    }
    Ok(radius * scale)
}

/// How far a card a system's renderer `renderer` draws reaches past its particle: a trail its
/// longest length and its width, a sprite its corner, a rope its width; a model draws no card.
pub(crate) fn card_extent(
    system: &Value,
    renderer: usize,
    points: Points<'_>,
) -> Result<f32, String> {
    let item = &system["renderers"][renderer];
    let (kind, f) = class(item);
    let radius = radius(system, points)? * most(&f["look"]["radius_scale"], points)?;
    match kind {
        "Trails" => Ok(number(&f["max_length"]) + radius),
        "Sprites" => Ok(radius * std::f32::consts::SQRT_2),
        "Ropes" => Ok(radius),
        "Models" => Ok(0.0),
        other => Err(format!("no extent for the renderer {other}")),
    }
}

/// A static outer shell's farthest point from CP0. A zero inner radius avoids the solver's
/// four-lane centre reseed; other modes need their own bound before this model accepts them.
fn distance_shell(item: &Value, system: &Value, points: Points<'_>) -> Result<Option<f32>, String> {
    use heatel::particle::{Constraint, EndCap, FloatInput, Function, Transform};

    let function: Function<Constraint> =
        serde_json::from_value(item.clone()).map_err(|why| why.to_string())?;
    let Constraint::Distance(distance) = function.class else {
        return Err("not a distance shell".to_owned());
    };
    let FloatInput::Literal(strength) = function.timing.strength else {
        return Err("a distance shell of varying strength".to_owned());
    };
    if strength <= 0.0 {
        return Ok(None);
    }
    let timing = &function.timing;
    if timing.end_cap != EndCap::Always
        || timing.fade_in != [0.0; 2]
        || timing.fade_out != [0.0; 2]
        || timing.oscillate != 0.0
        || timing.run_once
    {
        return Err("a distance shell that is not always active".to_owned());
    }
    let (FloatInput::Literal(minimum), FloatInput::Literal(maximum)) =
        (distance.min_distance, distance.max_distance)
    else {
        return Err("a distance shell of varying radii".to_owned());
    };
    if minimum != 0.0 {
        return Err("a nonzero distance-shell minimum needs a reseed bound".to_owned());
    }
    if maximum < 0.0 {
        return Ok(None);
    }
    let origin = match (distance.global_center, distance.transform) {
        (true, _) | (_, Transform::Identity) => points.value,
        (
            _,
            Transform::ControlPoint {
                control_point,
                range_max,
                ..
            },
        ) if control_point.index() == 0 && range_max.is_none_or(|last| last.index() == 0) => 0.0,
        _ => spread(system, points)?,
    };
    let (_, fields) = class(item);
    let bound = origin + most(&fields["center_offset"], points)? + maximum;
    if !bound.is_finite() {
        return Err("a distance shell of no finite reach".to_owned());
    }
    Ok(Some(bound))
}

/// A system's reach from its control point 0, given where the line puts its control points and
/// `parent` (its parent's reach, for one made from its particles).
fn reach(system: &Value, points: Points<'_>, parent: Option<f32>) -> Result<f32, String> {
    let mut life = number(&system["constants"]["lifetime"]);
    let (mut spawn, mut speed, mut push, mut drift) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
    for item in system["initializers"].as_array().into_iter().flatten() {
        match class(item) {
            ("InitFloat", f) if f["field"] == 1 => life = life.max(most(&f["value"], points)?),
            ("CreateWithinSphere", f) => {
                spawn += most(&f["radius_max"], points)?;
                speed += most(&f["speed_max"], points)? + most(&f["local_speed_max"], points)?;
            }
            ("RingWave", f) => {
                spawn += most(&f["radius"], points)? + most(&f["thickness"], points)?;
                speed += most(&f["speed_max"], points)?;
            }
            ("PositionOffset", f) => spawn += farther(&f["min"], &f["max"])?,
            ("InitialVelocityNoise", f) => speed += farther(&f["min"], &f["max"])?,
            ("VelocityFromCp", f) => speed += number(&f["scale"]) * most(&f["velocity"], points)?,
            // A place in the body-sized capsule the client stands in for the model's hitboxes.
            ("CreateOnModel", f) => {
                let (radius, height) = crate::particles::BODY;
                spawn += Vec2::new(radius, height).length() * most(&f["scale"], points)?;
                if number(&f["bone_velocity"]) > 0.0 {
                    speed += number(&f["max_bone_velocity"]);
                }
            }
            ("PositionPlaceOnGround", f) => {
                spawn += most(&f["max_trace_length"], points)? + most(&f["offset"], points)?;
            }
            ("CreateFromParentParticles", _) => {
                spawn += parent.ok_or("made from no parent's particles")?;
            }
            (
                "InitFloat"
                | "RandomColor"
                | "RandomSequence"
                | "RandomYawFlip"
                | "RemapInitialDirectionToTransformToVector"
                | "RemapParticleCountToScalar",
                _,
            ) => {}
            (other, _) => return Err(format!("no bound for the initializer {other}")),
        }
    }
    let (mut cap, mut uncapped, mut moving) = (None::<f32>, false, false);
    for item in system["operators"].as_array().into_iter().flatten() {
        match class(item) {
            ("BasicMovement", f) => {
                moving = true;
                push += most(&f["gravity"], points)?;
            }
            // Its value (within min to max) times the step, added to the place and the place
            // before alike: a drift of at most its farthest corner a second. Its scale is the
            // noise's spatial frequency, not its size.
            ("VectorNoise", f) if f["field"] == 0 && f["additive"] == true => {
                drift += farther(&f["min"], &f["max"])?;
            }
            // Its rate times its rate scale times the step, each step: a drift where it offsets
            // the place before too, else a change of velocity every step only a cap bounds.
            ("OscillateVector", f) if f["field"] == 0 => {
                let rate =
                    farther(&f["rate_min"], &f["rate_max"])? * most(&f["rate_scale"], points)?;
                if f["offset_position"] == true {
                    drift += rate;
                } else {
                    uncapped = true;
                }
            }
            ("MaxVelocity", f) => cap = Some(most(&f["max"], points)?),
            ("SetToControlPoint", _) => spawn += spread(system, points)?,
            (
                "PositionLock"
                | "MovementRotateParticleAroundAxis"
                | "Decay"
                | "FadeInSimple"
                | "FadeOutSimple"
                | "InterpolateRadius"
                | "SpinUpdate"
                | "ColorInterpolate"
                | "RampScalarLinear"
                | "RampScalarLinearSimple"
                | "RampScalarSpline"
                | "LerpEndCapScalar"
                | "DistanceToTransform"
                | "VectorNoise"
                | "OscillateVector"
                | "Unsupported",
                _,
            ) => {}
            (other, _) => return Err(format!("no bound for the operator {other}")),
        }
    }
    for item in system["forces"].as_array().into_iter().flatten() {
        match class(item) {
            ("AttractToControlPoint", f) => push += most(&f["amount"], points)?,
            ("TwistAroundAxis", f) => push += most(&f["amount"], points)?,
            (other, _) => return Err(format!("no bound for the force {other}")),
        }
    }
    let mut shell_cap = None::<f32>;
    for item in system["constraints"].as_array().into_iter().flatten() {
        match class(item) {
            ("Distance", _) if moving => {
                if let Some(bound) = distance_shell(item, system, points)? {
                    shell_cap = Some(shell_cap.map_or(bound, |previous| previous.max(bound)));
                }
            }
            ("Distance", _) => {}
            ("Unsupported", _) => {}
            (other, _) => return Err(format!("no bound for the constraint {other}")),
        }
    }
    // Without a movement no velocity or force moves a particle: its velocity only sets the place
    // it was before (what a trail draws from).
    let moved = speed * life + push * life * life / 2.0;
    // An oscillating velocity is not in `moved`: only the cap's travel bounds it.
    let moved = match (moving, cap, uncapped) {
        (false, ..) => 0.0,
        (true, Some(cap), true) => cap * life,
        (true, Some(cap), false) => moved.min(cap * life),
        (true, None, false) => moved,
        (true, None, true) => return Err("an oscillating velocity no cap bounds".to_owned()),
    };
    // Keep the original free-motion allowance, plus any placement the shell can impose.
    // Position drift may run after BasicMovement's constraints, so it remains outside the cap.
    let placed = spawn + moved;
    Ok(shell_cap.map_or(placed, |shell| placed.max(shell)) + drift * life)
}

/// The reach of each system of a particle file (in its order, its definitions'), none for one
/// placed about a control point neither its line nor itself sets, or made from such a one's
/// particles.
pub(crate) fn reaches(file: &Value, points: Points<'_>) -> Result<Vec<Option<f32>>, String> {
    let systems = file["systems"].as_array().ok_or("no systems")?;
    let index: HashMap<&str, usize> = systems
        .iter()
        .enumerate()
        .filter_map(|(i, s)| Some((s["path"].as_str()?, i)))
        .collect();
    let mut parent = vec![None; systems.len()];
    for (i, s) in systems.iter().enumerate() {
        for child in s["system"]["children"].as_array().into_iter().flatten() {
            if let Some(&c) = child["path"].as_str().and_then(|p| index.get(p)) {
                parent[c] = Some(i);
            }
        }
    }
    let mut out: Vec<Option<f32>> = vec![None; systems.len()];
    // Parents come before their children in the file.
    for (i, s) in systems.iter().enumerate() {
        let system = &s["system"];
        let used = uses(system);
        let set: Vec<u64> = used
            .iter()
            .filter(|(_, how)| *how == Use::Set)
            .map(|(cp, _)| *cp)
            .collect();
        let placed: Vec<u64> = used
            .iter()
            .filter(|(_, how)| *how == Use::Place)
            .map(|(cp, _)| *cp)
            .collect();
        let unset_above = parent[i].is_some_and(|p| out[p].is_none());
        if unset_above
            || placed
                .iter()
                .any(|cp| !points.fed.contains(cp) && !set.contains(cp))
        {
            continue;
        }
        let path = s["path"].as_str().unwrap_or_default();
        let r = reach(system, points, parent[i].and_then(|p| out[p]))
            .map_err(|why| format!("{path}: {why}"))?;
        // A system placed about another of its control points is as far off as that one is.
        let off = if placed.iter().any(|&cp| cp != 0) {
            spread(system, points).map_err(|why| format!("{path}: {why}"))?
        } else {
            0.0
        };
        out[i] = Some(r + off);
    }
    Ok(out)
}

/// A particle file of the cache, read as the client reads it, and the effect made of it.
pub(crate) fn effect_file(name: &str) -> (Value, Arc<Effect>) {
    let root = std::path::PathBuf::from(std::env::var("DBOTA_ASSETS").expect("DBOTA_ASSETS"));
    let text = std::fs::read_to_string(root.join("particles").join(name)).expect("the file");
    let json: Value = serde_json::from_str(&text).expect("its JSON");
    let parsed: Vec<(String, heatel::particle::System)> = json["systems"]
        .as_array()
        .expect("its systems")
        .iter()
        .map(|e| {
            let path = e["path"].as_str().expect("a path").to_owned();
            let system = serde_json::from_value(e["system"].clone()).expect("a system");
            (path, system)
        })
        .collect();
    (json, Arc::new(Effect::new(&parsed)))
}

/// Particles of `effect` farther from where it stands than their systems' reach.
fn strays(
    playing: &mut Playing,
    handle: crate::particles::Handle,
    reach: &[Option<f32>],
    at: Vec3,
) -> Vec<String> {
    let instance = playing.get_mut(handle).expect("playing");
    let mut out = Vec::new();
    for (def, p, age) in instance.particles() {
        let Some(limit) = reach[def] else { continue };
        let off = p.distance(at);
        if off > limit {
            out.push(format!(
                "system {def}: {off:.0} off at {age:.2} s, past its reach {limit:.0}"
            ));
        }
    }
    out
}

/// Shadow Fiend's worn effects and the files they play.
const WORN: [&str; 3] = [
    "nevermore_trail.json",
    "nevermore_ambient_glow.json",
    "shadow_fiend_ambient_eyes.json",
];

/// Frames to play: of 12 to 22 ms, of 16 ms with a 1 ms frame and then a 200 ms one every 40, or
/// 12 ms and 100 ms frames taking turns.
#[derive(Clone, Copy, Debug)]
enum Frames {
    Uneven,
    Hitches,
    TakingTurns,
}

impl Frames {
    fn lengths(self) -> Vec<f32> {
        let mut seed = 12_345u32;
        (0..240)
            .map(|frame| {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                match (self, frame % 40) {
                    (Self::Uneven, _) => 0.012 + (seed >> 8) as f32 / 16_777_216.0 * 0.01,
                    (Self::Hitches, 38) => 0.001,
                    (Self::Hitches, 39) => 0.2,
                    (Self::Hitches, _) => 0.016,
                    (Self::TakingTurns, n) if n % 2 == 0 => 0.012,
                    (Self::TakingTurns, _) => 0.1,
                }
            })
            .collect()
    }
}

/// A velocity only a position oscillation (not offsetting the place before) builds, under a cap:
/// the reach is the cap's travel over the life, though no initial speed or force gives one.
#[test]
fn an_oscillation_under_a_cap_reaches_as_far_as_the_cap_travels() {
    let system = serde_json::json!({
        "constants": { "lifetime": 2.0, "radius": 5.0 },
        "initializers": [],
        "operators": [
            { "class": { "BasicMovement": { "gravity": { "Literal": [0.0, 0.0, 0.0] } } } },
            { "class": { "OscillateVector": {
                "field": 0, "offset_position": false,
                "rate_min": [-6.0, -6.0, -6.0], "rate_max": [6.0, 6.0, 6.0],
                "rate_scale": { "Literal": 1.0 }
            } } },
            { "class": { "MaxVelocity": { "max": { "Literal": 100.0 } } } }
        ],
        "forces": [],
        "constraints": []
    });
    let points = Points {
        fed: &[0],
        value: 0.0,
    };
    let reach = reach(&system, points, None).expect("a bound");
    assert!(
        (reach - 200.0).abs() < 1e-3,
        "100 a second for 2 s: {reach}"
    );
}

#[test]
fn a_distance_shell_can_place_particles_beyond_their_free_motion() {
    let system = serde_json::json!({
        "constants": { "lifetime": 1.0 },
        "initializers": [],
        "operators": [{ "class": { "BasicMovement": { "gravity": { "Literal": [0, 0, 0] } } } }],
        "forces": [],
        "constraints": [{
            "class": { "Distance": {
                "min_distance": { "Literal": 0.0 },
                "max_distance": { "Literal": 10.0 },
                "center_offset": { "Literal": [100, 0, 0] },
                "global_center": false,
                "transform": { "ControlPoint": { "control_point": 0, "orientation": true, "range_max": null } }
            } },
            "timing": { "end_cap": "Always", "fade_in": [0, 0], "fade_out": [0, 0],
                "oscillate": 0, "run_once": false, "strength": { "Literal": 1 } }
        }]
    });
    let bound = reach(
        &system,
        Points {
            fed: &[0],
            value: 0.0,
        },
        None,
    )
    .expect("a static outer shell has a finite reach");
    assert!(
        bound >= 90.0,
        "the origin projects to the shell's near edge: {bound}"
    );
    assert!(
        bound <= 110.0,
        "the shell's far edge bounds its reach: {bound}"
    );
}

#[test]
fn shadow_fiends_worn_effects_keep_their_particles_within_reach_on_the_stage_and_in_the_world() {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default);
    // Near the world's origin, so a control point read as a value (a velocity) is a modest one.
    let at = Vec3::new(40.0, -30.0, 20.0);
    let set_at = at + Vec3::Z * 150.0;
    let mut failures = Vec::new();
    for file in WORN {
        let (json, effect) = effect_file(file);
        // Every control point any of its systems uses: control point 0 and the rest above it.
        let fed: Vec<u64> = std::iter::once(0)
            .chain(
                json["systems"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .flat_map(|s| uses(&s["system"]).into_iter().map(|(cp, _)| cp)),
            )
            .collect();
        let points = Points {
            fed: &fed,
            value: at.length().max(set_at.length()),
        };
        let reach = reaches(&json, points).unwrap_or_else(|why| panic!("{file}: {why}"));
        let unmeasured: Vec<usize> = (0..reach.len()).filter(|&i| reach[i].is_none()).collect();
        assert!(
            unmeasured.is_empty(),
            "{file}: systems of no reach: {unmeasured:?}"
        );
        for frames in [Frames::Uneven, Frames::Hitches, Frames::TakingTurns] {
            for stage in [true, false] {
                let mut playing = Playing::default();
                let handle = if stage {
                    playing.hold_on_stage(&effect, 7)
                } else {
                    playing.hold(&effect, 7, EffectSight::Public)
                }
                .expect("started");
                let basis = Mat3::from_rotation_z(std::f32::consts::FRAC_PI_2);
                // The stage draws (and steps) every other frame, by the time since, as the
                // portrait does at 30 a second.
                let mut owed = 0.0f32;
                for (frame, dt) in frames.lengths().into_iter().enumerate() {
                    if let Some(e) = playing.get_mut(handle) {
                        e.bind_control_point(0, at, basis, 0.98);
                        for &cp in fed.iter().filter(|&&cp| cp != 0) {
                            e.set_control_point(cp as usize, set_at, Vec3::ZERO, false);
                        }
                    }
                    owed += dt;
                    let drawn = frame % 2 == 1;
                    let stage_dt = (stage && drawn).then(|| std::mem::take(&mut owed));
                    playing.advance(dt, stage_dt, None);
                    let place = if stage { "stage" } else { "world" };
                    failures.extend(
                        strays(&mut playing, handle, &reach, at)
                            .into_iter()
                            .map(|s| format!("{file} {place} ({frames:?} frames): {s}")),
                    );
                }
            }
        }
    }
    failures.dedup();
    let world = failures.iter().filter(|f| f.contains(" world (")).count();
    assert!(
        failures.is_empty(),
        "{} strays ({world} in the world): {:#?}",
        failures.len(),
        &failures[..failures.len().min(20)]
    );
}
