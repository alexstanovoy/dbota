//! Effects bound to the game's facts by data: the `fx` lines of the manifest name a Dota item, a
//! modifier or a unit and the particle and sound Dota shows with it (grammar in the manifest's
//! header). Three triggers feed them: an item used (`EventKind::ItemUsed`), a throw come down
//! (`EventKind::Landed`), and a modifier or a unit of a Dota name worn, from the moment a unit
//! shows it until it goes. A new item with an effect adds lines there, not code.

use std::collections::HashMap;

use bevy::{audio::Volume, prelude::*};
use bota_proto::{EntityId, EventKind, Inflictor, UnitView, Vec2 as Spot};

use crate::camera::RtsCamera;
use crate::config::Settings;
use crate::manifest::{At, Feed, Fx, Manifest, Trigger};
use crate::map::Ground;
use crate::particles::{Handle, Library, Playing};
use crate::session::Session;
use crate::units::{Dying, UnitIndex};

/// The control points a bound particle is given: where it plays, and what a line's `cp1=` feeds.
const PLACE: usize = 0;
const FEED: usize = 1;

/// What the game has sounded through the bound lines, by the logical prefix of the line's sound:
/// for the end-to-end tests to ask what was heard.
#[derive(Resource, Default)]
pub struct Heard(pub HashMap<String, u32>);

/// The worn effects playing: by the wearing unit and the line, the particle if the line has one.
#[derive(Resource, Default)]
struct Worn(HashMap<(EntityId, usize), Option<Handle>>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Heard>()
        .init_resource::<Worn>()
        .add_systems(Update, (told, worn));
}

/// A point of the sim as a point of the ground, in the world's frame.
fn on_ground(ground: &Ground, at: Spot) -> Vec3 {
    let (x, y) = (at.x.to_f32(), at.y.to_f32());
    Vec3::new(x, ground.at(x, y), -y)
}

/// What a bound line plays with: the audio, the camera that fades it, and the counters.
#[derive(bevy::ecs::system::SystemParam)]
struct Stage<'w, 's> {
    commands: Commands<'w, 's>,
    server: Res<'w, AssetServer>,
    manifest: Res<'w, Manifest>,
    settings: Res<'w, Settings>,
    args: Res<'w, crate::Args>,
    camera: Single<'w, 's, &'static RtsCamera>,
    heard: ResMut<'w, Heard>,
}

impl Stage<'_, '_> {
    /// Sounds one of the sounds under `prefix` where `at` is (Bevy frame), faded by its distance
    /// from the camera.
    fn sound(&mut self, prefix: &str, at: Vec3, n: u32) {
        let paths = self.manifest.sounds(prefix);
        let Some(path) = paths.get(n as usize % paths.len().max(1)) else {
            return;
        };
        *self.heard.0.entry(prefix.to_owned()).or_default() += 1;
        if self.settings.muted || self.args.mute {
            return;
        }
        let focus = Vec2::new(self.camera.focus.x, -self.camera.focus.z);
        let gain = crate::sound::fall_off((at.x, -at.z), focus);
        if gain < 0.03 {
            return;
        }
        self.commands.spawn((
            AudioPlayer::<AudioSource>::new(self.server.load(path.clone())),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
        ));
    }
}

/// The `fx` lines of a trigger and a subject.
fn lines<'a>(
    manifest: &'a Manifest,
    trigger: Trigger,
    subject: &'a str,
) -> impl Iterator<Item = &'a Fx> {
    manifest
        .fx
        .iter()
        .filter(move |fx| fx.trigger == trigger && fx.subject == subject)
}

/// Fires the lines of every item used and every throw come down that the last pump told.
fn told(
    session: NonSend<Session>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut stage: Stage,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let tick = view.tick;
    for (n, event) in core.told.iter().enumerate() {
        let (trigger, item, caster, point) = match event {
            EventKind::ItemUsed { caster, item, at } => (Trigger::Use, *item, *caster, *at),
            EventKind::Landed {
                inflictor: Inflictor::Item(item),
                caster,
                at,
            } => (Trigger::Land, *item, *caster, Some(*at)),
            _ => continue,
        };
        let Some(key) = stage.manifest.dota_name("item", item.0) else {
            continue;
        };
        let subject = format!("item.{key}");
        let stood = |v: Option<&bota_proto::WorldView>| {
            v.zip(caster)
                .and_then(|(v, id)| v.unit(id))
                .map(|u| u.pos)
        };
        let places = [
            (At::Caster, stood(Some(view))),
            (At::CasterBefore, stood(core.prev_view())),
            (At::Point, point),
        ];
        let seed = tick.wrapping_mul(31).wrapping_add(n as u32);
        let fxs: Vec<Fx> = lines(&stage.manifest, trigger, &subject).cloned().collect();
        for fx in fxs {
            let Some((_, Some(spot))) = places.iter().find(|(at, _)| *at == fx.at) else {
                continue;
            };
            let at = on_ground(&ground, *spot);
            if let Some(name) = &fx.particle {
                crate::effects::play_once(library.as_deref(), &mut playing, name, at, seed);
            }
            if let Some(prefix) = &fx.sound {
                stage.sound(prefix, at, seed);
            }
        }
    }
}

/// Whether a unit wears what a worn line is about.
fn wears(fx: &Fx, unit: &UnitView, manifest: &Manifest) -> bool {
    if let Some(name) = fx.subject.strip_prefix("unit.") {
        return crate::units::npc_unit(unit) == Some(name);
    }
    let Some(modifier) = fx.subject.strip_prefix("modifier.") else {
        return false;
    };
    unit.effects.iter().any(|effect| {
        manifest.dota_name("modifier", effect.id.0) == Some(modifier)
    })
}

/// Starts what each unit that has come to wear a worn line's subject wears, keeps its particle at
/// the unit, and ends it when the unit stops wearing it, falls or leaves the view.
fn worn(
    session: NonSend<Session>,
    index: Res<UnitIndex>,
    dying: Query<(), With<Dying>>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut worn: ResMut<Worn>,
    mut stage: Stage,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let standing = |id: &EntityId| {
        index
            .0
            .get(id)
            .is_some_and(|entity| !dying.contains(*entity))
    };
    let rules: Vec<(usize, Fx)> = stage
        .manifest
        .fx
        .iter()
        .enumerate()
        .filter(|(_, fx)| fx.trigger == Trigger::Worn)
        .map(|(i, fx)| (i, fx.clone()))
        .collect();
    if rules.is_empty() {
        return;
    }
    worn.0.retain(|(id, rule), handle| {
        let keep = standing(id)
            && view
                .unit(*id)
                .zip(rules.iter().find(|(i, _)| i == rule))
                .is_some_and(|(unit, (_, fx))| wears(fx, unit, &stage.manifest));
        if !keep
            && let Some(instance) = handle.and_then(|h| playing.get_mut(h))
        {
            instance.end();
        }
        keep
    });
    for unit in view.units.iter().filter(|u| standing(&u.id)) {
        let at = on_ground(&ground, unit.pos);
        for (rule, fx) in &rules {
            if !wears(fx, unit, &stage.manifest) {
                continue;
            }
            let key = (unit.id, *rule);
            if !worn.0.contains_key(&key) {
                let seed = unit.id.idx ^ unit.id.generation.rotate_left(16);
                let handle = fx
                    .particle
                    .as_deref()
                    .and_then(|name| library.as_deref()?.effect(name))
                    .and_then(|effect| playing.start(effect, seed));
                if let Some(instance) = handle.and_then(|h| playing.get_mut(h))
                    && let Some(Feed::Sight) = fx.cp1
                {
                    let sight = Vec3::new(unit.true_sight_radius.to_f32(), 0.0, 0.0);
                    instance.set_control_point(FEED, sight, Vec3::ZERO, true);
                }
                if let Some(prefix) = &fx.sound {
                    stage.sound(prefix, at, seed);
                }
                worn.0.insert(key, handle);
            }
            if let Some(instance) = worn.0[&key].and_then(|h| playing.get_mut(h)) {
                let place = Vec3::new(at.x, -at.z, at.y);
                instance.set_control_point(PLACE, place, Vec3::ZERO, false);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `fx` line of the manifest reads, names an item or modifier ids.txt knows (a unit
    /// a `unit` line draws), and names particles and sounds the manifest imports: a typo in a
    /// line would otherwise show as an effect that never plays.
    #[test]
    fn every_fx_line_reads_and_names_what_the_manifest_has() {
        let manifest = Manifest::load(std::path::Path::new("/nonexistent"));
        let lines: Vec<Vec<&str>> = crate::manifest::MANIFEST
            .lines()
            .map(|l| l.split('#').next().unwrap_or_default())
            .filter_map(|l| l.trim().strip_prefix("fx "))
            .map(|l| l.split_whitespace().collect())
            .collect();
        assert!(!lines.is_empty());
        let listed = |kind: &str, logical: &str| {
            crate::manifest::MANIFEST.lines().any(|l| {
                let mut words = l.split_whitespace();
                words.next() == Some(kind) && words.next().is_some_and(|w| w.starts_with(logical))
            })
        };
        for words in lines {
            let fx = Fx::parse(&words).unwrap_or_else(|why| panic!("{words:?}: {why}"));
            let known = match fx.subject.split_once('.') {
                Some(("item", key)) => manifest
                    .ids
                    .iter()
                    .any(|(k, _, name)| k == "item" && name == key),
                Some(("modifier", name)) => manifest
                    .ids
                    .iter()
                    .any(|(k, _, n)| k == "modifier" && n == name),
                Some(("unit", npc)) => manifest.unit_art(npc).is_some(),
                _ => false,
            };
            assert!(known, "{words:?}: no such subject");
            if let Some(particle) = &fx.particle {
                assert!(listed("particle", particle), "{words:?}: no such particle");
            }
            if let Some(sound) = &fx.sound {
                assert!(listed("sound", sound), "{words:?}: no such sound");
            }
        }
    }
}
