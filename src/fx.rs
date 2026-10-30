//! Effects bound to the game's facts by data: the `fx` lines of the manifest name a Dota item, a
//! modifier, a unit, a hero or a rune and the particle and sound Dota shows with it (grammar in
//! the manifest's header). The game's events feed them: an item used (`EventKind::ItemUsed`), a
//! throw come down (`EventKind::Landed`), a rune appeared, taken into effect or stored in a Bottle
//! (`RuneSpawned`, `RuneActivated`, `RuneBottled`); and what is worn: a modifier, a unit of a Dota
//! name or a hero, from the moment a unit shows it until it goes, and a rune lying in sight. A new
//! item, hero or rune with an effect adds lines there, not code.

use std::collections::HashMap;

use bevy::{audio::Volume, prelude::*};
use bota_proto::{EntityId, EventKind, Inflictor, ItemId, RuneView, UnitView, Vec2 as Spot};

use crate::camera::RtsCamera;
use crate::config::Settings;
use crate::fog::FogGrid;
use crate::manifest::{At, Feed, Fx, Manifest, Trigger};
use crate::map::Ground;
use crate::particles::{Handle, Library, Playing};
use crate::session::Session;
use crate::units::{Attachments, Dying, UnitIndex};

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

/// The trigger, the subject (by its Dota name), the unit and the spot of an event an `fx` line may
/// be bound to; none for any other.
fn bound(
    event: &EventKind,
    manifest: &Manifest,
) -> Option<(Trigger, String, Option<EntityId>, Option<Spot>)> {
    let item = |id: ItemId| Some(format!("item.{}", manifest.dota_name("item", id.0)?));
    let rune = |kind| format!("rune.{}", crate::runes::art(kind));
    Some(match *event {
        EventKind::ItemUsed {
            caster,
            item: id,
            at,
        } => (Trigger::Use, item(id)?, caster, at),
        EventKind::Landed {
            inflictor: Inflictor::Item(id),
            caster,
            at,
        } => (Trigger::Land, item(id)?, caster, Some(at)),
        EventKind::RuneSpawned { kind, at } => (Trigger::Spawn, rune(kind), None, Some(at)),
        EventKind::RuneActivated { unit, kind, .. } => {
            (Trigger::Activate, rune(kind), Some(unit), None)
        }
        // The one item a rune is stored in.
        EventKind::RuneBottled { unit, .. } => {
            (Trigger::Store, item(ItemId::BOTTLE)?, Some(unit), None)
        }
        _ => return None,
    })
}

/// Fires the lines of every event the last pump told that a line is bound to.
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
        let Some((trigger, subject, caster, point)) = bound(event, &stage.manifest) else {
            continue;
        };
        let stood = |v: Option<&bota_proto::WorldView>| {
            v.zip(caster).and_then(|(v, id)| v.unit(id)).map(|u| u.pos)
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
    if let Some(name) = fx.subject.strip_prefix("hero.") {
        return unit
            .hero
            .is_some_and(|hero| manifest.dota_name("hero", hero.0) == Some(name));
    }
    let Some(modifier) = fx.subject.strip_prefix("modifier.") else {
        return false;
    };
    unit.effects
        .iter()
        .any(|effect| manifest.dota_name("modifier", effect.id.0) == Some(modifier))
}

/// Whether a rune lying is what a worn line is about.
fn lies(fx: &Fx, rune: &RuneView) -> bool {
    fx.subject.strip_prefix("rune.") == Some(crate::runes::art(rune.kind))
}

/// Starts what a unit or a rune that has come to wear a worn line's subject wears (`sight`: how
/// far it gives true sight): its particle, fed what the line says, and its sound; the particle,
/// if any started.
fn begin(
    fx: &Fx,
    (id, sight): (EntityId, f32),
    at: Vec3,
    library: Option<&Library>,
    playing: &mut Playing,
    stage: &mut Stage,
) -> Option<Handle> {
    let seed = id.idx ^ id.generation.rotate_left(16);
    let handle = fx
        .particle
        .as_deref()
        .and_then(|name| library?.effect(name))
        .and_then(|effect| playing.hold(effect, seed));
    if let Some(instance) = handle.and_then(|h| playing.get_mut(h))
        && let Some(Feed::Sight) = fx.cp1
    {
        instance.set_control_point(FEED, Vec3::new(sight, 0.0, 0.0), Vec3::ZERO, true);
    }
    if let Some(prefix) = &fx.sound {
        stage.sound(prefix, at, seed);
    }
    handle
}

/// The units drawn: by their handle, whether they are falling, and where their models carry
/// their hit locations.
#[derive(bevy::ecs::system::SystemParam)]
struct Bodies<'w, 's> {
    index: Res<'w, UnitIndex>,
    dying: Query<'w, 's, (), With<Dying>>,
    attachments: Query<'w, 's, &'static Attachments>,
    places: Query<'w, 's, &'static GlobalTransform>,
}

impl Bodies<'_, '_> {
    /// Where unit `id`'s model takes its hits, in the particles' frame.
    fn hitloc(&self, id: EntityId) -> Option<Vec3> {
        let body = *self.index.0.get(&id)?;
        let at = self
            .places
            .get(self.attachments.get(body).ok()?.hitloc?)
            .ok()?;
        let at = at.translation();
        Some(Vec3::new(at.x, -at.z, at.y))
    }
}

/// Starts what each unit that has come to wear a worn line's subject wears, and each rune that
/// lies in sight, keeps its particle where it is, and ends it when the unit stops wearing it,
/// falls or leaves the view, or the rune is taken or leaves the sight.
#[allow(clippy::too_many_arguments)]
fn worn(
    session: NonSend<Session>,
    bodies: Bodies,
    ground: Res<Ground>,
    fog: Res<FogGrid>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut worn: ResMut<Worn>,
    mut stage: Stage,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let standing = |id: &EntityId| {
        bodies
            .index
            .0
            .get(id)
            .is_some_and(|entity| !bodies.dying.contains(*entity))
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
    let runes = || crate::runes::shown(view, &fog);
    worn.0.retain(|(id, rule), handle| {
        let fx = rules.iter().find(|(i, _)| i == rule).map(|(_, fx)| fx);
        let keep = fx.is_some_and(|fx| {
            runes().any(|rune| rune.id == *id && lies(fx, rune))
                || standing(id)
                    && view
                        .unit(*id)
                        .is_some_and(|unit| wears(fx, unit, &stage.manifest))
        });
        if !keep && let Some(instance) = handle.and_then(|h| playing.get_mut(h)) {
            instance.end();
        }
        keep
    });
    let units = view
        .units
        .iter()
        .filter(|u| standing(&u.id))
        .map(|u| (u.id, u.pos, u.true_sight_radius.to_f32(), Some(u), None));
    let lying = runes().map(|r| (r.id, r.pos, 0.0, None, Some(r)));
    for (id, pos, sight, unit, rune) in units.chain(lying) {
        let at = on_ground(&ground, pos);
        for (rule, fx) in &rules {
            let holds = match (unit, rune) {
                (Some(unit), _) => wears(fx, unit, &stage.manifest),
                (_, Some(rune)) => lies(fx, rune),
                _ => false,
            };
            if !holds {
                continue;
            }
            let handle = *worn.0.entry((id, *rule)).or_insert_with(|| {
                begin(
                    fx,
                    (id, sight),
                    at,
                    library.as_deref(),
                    &mut playing,
                    &mut stage,
                )
            });
            if let Some(instance) = handle.and_then(|h| playing.get_mut(h)) {
                let place = Vec3::new(at.x, -at.z, at.y);
                instance.set_control_point(PLACE, place, Vec3::ZERO, false);
                if let Some(Feed::Hitloc) = fx.cp1 {
                    let hit = bodies.hitloc(id).unwrap_or(place);
                    instance.set_control_point(FEED, hit, Vec3::ZERO, false);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `fx` line of the manifest reads, names an item or modifier ids.txt knows (a unit
    /// a `unit` line draws, a kind of rune), and names particles and sounds the manifest imports: a typo in a
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
                Some(("rune", art)) => crate::runes::of_art(art).is_some(),
                Some(("hero", name)) => manifest
                    .ids
                    .iter()
                    .any(|(k, _, n)| k == "hero" && n == name),
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
