//! The map's ambient effects: every particle system Dota's map places (fireflies by night,
//! butterflies and dragonflies by day, moths, bats, the lamps' flames, the river's flow and its
//! waterfalls' splashes, the Dire's lava), read by the importer into `map/mid/ambient.txt` and
//! its effects into `particles/map/`. Each shows by day, by night or both as the map says, and
//! where the fog hides the ground only if the map lets it. Only those near the camera play:
//! the ones whose place the view takes in, nearest the camera's focus first, at most
//! [`MAX_ACTIVE`]; one that leaves is ended, and its particles live out their lives.

use std::collections::BTreeSet;

use bevy::{
    camera::primitives::{Frustum, Sphere},
    prelude::*,
};

use crate::camera::RtsCamera;
use crate::daylight::Daylight;
use crate::fog::FogGrid;
use crate::manifest::Manifest;
use crate::particles::{Handle, Library, Playing};
use crate::session::Session;

/// Lines of the file read at most.
const MAX_LINES: usize = 4096;
/// Effects playing at once at most.
pub const MAX_ACTIVE: usize = 48;
/// World units about an emitter that count as its place for the view: the view's edge plus a
/// small margin, so one starts just before it comes into sight. Most of the map's effects keep
/// their particles well within it; the widest (fireflies, butterflies, bats) scatter theirs
/// some 260 units out, so one just past the margin could have had a few at the screen's edge.
const REACH: f32 = 200.0;
/// bota = Dota + this, on x and y.
const DOTA_TO_BOTA: f32 = 9216.0;

/// One particle system the map places.
pub struct Emitter {
    /// Its effect's name in the library: `map.<the effect's file stem>`.
    pub name: String,
    /// Where it is in bota's units (x, y).
    pub at: Vec2,
    /// Its place and facing in the particles' frame.
    place: Vec3,
    forward: Vec3,
    /// Its middle in the world, for the view.
    middle: Vec3,
    day: bool,
    night: bool,
    fog: bool,
    data: Option<(usize, Vec3)>,
}

/// The map's emitters, and the ones playing now (index into `emitters`, the effect playing).
#[derive(Resource, Default)]
pub struct Ambient {
    pub emitters: Vec<Emitter>,
    active: Vec<(usize, Handle)>,
    /// The emitters wanted this frame with their distance, kept for their capacity.
    wanted: Vec<(usize, f32)>,
}

impl Ambient {
    /// The emitters playing now.
    #[cfg(test)]
    pub fn active(&self) -> impl Iterator<Item = &Emitter> {
        self.active.iter().map(|(i, _)| &self.emitters[*i])
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Ambient>()
        .add_systems(
            Update,
            load.after(crate::particles::Loaded)
                .run_if(resource_added::<crate::particles::Library>),
        )
        .add_systems(Update, play);
}

/// Where Source's angles (pitch, yaw, roll in degrees) turn a system's forward, in the
/// particles' frame (x east, y north, z up).
fn forward([pitch, yaw, _]: [f32; 3]) -> Vec3 {
    let (pitch, yaw) = (pitch.to_radians(), yaw.to_radians());
    Vec3::new(
        pitch.cos() * yaw.cos(),
        pitch.cos() * yaw.sin(),
        -pitch.sin(),
    )
}

/// Reads the map's emitters and adds their effects to the library; a cache without them
/// plays none.
fn load(
    mut ambient: ResMut<Ambient>,
    library: Option<ResMut<Library>>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    let (Some(root), Some(mut library)) = (manifest.root.as_deref(), library) else {
        return;
    };
    let Some(text) = crate::cachefs::read_small(root, "map/mid/ambient.txt")
        .and_then(|bytes| String::from_utf8(bytes).ok())
    else {
        return;
    };
    let lines = crate::cache_text::ambient(&text, MAX_LINES);
    let mut unsupported = BTreeSet::new();
    let stems: BTreeSet<&str> = lines.iter().map(|l| l.effect.as_str()).collect();
    for stem in stems {
        let path = format!("particles/map/{stem}.json");
        let Some(bytes) = crate::cachefs::read_small(root, &path) else {
            continue;
        };
        match library.add(&format!("map.{stem}"), &bytes, &server) {
            Ok(classes) => unsupported.extend(classes),
            Err(why) => error!("map effect {stem} ({path}): {why}; drawn without it"),
        }
    }
    if !unsupported.is_empty() {
        let list: Vec<String> = unsupported.into_iter().collect();
        warn!(
            "particle classes of the map's effects not simulated, left out: {}",
            list.join(", ")
        );
    }
    ambient.emitters = lines
        .into_iter()
        .filter_map(|line| {
            let name = format!("map.{}", line.effect);
            library.effect(&name)?;
            let [x, y, z] = line.at;
            let at = Vec2::new(x + DOTA_TO_BOTA, y + DOTA_TO_BOTA);
            Some(Emitter {
                name,
                at,
                place: Vec3::new(at.x, at.y, z),
                forward: forward(line.angles),
                middle: Vec3::new(at.x, z, -at.y),
                day: line.day,
                night: line.night,
                fog: line.fog,
                data: line.data.map(|(cp, v)| (cp, Vec3::from(v))),
            })
        })
        .collect();
}

/// Starts the emitters the view now takes in, nearest first and within the budget, and ends
/// the rest.
fn play(
    session: NonSend<Session>,
    daylight: Res<Daylight>,
    fog: Res<FogGrid>,
    camera: Query<(&RtsCamera, &Frustum)>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut ambient: ResMut<Ambient>,
) {
    let ambient = &mut *ambient;
    ambient.wanted.clear();
    if let (Some(_), Ok((camera, frustum))) = (session.0.view(), camera.single()) {
        let night = daylight.night >= 0.5;
        let focus = Vec2::new(camera.focus.x, -camera.focus.z);
        for (i, e) in ambient.emitters.iter().enumerate() {
            let shown = if night { e.night } else { e.day };
            let unfogged = e.fog || !fog.on || fog.lit(e.at.x, e.at.y);
            let sphere = Sphere {
                center: e.middle.into(),
                radius: REACH,
            };
            if shown && unfogged && frustum.intersects_sphere(&sphere, true) {
                ambient.wanted.push((i, e.at.distance_squared(focus)));
            }
        }
        ambient.wanted.sort_unstable_by(|a, b| a.1.total_cmp(&b.1));
        ambient.wanted.truncate(MAX_ACTIVE);
    }
    let Ambient {
        emitters,
        active,
        wanted,
    } = ambient;
    active.retain(|(i, handle)| {
        let keep = wanted.iter().any(|(w, _)| w == i);
        if !keep && let Some(instance) = playing.get_mut(*handle) {
            instance.end();
        }
        keep
    });
    for (i, _) in wanted.iter() {
        if active.iter().any(|(a, _)| a == i) {
            continue;
        }
        let e = &emitters[*i];
        let seed = (*i as u32).wrapping_mul(0x9E37_79B9);
        let Some(handle) = library
            .as_deref()
            .and_then(|l| l.effect(&e.name))
            .and_then(|effect| playing.hold(effect, seed))
        else {
            continue;
        };
        if let Some(instance) = playing.get_mut(handle) {
            instance.set_control_point(0, e.place, e.forward, true);
            if let Some((cp, value)) = e.data {
                instance.set_control_point(cp, value, Vec3::ZERO, true);
            }
        }
        active.push((*i, handle));
    }
}
