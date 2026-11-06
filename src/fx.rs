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

use bevy::prelude::*;
use bota_proto::{Attribute, EntityId, ItemId, ItemView, UnitView, Vec2 as Spot};

use crate::camera::RtsCamera;
use crate::effects::Shooters;
use crate::map::Ground;
use crate::mixer::{Owner, Request, Started};
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

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct PresentFacts;

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
pub(crate) fn attach_point(
    ground: &Ground,
    bodies: &Shooters,
    unit: &UnitView,
    attach: &Attach,
) -> Vec3 {
    // Where the unit is drawn this frame, so what it wears moves with its model.
    let feet = match bodies.drawn(unit.id) {
        Some(at) => Vec3::new(
            at.x,
            ground.at(at.x, at.y) + bodies.flight_height(unit.id),
            -at.y,
        ),
        None => on_ground(ground, unit.pos),
    };
    let over = Vec3::Y * crate::bars::bar_height(unit.kind);
    match attach {
        Attach::Origin => feet,
        Attach::Overhead => feet + over,
        Attach::Node(name) => bodies.node(unit.id, name).unwrap_or(feet + over * 0.5),
    }
}

/// The model attachment's Source axes, as well as its place, follow the drawn pose.
fn attach_basis(bodies: &Shooters, unit: &UnitView, attach: &Attach) -> Mat3 {
    if let Attach::Node(name) = attach
        && let Some(frame) = bodies.node_frame(unit.id, name)
    {
        let rotation = frame.to_scale_rotation_translation().1;
        // Only the skeleton root is converted to Bevy's frame. A joint or attachment's
        // local axes are still Source X/Y/Z; converting them as model axes turns twice.
        return Mat3::from_cols(
            place_of(rotation * Vec3::X),
            place_of(rotation * Vec3::Y),
            place_of(rotation * Vec3::Z),
        );
    }
    let facing = bodies
        .facing(unit.id)
        .unwrap_or(f32::from(unit.facing.brads) * std::f32::consts::TAU / 65536.0);
    Mat3::from_rotation_z(facing)
}

/// Whether every node of the model a line puts its effect at, or feeds it, can be told yet: a body
/// whose scene has not been drawn once has none, and an effect started then would jump from the
/// origin to the node and fly off with the speed of the jump.
fn nodes_ready(fx: &Fx, bodies: &Shooters, unit: &UnitView) -> bool {
    if fx
        .feeds
        .iter()
        .any(|(_, feed)| matches!(feed, Feed::ModelRadius))
        && bodies.model_radius(unit.id).is_none()
    {
        return false;
    }
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
    camera: Single<'w, 's, &'static RtsCamera>,
    heard: ResMut<'w, Heard>,
    mixer: ResMut<'w, crate::mixer::Mixer>,
    fog: Res<'w, crate::fog::FogGrid>,
    session: NonSend<'w, crate::session::Session>,
    time: Res<'w, Time>,
    /// The files and events of each prefix of sounds, found once.
    files: Local<'s, HashMap<String, Vec<(String, String)>>>,
}

impl Stage<'_, '_> {
    /// Sounds one of the sounds under `prefix` where `at` is (Bevy frame), faded by its distance
    /// from the camera, as the mixer lets it. A sound that loops sounds until `owner` stops the
    /// voice returned; the sounds counted as heard are those the mixer took.
    fn sound(
        &mut self,
        prefix: &str,
        at: Vec3,
        n: u32,
        owner: Owner,
        emitter: Option<EntityId>,
    ) -> Started {
        let files = self.files_of(prefix);
        let Some((path, event)) = files.get(n as usize % files.len().max(1)).cloned() else {
            return Started::Refused;
        };
        let focus = Vec2::new(self.camera.focus.x, -self.camera.focus.z);
        let place = (at.x, -at.z);
        let own_hero = self.session.0.my_hero();
        // A unit of the watched side always stands in that side's sight: a sound on it is not
        // muted by a fog of war that has not yet caught up with where it stands (a unit that
        // came out of a teleport or a warp this tick).
        let own_side = emitter.is_some_and(|id| {
            self.fog.team.is_some_and(|team| {
                self.session
                    .0
                    .view()
                    .and_then(|view| view.unit(id))
                    .is_some_and(|unit| unit.team == team)
            })
        });
        let request = Request {
            event: if event.is_empty() { prefix } else { &event },
            owner,
            emitter,
            at: Some(place),
            listener: Some((focus.x, focus.y)),
            on_player: emitter.is_some() && emitter == own_hero,
            fog: (!own_side).then_some(&self.fog),
            ..Request::default()
        };
        let handle = self.server.load(path.clone());
        let now = self.time.elapsed();
        let started = self.mixer.start(&mut self.commands, now, &request, handle);
        if started != Started::Refused {
            *self.heard.0.entry(prefix.to_owned()).or_default() += 1;
        }
        started
    }

    /// Whether the sounds under `prefix` loop: one that does is owned, and kept sounding.
    fn loops(&mut self, prefix: &str) -> bool {
        let event = self
            .files_of(prefix)
            .first()
            .map(|(_, event)| event.clone());
        event.is_some_and(|event| self.mixer.loops(&event))
    }

    /// The files of the sounds under `prefix` with the events they play, found once.
    fn files_of(&mut self, prefix: &str) -> &[(String, String)] {
        if !self.files.contains_key(prefix) {
            let found = self.manifest.sounds_with_events(prefix);
            self.files.insert(prefix.to_owned(), found);
        }
        &self.files[prefix]
    }

    /// Moves a looping voice with its source: it is as loud as the camera's distance leaves it.
    fn follow(&mut self, voice: Entity, at: Vec3) {
        let focus = Vec2::new(self.camera.focus.x, -self.camera.focus.z);
        self.mixer.follow(voice, (at.x, -at.z), (focus.x, focus.y));
    }

    /// Stops every loop: no match is on to sound them.
    fn silence_loops(&mut self) {
        self.mixer.stop_loops(&mut self.commands);
    }

    /// Stops a voice that [`Stage::sound`] started, as its event fades out.
    fn silence(&mut self, voice: Entity) {
        let now = self.time.elapsed();
        self.mixer.stop(&mut self.commands, now, voice);
    }

    /// Plays what a line plays at `at`, once: its particle (fed from `place`, the points of the
    /// unit the fact is about) to play out on its own, and its sound, from `source`, the unit it
    /// plays at when it plays at one.
    fn play(
        &mut self,
        fx: &Fx,
        (at, seed): (Vec3, u32),
        (library, playing): (Option<&Library>, &mut Playing),
        (feeds, place): (&Feeds, &dyn Fn(&Attach) -> Vec3),
        sight: crate::particles::EffectSight,
        emitter: Option<EntityId>,
    ) {
        if let Some(name) = &fx.particle {
            let handle = crate::effects::play_once(library, playing, name, at, seed, sight);
            feed(fx, handle, playing, feeds, place);
        }
        if let Some(prefix) = &fx.sound {
            self.sound(prefix, at, seed, Owner::None, emitter);
        }
    }
}

/// What a line can feed its particle from the fact it plays for.
#[derive(Default, Clone, Copy)]
struct Feeds {
    /// The worn unit's imported render-bounds radius.
    model_radius: Option<f32>,
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
    /// The side the unit is held for: 0 the Radiant, 1 the Dire.
    side: f32,
    /// How far the unit sees now, or for the side it is lit for.
    sight: f32,
    /// The share of the unit's countdown run, 0 to 1.
    gathered: f32,
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
            Feed::Stacks => Vec3::new(feeds.stacks, 0.0, 0.0),
            Feed::Seconds => Vec3::new(feeds.seconds, 0.0, 0.0),
            Feed::Mode => Vec3::new(feeds.mode, 0.0, 0.0),
            Feed::Charges => Vec3::new(feeds.charges, 0.0, 0.0),
            Feed::ModelRadius => {
                let Some(radius) = feeds.model_radius.filter(|r| r.is_finite() && *r > 0.0) else {
                    continue;
                };
                Vec3::new(radius, 0.0, 0.0)
            }
            Feed::Vector(vector) => *vector,
            Feed::Place(attach) => place_of(place(attach)),
            Feed::Caster => place_of(feeds.caster.unwrap_or_else(|| place(&Attach::Origin))),
            Feed::Side => Vec3::new(feeds.side, 0.0, 0.0),
            Feed::Sight => Vec3::new(feeds.sight, 0.0, 0.0),
            Feed::Gathered(share, plus) => *share * feeds.gathered + *plus,
        };
        instance.set_control_point(*point, value, Vec3::ZERO, false);
    }
}

#[cfg(test)]
mod tests;
