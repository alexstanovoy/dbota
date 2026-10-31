//! Effects bound to the game's facts by data: the `fx` lines of the manifest name a Dota item,
//! ability, modifier or unit and the particle and sound Dota shows with it (grammar in the
//! manifest's header). Four feeds drive them: the events the last pump told (an item used, an
//! ability cast, a throw come down, a blow, a death: `told`), the effects and the units worn from
//! the moment a unit shows them until they go (`worn`), and the missiles in flight (`flies`). A
//! new item, ability or modifier with an effect adds lines there, not code.

mod flies;
pub mod line;
pub mod rules;
mod told;
mod worn;

use std::collections::HashMap;

use bevy::{audio::Volume, prelude::*};
use bota_proto::{Attribute, ItemId, ItemView, UnitView, Vec2 as Spot};

use crate::camera::RtsCamera;
use crate::config::Settings;
use crate::effects::Shooters;
use crate::map::Ground;
use crate::particles::{Handle, Library, Playing};
use line::{Attach, Feed, Fx};

/// The control point a bound particle is placed by.
const PLACE: usize = 0;
/// The item slots that are a hero's inventory; the backpack's three carry nothing active.
const ACTIVE_SLOTS: usize = 6;

/// What the game has sounded through the bound lines, by the logical prefix of the line's sound:
/// for the end-to-end tests to ask what was heard.
#[derive(Resource, Default)]
pub struct Heard(pub HashMap<String, u32>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Heard>().add_plugins((
        rules::plugin,
        told::plugin,
        worn::plugin,
        flies::plugin,
    ));
}

/// A point of the sim as a point of the ground, in the world's frame.
fn on_ground(ground: &Ground, at: Spot) -> Vec3 {
    let (x, y) = (at.x.to_f32(), at.y.to_f32());
    Vec3::new(x, ground.at(x, y), -y)
}

/// Where a line puts its effect on a unit, in the world's frame: at its feet, over its head
/// where its health bar floats, or at a node of its model (half way up where it has none).
fn attach_point(ground: &Ground, bodies: &Shooters, unit: &UnitView, attach: &Attach) -> Vec3 {
    // Where the unit is drawn this frame, so what it wears moves with its model.
    let feet = match bodies.drawn(unit.id) {
        Some(at) => Vec3::new(at.x, ground.at(at.x, at.y), -at.y),
        None => on_ground(ground, unit.pos),
    };
    let over = Vec3::Y * crate::bars::bar_height(unit.kind);
    match attach {
        Attach::Origin => feet,
        Attach::Overhead => feet + over,
        Attach::Node(name) => bodies.node(unit.id, name).unwrap_or(feet + over * 0.5),
    }
}

/// Whether every node of the model a line puts its effect at, or feeds it, can be told yet: a body
/// whose scene has not been drawn once has none, and an effect started then would jump from the
/// origin to the node and fly off with the speed of the jump.
fn nodes_ready(fx: &Fx, bodies: &Shooters, unit: &UnitView) -> bool {
    let feeds = fx.feeds.iter().filter_map(|(_, what)| match what {
        Feed::Place(attach) => Some(attach),
        _ => None,
    });
    std::iter::once(&fx.attach)
        .chain(feeds)
        .all(|attach| match attach {
            Attach::Node(name) => bodies.node(unit.id, name).is_some(),
            _ => true,
        })
}

/// A point of the world's frame as a control point's place (the particles' frame is Z up).
fn place_of(at: Vec3) -> Vec3 {
    Vec3::new(at.x, -at.z, at.y)
}

/// What a bound line plays with: the audio, the camera that fades it, and the counters.
#[derive(bevy::ecs::system::SystemParam)]
struct Stage<'w, 's> {
    commands: Commands<'w, 's>,
    server: Res<'w, AssetServer>,
    manifest: Res<'w, crate::manifest::Manifest>,
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

    /// Plays what a line plays at `at`, once: its particle (fed from `place`, the points of the
    /// unit the fact is about) to play out on its own, and its sound.
    fn play(
        &mut self,
        fx: &Fx,
        (at, seed): (Vec3, u32),
        (library, playing): (Option<&Library>, &mut Playing),
        (feeds, place): (&Feeds, &dyn Fn(&Attach) -> Vec3),
    ) {
        if let Some(name) = &fx.particle {
            let handle = crate::effects::play_once(library, playing, name, at, seed);
            feed(fx, handle, playing, feeds, place);
        }
        if let Some(prefix) = &fx.sound {
            self.sound(prefix, at, seed);
        }
    }
}

/// What a line can feed its particle from the fact it plays for.
#[derive(Default, Clone, Copy)]
struct Feeds {
    /// How far the wearer gives true sight.
    sight: f32,
    /// How many are held of the effect worn.
    stacks: f32,
    /// Seconds the effect worn has left.
    seconds: f32,
    /// The attribute the item concerned is set to.
    mode: f32,
    /// The share of its charges the item concerned held.
    charges: f32,
    /// Where the user stands, in the world.
    caster: Option<Vec3>,
}

/// The item of the id among a unit's active slots, if it has one.
fn carried(unit: Option<&UnitView>, item: ItemId) -> Option<&ItemView> {
    unit?
        .items
        .iter()
        .take(ACTIVE_SLOTS)
        .flatten()
        .find(|i| i.id == item)
}

/// What an item gives a line to feed: the attribute it is set to and the share of its charges it
/// holds (all, for one that has none to spend).
fn item_feeds(item: Option<&ItemView>) -> (f32, f32) {
    let mode = match item.and_then(|i| i.mode) {
        None | Some(Attribute::Strength) => 0.0,
        Some(Attribute::Agility) => 1.0,
        Some(Attribute::Intelligence) => 2.0,
    };
    let charges = item.map_or(0.0, |item| match bota_core::item_def(item.id) {
        Some(def) if def.cast_charges > 0 => {
            f32::from(item.charges.unwrap_or(0)) / f32::from(def.cast_charges)
        }
        _ => 1.0,
    });
    (mode, charges)
}

/// Gives the control points of a line's particle what the line feeds them.
fn feed(
    fx: &Fx,
    handle: Option<Handle>,
    playing: &mut Playing,
    feeds: &Feeds,
    place: &dyn Fn(&Attach) -> Vec3,
) {
    let Some(instance) = handle.and_then(|h| playing.get_mut(h)) else {
        return;
    };
    for (point, what) in &fx.feeds {
        let value = match what {
            Feed::Sight => Vec3::new(feeds.sight, 0.0, 0.0),
            Feed::Stacks => Vec3::new(feeds.stacks, 0.0, 0.0),
            Feed::Seconds => Vec3::new(feeds.seconds, 0.0, 0.0),
            Feed::Mode => Vec3::new(feeds.mode, 0.0, 0.0),
            Feed::Charges => Vec3::new(feeds.charges, 0.0, 0.0),
            Feed::Vector(vector) => *vector,
            Feed::Place(attach) => place_of(place(attach)),
            Feed::Caster => place_of(feeds.caster.unwrap_or_else(|| place(&Attach::Origin))),
        };
        instance.set_control_point(*point, value, Vec3::ZERO, false);
    }
}

#[cfg(test)]
mod tests;
