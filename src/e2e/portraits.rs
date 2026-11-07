//! The HUD portrait as Dota defines it in the install's `scripts/npc/portraits.txt`, for every
//! hero: the camera stands where the hero model's entry puts it (`cameras`, `default`:
//! `PortraitPosition`, `PortraitAngles`, `PortraitFOV`, in the model's own inches and degrees,
//! scaled with the model; the field of view across the picture's width, as Source's), its key
//! light shines along the entry's spot's angles (`PortraitLightAngles`), every side of the model
//! gets the entry's shadow colour and the side the ambient light comes from its ambient colour
//! (`PortraitAmbientDirection`); what a white surface sends back from each light is Dota's
//! colour times its scale (`PortraitLightColor`/`Scale`, `PortraitAmbientColor`/`Scale`,
//! `PortraitShadowColor`/`Scale`), one factor for all, by Bevy's own light equations. What an
//! entry leaves out is the file's `default` entry's, key by key. The hero wears its particles in
//! it as it does in the world (no hero sets `PortraitHideParticles`): each effect the manifest
//! has the hero wear plays on the stage, in its view, its cards on the stage's layer alone.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::light::{AmbientLight, DirectionalLight};
use bevy::prelude::*;
use bota_proto::HeroId;
use heatel::{DirSource, Vpk, find_game_dir};

use super::harness::{Client, Server};
use crate::portrait::Stage;

/// A KeyValues block of `portraits.txt`, read plainly: quoted keys, quoted values or blocks.
enum Kv {
    Text(String),
    Block(Vec<(String, Kv)>),
}

impl Kv {
    fn get(&self, key: &str) -> Option<&Kv> {
        let Kv::Block(items) = self else { return None };
        items
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
    }

    fn numbers(&self, key: &str) -> Option<Vec<f32>> {
        let Kv::Text(text) = self.get(key)? else {
            return None;
        };
        Some(
            text.split_whitespace()
                .filter_map(|w| w.parse().ok())
                .collect(),
        )
    }
}

fn parse(tokens: &mut std::iter::Peekable<std::vec::IntoIter<String>>) -> Vec<(String, Kv)> {
    let mut items = Vec::new();
    while let Some(key) = tokens.next() {
        if key == "}" {
            break;
        }
        let value = match tokens.next().as_deref() {
            Some("{") => Kv::Block(parse(tokens)),
            Some(text) => Kv::Text(text.to_owned()),
            None => break,
        };
        items.push((key, value));
    }
    items
}

/// The install's `Portraits` block.
fn portraits() -> Kv {
    let dota = std::env::var_os("DBOTA_DOTA_PATH").map(std::path::PathBuf::from);
    let source = DirSource::new(
        find_game_dir(dota.as_deref()).expect("a Dota 2 install; set DBOTA_DOTA_PATH"),
    );
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("its package");
    let bytes = pak
        .read_file(&source, "scripts/npc/portraits.txt")
        .expect("portraits.txt");
    let text = String::from_utf8(bytes).expect("text");
    let mut tokens = Vec::new();
    for line in text.lines() {
        let line = line.split("//").next().unwrap_or_default();
        let mut rest = line;
        while let Some(at) = rest.find(['"', '{', '}']) {
            if rest[at..].starts_with('"') {
                let body = &rest[at + 1..];
                let end = body.find('"').expect("a closed string");
                tokens.push(body[..end].to_owned());
                rest = &body[end + 1..];
            } else {
                tokens.push(rest[at..at + 1].to_owned());
                rest = &rest[at + 1..];
            }
        }
    }
    let mut tokens = tokens.into_iter().peekable();
    parse(&mut tokens)
        .into_iter()
        .find(|(key, _)| key == "Portraits")
        .map(|(_, block)| block)
        .expect("a Portraits block")
}

/// The `.vmdl` of a hero's model and its scale, as the manifest imports it.
fn model_of(name: &str) -> (String, f32) {
    let logical = format!("hero.{name}.model");
    let line = dbota_import::import::MANIFEST
        .lines()
        .find(|l| l.split_whitespace().nth(1) == Some(&logical))
        .expect("the hero's model line");
    let vmdl = line.split_whitespace().nth(2).expect("its source");
    let scale = line
        .split_whitespace()
        .find_map(|w| w.strip_prefix("model_scale="))
        .map_or(1.0, |s| s.parse().expect("a scale"));
    (vmdl.trim_end_matches("_c").to_owned(), scale)
}

/// How many effects the manifest has the hero wear for as long as it stands.
fn worn_effects(name: &str) -> usize {
    worn_lines(name).len()
}

/// The effects the manifest has the hero wear, each its particle's logical name and the control
/// points its line sets (0, where it stands, and each `cpN=`).
fn worn_lines(name: &str) -> Vec<(String, Vec<u64>)> {
    let subjects = [
        format!("hero.{name}.worn"),
        format!("unit.npc_dota_hero_{name}.worn"),
    ];
    let mut out = Vec::new();
    for line in dbota_import::import::MANIFEST.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        if words.first() != Some(&"fx")
            || !words
                .get(1)
                .is_some_and(|s| subjects.iter().any(|w| w == s))
        {
            continue;
        }
        let Some(particle) = words.iter().find_map(|w| w.strip_prefix("particle=")) else {
            continue;
        };
        let fed = words
            .iter()
            .filter_map(|w| w.strip_prefix("cp")?.split_once('=')?.0.parse().ok());
        out.push((particle.to_owned(), std::iter::once(0).chain(fed).collect()));
    }
    out
}

/// Particles and cards of the hero's worn effects on the stage past their systems' reach (from
/// their own data, see `effect_bounds`; a card as far again as its renderer draws past its
/// particle) from the stage's pivot (`stage` in the particles' frame, `pivot` in Bevy's): none
/// may be. A look some system of no reach draws (one placed about a control point the line never
/// sets) is not held.
fn beyond_reach(
    world: &mut World,
    name: &str,
    handles: &[crate::particles::Handle],
    (stage, pivot): (Vec3, Vec3),
) -> Vec<String> {
    use super::effect_bounds::{CP_SPREAD, Points, card_extent, reaches};
    let root = world.resource::<crate::manifest::Manifest>().root.clone();
    let files: Vec<(String, Vec<u64>, String)> = {
        let manifest = world.resource::<crate::manifest::Manifest>();
        worn_lines(name)
            .into_iter()
            .map(|(particle, fed)| {
                let (_, rel) = manifest
                    .particles()
                    .find(|(logical, _)| *logical == particle)
                    .expect("the particle's file");
                (particle, fed, rel.to_owned())
            })
            .collect()
    };
    let effects: Vec<_> = {
        let library = world.resource::<crate::particles::Library>();
        files
            .iter()
            .map(|(particle, _, _)| Arc::clone(library.effect(particle).expect("the worn effect")))
            .collect()
    };
    let mut out = Vec::new();
    // Each look's farthest card: its systems' reach and extent (none: one of no reach draws it).
    let mut looks: HashMap<u16, Option<f32>> = HashMap::new();
    let mut playing = world.resource_mut::<crate::particles::Playing>();
    for ((particle, fed, rel), effect) in files.iter().zip(&effects) {
        let text = std::fs::read_to_string(root.join(rel)).expect("the particle file");
        let json: serde_json::Value = serde_json::from_str(&text).expect("its JSON");
        // A set control point read as a value is its place in the world.
        let points = Points {
            fed,
            value: stage.length() + CP_SPREAD,
        };
        let reach = reaches(&json, points).unwrap_or_else(|why| panic!("{particle}: {why}"));
        for (def, d) in effect.defs.iter().enumerate() {
            let system = &json["systems"][def]["system"];
            for draw in &d.draws {
                let limit = reach[def].map(|r| {
                    r + card_extent(system, draw.renderer, points)
                        .unwrap_or_else(|why| panic!("{particle} system {def}: {why}"))
                });
                let entry = looks.entry(draw.look).or_insert(Some(0.0));
                *entry = entry.zip(limit).map(|(a, b)| a.max(b));
            }
        }
        for handle in handles {
            let Some(instance) = playing.get_mut(*handle) else {
                continue;
            };
            if !Arc::ptr_eq(&instance.effect, effect) {
                continue;
            }
            for (def, at, age) in instance.particles() {
                let Some(limit) = reach[def] else { continue };
                let off = at.distance(stage);
                if off > limit {
                    out.push(format!(
                        "{name} {particle} system {def}: {off:.0} off at {age:.2} s, past its reach {limit:.0}"
                    ));
                }
            }
        }
    }
    let frame = world.resource::<crate::particles::ParticleFrame>();
    let mut held = 0;
    for batch in frame.batches.iter().filter(|b| b.stage) {
        let Some(Some(limit)) = looks.get(&batch.look) else {
            continue;
        };
        for q in &frame.quads[batch.range.start as usize..batch.range.end as usize] {
            held += 1;
            let off = Vec3::from(q.head)
                .distance(pivot)
                .max(Vec3::from(q.tail).distance(pivot));
            if off > *limit {
                out.push(format!(
                    "{name}: a card of look {} {off:.0} off, past its reach {limit:.0}",
                    batch.look
                ));
            }
        }
    }
    if !files.is_empty() && held == 0 {
        out.push(format!(
            "{name}: no card of its worn effects held to a reach"
        ));
    }
    eprintln!("{name}: {held} cards held to their reach");
    out
}

/// A point of the model's own axes (Source's: x ahead, y left, z up) on the stage: the glTF
/// export's (x = left, y = up, z = ahead) turned half round to Bevy's forward, -z.
fn on_stage(v: [f32; 3]) -> Vec3 {
    Vec3::new(-v[1], v[2], -v[0])
}

/// The way Source's pitch and yaw look, in the stage's axes.
fn looking(angles: &[f32]) -> Vec3 {
    let (pitch, yaw) = (angles[0].to_radians(), angles[1].to_radians());
    on_stage([
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    ])
}

fn three(entry: &Kv, default: &Kv, key: &str) -> [f32; 3] {
    let n = entry
        .numbers(key)
        .or_else(|| default.numbers(key))
        .unwrap_or_else(|| panic!("{key}"));
    [n[0], n[1], n[2]]
}

fn one(entry: &Kv, default: &Kv, key: &str) -> f32 {
    entry
        .numbers(key)
        .or_else(|| default.numbers(key))
        .unwrap_or_else(|| panic!("{key}"))[0]
}

fn near(a: Vec3, b: Vec3, what: &str, failures: &mut Vec<String>) {
    if a.distance(b) > 1e-2 * b.length().max(1.0) {
        failures.push(format!("{what}: {a} where Dota's is {b}"));
    }
}

fn srgb_linear(c: [f32; 3], scale: f32) -> [f32; 3] {
    c.map(|v| Color::srgb(v / 255.0, 0.0, 0.0).to_linear().red * scale)
}

#[allow(clippy::too_many_lines)]
fn check_hero(hero: HeroId, all: &Kv, failures: &mut Vec<String>) {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, hero, &[]);
    let name = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .dota_name("hero", hero.0)
        .expect("the hero's name")
        .to_owned();
    let (vmdl, size) = model_of(&name);
    let default = all.get("default").expect("the default entry");
    let entry = all
        .get(&vmdl)
        .unwrap_or_else(|| panic!("{vmdl} in portraits.txt"));
    fn camera_of(kv: &Kv) -> Option<&Kv> {
        kv.get("cameras")?.get("default")
    }
    let default_camera = camera_of(default).expect("the default entry's camera");
    let camera_def = camera_of(entry).unwrap_or(default_camera);
    client.within(Duration::from_secs(120), "the portrait drawn", |c| {
        c.world()
            .resource::<Stage>()
            .actor
            .as_ref()
            .is_some_and(|a| a.face.is_some())
    });
    // The worn effects step and draw a few frames after the model is posed, and on the frames
    // the portrait is drawn (at most 30 a second): one of those is looked at.
    client.frames(30);
    client.until("a frame the portrait is drawn on", |c| {
        c.world().resource::<Stage>().drawing()
    });
    let world = client.world_mut();
    let stage = world.resource::<Stage>();
    let live = stage.live().clone();
    let pivot = stage.actor.as_ref().expect("the model on the stage").pivot;
    let origin = world
        .get::<Transform>(pivot)
        .expect("the stage")
        .translation;
    let mut cameras = world.query::<(&RenderTarget, &Transform, &Projection, &AmbientLight)>();
    let (camera, projection, ambient) = cameras
        .iter(world)
        .find(|(target, ..)| target.as_image() == Some(&live))
        .map(|(_, t, p, a)| (*t, p.clone(), a.clone()))
        .expect("the portrait's camera");
    let at = |v: [f32; 3]| origin + on_stage(v) * size;
    let what = |part: &str| format!("{name}: {part}");
    near(
        camera.translation,
        at(three(camera_def, default_camera, "PortraitPosition")),
        &what("the camera's place"),
        failures,
    );
    let angles = three(camera_def, default_camera, "PortraitAngles");
    near(
        camera.forward().as_vec3(),
        looking(&angles),
        &what("the camera's look"),
        failures,
    );
    // Source's field of view is across the picture's width (`CViewSetup::fov`, "horizontal
    // FOV in degrees"); Bevy's is of its height: tan(v / 2) = tan(h / 2) * height / width.
    let fov = one(camera_def, default_camera, "PortraitFOV");
    let picture = world
        .resource::<Assets<Image>>()
        .get(&live)
        .expect("the portrait's picture")
        .size_f32();
    let vertical = 2.0 * ((fov.to_radians() / 2.0).tan() * picture.y / picture.x).atan();
    match projection {
        Projection::Perspective(p) if (p.fov - vertical).abs() < 1e-4 => {}
        other => failures.push(format!(
            "{name}: the camera's field of view {other:?}, Dota's {fov} across {picture}"
        )),
    }
    let shadow = srgb_linear(
        three(entry, default, "PortraitShadowColor"),
        one(entry, default, "PortraitShadowScale"),
    );
    let key_colour = srgb_linear(
        three(entry, default, "PortraitLightColor"),
        one(entry, default, "PortraitLightScale"),
    );
    let ambient_colour = srgb_linear(
        three(entry, default, "PortraitAmbientColor"),
        one(entry, default, "PortraitAmbientScale"),
    );
    let fill_colour: [f32; 3] = std::array::from_fn(|i| (ambient_colour[i] - shadow[i]).max(0.0));
    let on_layer =
        |layers: &RenderLayers| layers.intersects(&RenderLayers::layer(crate::portrait::LAYER));
    let mut lights = world.query::<(&DirectionalLight, &Transform, &RenderLayers)>();
    let lights: Vec<(DirectionalLight, Vec3)> = lights
        .iter(world)
        .filter(|(.., layers)| on_layer(layers))
        .map(|(l, t, _)| (*l, t.forward().as_vec3()))
        .collect();
    let along = |way: Vec3| {
        lights
            .iter()
            .find(|(_, forward)| forward.distance(way.normalize()) < 1e-2)
            .map(|(light, _)| sent_back(light.color, light.illuminance / std::f32::consts::PI))
    };
    let light_angles = three(entry, default, "PortraitLightAngles");
    let direction = three(entry, default, "PortraitAmbientDirection");
    let key = along(looking(&light_angles));
    let fill = along(on_stage(direction));
    let every_side = sent_back(ambient.color, ambient.brightness * bevy_ambient_gain());
    if lights.len() != 2 {
        failures.push(format!(
            "{name}: {} lights on the stage, Dota's key and ambient",
            lights.len()
        ));
    }
    match (key, fill) {
        (Some(key), Some(fill)) => {
            // What a white matte surface sends back from each light, one scale for all: the
            // shares of Dota's numbers.
            let unit = key.iter().copied().fold(0.0, f32::max)
                / key_colour.iter().copied().fold(0.0, f32::max);
            // The factor itself: a stage lit at all, and not past what a screen shows (a white
            // surface under a unit of Dota's light sends back between 10 and 10000 cd/m²).
            if !(10.0..=10_000.0).contains(&unit) {
                failures.push(format!(
                    "{name}: a unit of Dota's light sends back {unit} cd/m²"
                ));
            }
            for (part, got, want) in [
                ("the key light", key, key_colour),
                ("the ambient light's side", fill, fill_colour),
                ("every side (the shadow colour)", every_side, shadow),
            ] {
                let off = got
                    .iter()
                    .zip(want)
                    .any(|(g, w)| (g - unit * w).abs() > 0.02 * unit * w.max(0.05));
                if off {
                    failures.push(format!(
                        "{name}: {part} sends back {got:?}, Dota's {want:?} times {unit}"
                    ));
                }
            }
        }
        found => failures.push(format!(
            "{name}: no light along Dota's key light's angles or its ambient direction: {found:?}"
        )),
    }
    let worn = worn_effects(&name);
    let handles: Vec<crate::particles::Handle> = world
        .resource::<crate::fx::portrait::StageFx>()
        .handles()
        .collect();
    if handles.len() != worn {
        failures.push(format!(
            "{name}: {} effects on the stage, it wears {worn}",
            handles.len()
        ));
    }
    let mut playing = world.resource_mut::<crate::particles::Playing>();
    for handle in &handles {
        match playing.get_mut(*handle) {
            Some(effect) if effect.portrait && !effect.out_of_view() && !effect.ending() => {}
            Some(_) => failures.push(format!("{name}: an effect on the stage not in its view")),
            None => failures.push(format!("{name}: an effect on the stage ended")),
        }
    }
    // The stage stands at the pivot: in the particles' frame (x, -z, height).
    let stage = Vec3::new(origin.x, -origin.z, origin.y);
    failures.extend(beyond_reach(world, &name, &handles, (stage, origin)));
    let frame = world.resource::<crate::particles::ParticleFrame>();
    let count = |stage: bool, near: bool| -> usize {
        frame
            .batches
            .iter()
            .filter(|b| b.stage == stage)
            .flat_map(|b| &frame.quads[b.range.start as usize..b.range.end as usize])
            // A card is on the stage when either end of it is (a trail's stretches far).
            .filter(|q| {
                let at = |y: f32| (y - origin.y).abs() < 2_000.0;
                (at(q.head[1]) || at(q.tail[1])) == near
            })
            .count()
    };
    let (staged, leaked) = (count(true, true), count(false, true));
    if (worn > 0) != (staged > 0) {
        failures.push(format!(
            "{name}: {staged} cards on the stage's layer for {worn} effects"
        ));
    }
    if leaked > 0 {
        failures.push(format!(
            "{name}: {leaked} cards of the stage among the world's"
        ));
    }
    eprintln!("{name}: {worn} worn effects, {staged} cards on the stage");
}

/// The portrait's effects are the shown unit's: another unit of the same model selected, the
/// first one's effects end and the second one's play on the same copy.
#[test]
fn the_portraits_effects_follow_the_unit_shown_not_its_model() {
    let server = Server::start(4, &[]);
    for _ in 0..3 {
        super::harness::quiet_seat(&server, HeroId::SHADOW_FIEND);
    }
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let effects = |c: &Client| -> Vec<crate::particles::Handle> {
        c.world()
            .resource::<crate::fx::portrait::StageFx>()
            .handles()
            .collect()
    };
    client.within(
        Duration::from_secs(120),
        "Shadow Fiend's effects in his portrait",
        |c| effects(c).len() == worn_effects("nevermore"),
    );
    let mine = effects(&client);
    assert!(
        !mine.is_empty(),
        "Shadow Fiend wears effects in his portrait"
    );
    let me = client.me().clone();
    let ally = client
        .view()
        .units
        .iter()
        .find(|u| u.hero.is_some() && u.team == me.team && u.id != me.id)
        .map(|u| u.id)
        .expect("an allied Shadow Fiend");
    client.core_mut().selected = Some(ally);
    client.within(
        Duration::from_secs(30),
        "the ally's effects in the portrait",
        |c| {
            let now = effects(c);
            now.len() == mine.len() && now.iter().all(|h| !mine.contains(h))
        },
    );
    let mut playing = client
        .world_mut()
        .resource_mut::<crate::particles::Playing>();
    for handle in mine {
        assert!(
            playing.get_mut(handle).is_none_or(|e| e.ending()),
            "the first unit's effect still plays"
        );
    }
}

/// What Bevy's ambient light gives a white matte surface per unit of brightness:
/// `EnvBRDFApprox(diffuse, F_AB(1, NdotV))` of bevy_pbr 0.19's `pbr_ambient.wgsl`, `F_AB` its
/// polynomial (no `dfg_lut`), worked out here at a roughness of 1 facing the camera.
fn bevy_ambient_gain() -> f32 {
    let (c0, c1) = (
        [-1.0f32, -0.0275, -0.572, 0.022],
        [1.0f32, 0.0425, 1.04, -0.04],
    );
    let r: [f32; 4] = std::array::from_fn(|i| c0[i] + c1[i]);
    let a004 = (r[0] * r[0]).min((-9.28f32).exp2()) * r[0] + r[1];
    (-1.04 * a004 + r[2]).max(0.00005)
}

/// What a white matte surface sends back from a light of `color` at `strength` (linear).
fn sent_back(color: Color, strength: f32) -> [f32; 3] {
    let c = color.to_linear();
    [c.red, c.green, c.blue].map(|v| v * strength)
}

#[test]
fn every_heros_portrait_is_framed_lit_and_dressed_as_dotas_portraits_define_it() {
    let all = portraits();
    let mut failures = Vec::new();
    for hero in bota_core::HEROES.iter().map(|h| h.id) {
        check_hero(hero, &all, &mut failures);
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
