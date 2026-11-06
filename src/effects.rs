//! What flies and bursts, as Dota's particle effects: attack missiles (each unit's
//! `ProjectileModel`), a Blood Grenade's flight and burst, Shadow Fiend's abilities, a hero's
//! level up and Shadow Fiend's death.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use bota_proto::{EntityId, EventKind};

use crate::ability_art::{Flight, art_by};
use crate::anim::Pose;
use crate::manifest::Manifest;
use crate::map::Ground;
use crate::particles::{EffectSight, Library, Playing};
use crate::session::Session;
use crate::units::{Attachments, Rig, Unit, UnitIndex};

/// How high missiles fly over the ground, in world units.
pub(crate) const MISSILE_HEIGHT: f32 = 110.0;

/// How a missile is drawn: as a particle effect.
#[derive(Clone, Copy)]
enum Body {
    Effect(crate::particles::Handle),
    /// A line of a requiem: its effect flies on its own, along the velocity it started with.
    Line(crate::particles::Handle),
    /// A Blood Grenade: its effect flies where the grenade does and bursts as its end.
    Grenade(crate::particles::Handle),
}

/// The effects of Shadow Fiend's abilities, by their manifest names.
const RAZE: &str = "hero.nevermore.raze";
const REQUIEM: &str = "hero.nevermore.requiem";
const REQUIEM_LINE: &str = "hero.nevermore.requiem_line";
const NECRO_SOUL: &str = "hero.nevermore.necro_soul";
/// A Blood Grenade's flight and burst, and a hero's level up.
const GRENADE: &str = "item.blood_grenade";
const LEVEL_UP: &str = "hero.levelup";
/// Seconds a requiem line's effect may fly: past what a line flies; the line's end ends it.
const LINE_SECONDS: f32 = 4.0;

/// The control points a missile's effect is given, as Dota gives a projectile's: where it
/// left from, where it flies to, and its speed.
const LAUNCH: usize = 0;
const TARGET: usize = 1;
const SPEED: usize = 2;

/// The missiles drawn, by wire handle, and the raze marks already burst.
#[derive(Resource, Default)]
struct Flying {
    missiles: HashMap<EntityId, Body>,
    /// The scorching projectiles already burst, while they are in view.
    burst: HashSet<EntityId>,
    /// The souls flying to the heroes that took them, by the hero.
    souls: Vec<(crate::particles::Handle, EntityId)>,
    /// The effects that follow a unit for their life (a level up), by the unit.
    following: Vec<(crate::particles::Handle, EntityId)>,
}

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Flights;

pub fn plugin(app: &mut App) {
    app.init_resource::<Flying>().add_systems(
        Update,
        (
            missiles.in_set(Flights),
            bursts,
            casts.after(crate::units::Placed),
            souls,
            level_ups.after(crate::units::Placed),
            deaths,
        ),
    );
}

/// What a missile's effect reads of the units: their roots, models, poses and attachments.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct Shooters<'w, 's> {
    sight: crate::fog::EventSight<'w>,
    index: Res<'w, UnitIndex>,
    manifest: Res<'w, Manifest>,
    ground: Res<'w, Ground>,
    units: Query<
        'w,
        's,
        (
            &'static Unit,
            Option<&'static Pose>,
            Option<&'static Attachments>,
        ),
    >,
    places: Query<'w, 's, &'static GlobalTransform>,
    rigs: Query<'w, 's, &'static Rig>,
}

impl Shooters<'_, '_> {
    fn start_body(
        &self,
        f: &bota_client_core::Flight,
        at: Vec3,
        dir: Vec3,
        library: Option<&Library>,
        playing: &mut Playing,
    ) -> Option<Body> {
        let flight = f
            .missile
            .inflictor
            .map_or(Flight::Bolt, |by| art_by(by).flight);
        let line = (flight == Flight::Soul)
            .then(|| start_line(f, at, dir, &self.ground, library, playing))
            .flatten()
            .map(Body::Line);
        let grenade = (flight == Flight::Grenade)
            .then(|| self.start_grenade(f, at, library, playing))
            .flatten()
            .map(Body::Grenade);
        line.or(grenade)
            .or_else(|| self.start(f, at, library, playing).map(Body::Effect))
    }

    /// Where unit `id`'s missile leaves: the hand of the attack it swings.
    pub(crate) fn launch(&self, id: EntityId) -> Option<Vec3> {
        self.sight.visible_unit(id)?;
        let (_, pose, attachments) = self.units.get(*self.index.0.get(&id)?).ok()?;
        let hand = usize::from(pose.is_some_and(|p| p.clip() == "attack2"));
        let attachments = attachments?;
        let at = attachments.attack[hand].or(attachments.attack[0])?;
        Some(self.places.get(at).ok()?.translation())
    }

    /// Where unit `id` is drawn this frame, in the sim's frame.
    pub(crate) fn drawn(&self, id: EntityId) -> Option<Vec2> {
        self.sight.visible_unit(id)?;
        let (unit, _, _) = self.units.get(*self.index.0.get(&id)?).ok()?;
        Some(unit.at)
    }

    /// The elevation of the model actually drawn, shared with picking and bars.
    pub(crate) fn flight_height(&self, id: EntityId) -> f32 {
        if self.sight.visible_unit(id).is_none() {
            return 0.0;
        }
        self.index
            .0
            .get(&id)
            .and_then(|entity| self.units.get(*entity).ok())
            .map_or(0.0, |(unit, _, _)| crate::units::flight_height(unit))
    }

    /// Where the node `name` of unit `id`'s model is.
    pub(crate) fn node(&self, id: EntityId, name: &str) -> Option<Vec3> {
        self.node_frame(id, name).map(|frame| frame.translation())
    }

    /// The drawn attachment's full frame, including its animated orientation.
    pub(crate) fn node_frame(&self, id: EntityId, name: &str) -> Option<GlobalTransform> {
        self.sight.visible_unit(id)?;
        let (_, _, attachments) = self.units.get(*self.index.0.get(&id)?).ok()?;
        let frame = self.places.get(attachments?.node(name)?).ok()?;
        (frame.translation() != Vec3::ZERO).then_some(*frame)
    }

    /// Source's uniform object scale, from the scene actually drawn, before metre conversion.
    pub(crate) fn object_scale(&self, id: EntityId) -> f32 {
        if self.sight.visible_unit(id).is_none() {
            return 1.0;
        }
        self.index
            .0
            .get(&id)
            .and_then(|entity| self.rigs.get(*entity).ok())
            .and_then(|rig| self.places.get(rig.0).ok())
            .map_or(1.0, |frame| {
                frame.to_scale_rotation_translation().0.abs().max_element()
                    / crate::map::UNITS_PER_METRE
            })
    }

    pub(crate) fn facing(&self, id: EntityId) -> Option<f32> {
        self.sight.visible_unit(id)?;
        self.units
            .get(*self.index.0.get(&id)?)
            .ok()
            .map(|(unit, _, _)| unit.facing)
    }

    /// Where a missile hits unit `id`.
    pub(crate) fn hit(&self, id: EntityId) -> Option<Vec3> {
        self.sight.visible_unit(id)?;
        let (_, _, attachments) = self.units.get(*self.index.0.get(&id)?).ok()?;
        let attached = attachments
            .and_then(|attachments| attachments.hitloc)
            .and_then(|node| self.places.get(node).ok())
            .map(GlobalTransform::translation)
            .filter(|place| *place != Vec3::ZERO);
        attached.or_else(|| self.at_flight(id))
    }

    /// Starts the attack effect of missile `f` (an attack's, its look's particle), from its
    /// source's hand, or where it is (`at`) when the source is not seen, to its target.
    fn start(
        &self,
        f: &bota_client_core::Flight,
        at: Vec3,
        library: Option<&Library>,
        playing: &mut Playing,
    ) -> Option<crate::particles::Handle> {
        let model = f.missile.model.filter(|_| f.missile.inflictor.is_none())?;
        let name = crate::units::missile_effect(model, f.missile.team, &self.manifest)?;
        let effect = library?.effect(&name)?;
        let handle = playing.start(
            effect,
            f.missile.id.idx ^ f.missile.id.generation.rotate_left(16),
            EffectSight::Projectile(f.missile.id),
        )?;
        let instance = playing.get_mut(handle)?;
        let launch = f.missile.source.and_then(|s| self.launch(s)).unwrap_or(at);
        instance.set_control_point(LAUNCH, from_world(launch), Vec3::ZERO, true);
        instance.set_control_point(TARGET, from_world(self.aim(f, at)), Vec3::ZERO, true);
        Some(handle)
    }

    /// Keeps a missile's effect flying to its target at its speed (`dir`: its move in a tick).
    fn steer(
        &self,
        f: &bota_client_core::Flight,
        at: Vec3,
        dir: Vec3,
        playing: &mut Playing,
        handle: crate::particles::Handle,
    ) {
        steer(self.aim(f, at), dir, playing, handle);
    }

    /// Starts a Blood Grenade's effect from its thrower's hand (where it is, `at`, when the
    /// thrower is not seen) towards where it is.
    fn start_grenade(
        &self,
        f: &bota_client_core::Flight,
        at: Vec3,
        library: Option<&Library>,
        playing: &mut Playing,
    ) -> Option<crate::particles::Handle> {
        let effect = library?.effect(GRENADE)?;
        let handle = playing.start(
            effect,
            f.missile.id.idx ^ f.missile.id.generation.rotate_left(16),
            EffectSight::Projectile(f.missile.id),
        )?;
        let instance = playing.get_mut(handle)?;
        let launch = f.missile.source.and_then(|s| self.launch(s)).unwrap_or(at);
        instance.set_control_point(LAUNCH, from_world(launch), Vec3::ZERO, true);
        instance.set_control_point(TARGET, from_world(at), Vec3::ZERO, true);
        Some(handle)
    }

    /// Where missile `f` hits: its target's hit location, else where it is now.
    fn aim(&self, f: &bota_client_core::Flight, at: Vec3) -> Vec3 {
        f.missile.target.and_then(|target| {
            self.hit(target).or_else(|| {
                let view = self.sight.core().view()?;
                let loot = view.loot.iter().find(|loot| loot.id == target)?;
                if self.sight.fog().hides(loot.seen_by) { return None; }
                let x = loot.pos.x.to_f32();
                let y = loot.pos.y.to_f32();
                Some(Vec3::new(x, self.ground.at(x, y), -y))
            })
        }).unwrap_or(at)
    }

    /// Unit `id`'s root in the world.
    pub(crate) fn place(&self, id: EntityId) -> Option<Vec3> {
        self.sight.visible_unit(id)?;
        Some(self.places.get(*self.index.0.get(&id)?).ok()?.translation())
    }

    /// Unit `id`'s place at missile height, when it has no attachment for it.
    fn at_flight(&self, id: EntityId) -> Option<Vec3> {
        let at = self.drawn(id)?;
        let height = self.flight_height(id);
        let height = if height > 0.0 { height } else { MISSILE_HEIGHT };
        Some(Vec3::new(at.x, self.ground.at(at.x, at.y) + height, -at.y))
    }
}

/// Keeps every missile in flight drawn where the frame puts it, as its source's particle effect
/// (ended, playing its impact, when the missile goes); a missile with none is not drawn.
fn missiles(
    sight: crate::fog::EventSight,
    ground: Res<Ground>,
    mut flying: ResMut<Flying>,
    shooters: Shooters,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
) {
    if sight.core().view().is_none() {
        return;
    }
    let shown = sight.flights();
    let mut alive = Vec::with_capacity(shown.len());
    for f in &shown {
        let flight = f
            .missile
            .inflictor
            .map_or(Flight::Bolt, |by| art_by(by).flight);
        if flight == Flight::None {
            continue;
        }
        alive.push(f.missile.id);
        let at = Vec3::new(f.x, ground.at(f.x, f.y) + MISSILE_HEIGHT, -f.y);
        let dir = Vec3::new(f.x - f.tail_x, 0.0, -(f.y - f.tail_y));
        let body = match flying.missiles.get(&f.missile.id).copied() {
            Some(body) => body,
            // A line's effect starts with its velocity, known once it has moved a tick.
            None if flight == Flight::Soul
                && dir == Vec3::ZERO
                && library
                    .as_deref()
                    .is_some_and(|l| l.effect(REQUIEM_LINE).is_some()) =>
            {
                continue;
            }
            None => {
                let Some(body) = shooters.start_body(f, at, dir, library.as_deref(), &mut playing)
                else {
                    continue;
                };
                flying.missiles.insert(f.missile.id, body);
                body
            }
        };
        let handle = match body {
            Body::Effect(handle) | Body::Grenade(handle) | Body::Line(handle) => handle,
        };
        playing.note_flight(handle, &f.missile, sight.knowledge(f.missile.seen_by));
        match body {
            Body::Effect(handle) => shooters.steer(f, at, dir, &mut playing, handle),
            // Its effect is pulled to where it will be a tick on.
            Body::Grenade(handle) => steer(at + dir, dir, &mut playing, handle),
            Body::Line(_) => {}
        }
    }
    grenades(
        &ground,
        library.as_deref(),
        &mut playing,
        &sight,
        &mut flying,
    );
    land(&mut flying, &mut alive, &mut playing, &sight);
}

/// A missing grenade can have left vision: its burst requires a visible landing fact.
fn land(
    flying: &mut Flying,
    alive: &mut [EntityId],
    playing: &mut Playing,
    sight: &crate::fog::EventSight,
) {
    // Sorted for the lookups below: thousands of missiles each searched
    // through all the others every frame.
    alive.sort_unstable();
    let is_alive = |id: &EntityId| alive.binary_search(id).is_ok();
    flying.missiles.retain(|id, body| {
        let keep = is_alive(id);
        if !keep {
            match *body {
                // Visible grenade landings were reused above; a missing untold one is hidden.
                Body::Grenade(handle) => playing.cancel(handle),
                Body::Effect(handle) | Body::Line(handle) => playing.finish_flight(handle, sight),
            }
        }
        keep
    });
}

/// A visible landing plays the grenade's end cap at its actual point, even if the flight was hidden.
fn grenades(
    ground: &Ground,
    library: Option<&Library>,
    playing: &mut Playing,
    sight: &crate::fog::EventSight,
    flying: &mut Flying,
) {
    let landed = sight.core().told.iter().filter_map(|event| match event {
        EventKind::Landed {
            inflictor,
            at,
            missile,
            ..
        } if art_by(*inflictor).flight == Flight::Grenade
            && sight.allows(event, crate::fog::EventSubject::Point(*at))
            && missile.is_none_or(|id| !sight.has_projectile(id)) =>
        {
            Some((*missile, *at))
        }
        _ => None,
    });
    for (n, (missile, point)) in landed.enumerate() {
        let (x, y) = (point.x.to_f32(), point.y.to_f32());
        let at = Vec3::new(x, ground.at(x, y), -y);
        // Exact identity prevents a landing from taking another throw by the same caster.
        // A newly visible landing gets a fresh effect, without its private preceding path.
        let held = missile.and_then(|id| match flying.missiles.get(&id).copied() {
            Some(Body::Grenade(handle)) if playing.witnessed_flight(handle, sight) => {
                flying.missiles.remove(&id);
                Some(handle)
            }
            _ => None,
        });
        if let Some(handle) =
            held.or_else(|| play_once(library, playing, GRENADE, at, n as u32, EffectSight::Point))
            && let Some(instance) = playing.get_mut(handle)
        {
            instance.sight = EffectSight::Point;
            instance.set_control_point(0, from_world(at), Vec3::ZERO, true);
            instance.set_control_point(TARGET, from_world(at), Vec3::ZERO, true);
            instance.end();
        }
    }
}

/// Pulls a flying effect to `to` at the speed of a tick's move `dir`.
pub(crate) fn steer(to: Vec3, dir: Vec3, playing: &mut Playing, handle: crate::particles::Handle) {
    let Some(instance) = playing.get_mut(handle) else {
        return;
    };
    instance.set_control_point(TARGET, from_world(to), Vec3::ZERO, false);
    let speed = dir.length() * bota_core::rules::TICKS_PER_SECOND as f32;
    if speed > 0.0 {
        instance.set_control_point(SPEED, Vec3::new(speed, 0.0, 0.0), Vec3::ZERO, false);
    }
}

/// A point of the world in the particles' frame.
fn from_world(v: Vec3) -> Vec3 {
    Vec3::new(v.x, -v.z, v.y)
}

/// Starts the effect of a requiem's line where missile `f` left (a tick back along `dir`, on the
/// ground) with the velocity it flies at.
fn start_line(
    f: &bota_client_core::Flight,
    at: Vec3,
    dir: Vec3,
    ground: &Ground,
    library: Option<&Library>,
    playing: &mut Playing,
) -> Option<crate::particles::Handle> {
    let effect = library?.effect(REQUIEM_LINE)?;
    let handle = playing.start(
        effect,
        f.missile.id.idx ^ f.missile.id.generation.rotate_left(16),
        EffectSight::Projectile(f.missile.id),
    )?;
    let instance = playing.get_mut(handle)?;
    let from = at - dir;
    let start = Vec3::new(from.x, ground.at(from.x, -from.z), from.z);
    let velocity = from_world(dir) * bota_core::rules::TICKS_PER_SECOND as f32;
    instance.set_control_point(0, from_world(start), velocity, true);
    instance.set_control_point(1, velocity, Vec3::ZERO, true);
    instance.set_control_point(2, Vec3::splat(LINE_SECONDS), Vec3::ZERO, true);
    Some(handle)
}

/// Starts effect `name` at `at` (a point of the world on the ground), to play out on its own.
pub(crate) fn play_once(
    library: Option<&Library>,
    playing: &mut Playing,
    name: &str,
    at: Vec3,
    seed: u32,
    sight: EffectSight,
) -> Option<crate::particles::Handle> {
    let handle = playing.start(library?.effect(name)?, seed, sight)?;
    playing
        .get_mut(handle)?
        .set_control_point(0, from_world(at), Vec3::ZERO, true);
    Some(handle)
}

/// A requiem's cast: Dota's burst of souls about its caster as it goes off.
fn casts(
    sight: crate::fog::EventSight,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    shooters: Shooters,
    ground: Res<Ground>,
) {
    for event in &sight.core().told {
        let EventKind::AbilityCast { caster, ability } = event else {
            continue;
        };
        if *ability != bota_proto::AbilityId::REQUIEM {
            continue;
        }
        let Some(unit) = sight.core().view().and_then(|view| view.unit(*caster)) else {
            continue;
        };
        if !sight.allows(event, crate::fog::EventSubject::Unit(unit)) {
            continue;
        }
        if let Some(at) = shooters.place(*caster) {
            let at = Vec3::new(at.x, ground.at(at.x, -at.z), at.z);
            play_once(
                library.as_deref(),
                &mut playing,
                REQUIEM,
                at,
                caster.idx,
                EffectSight::KnownPoint(sight.knowledge(unit.seen_by)),
            );
        }
    }
}

/// Necromastery: a soul flies from each unit Shadow Fiend brings down to him, as Dota shows
/// the soul he gains.
fn souls(
    sight: crate::fog::EventSight,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    shooters: Shooters,
    mut flying: ResMut<Flying>,
) {
    let core = sight.core();
    for (n, event) in core.told.iter().enumerate() {
        let EventKind::Died {
            unit,
            killer: Some(killer),
            denied: false,
            ..
        } = event
        else {
            continue;
        };
        // Necromastery is Shadow Fiend's own.
        let view = core.view();
        if !view
            .and_then(|v| v.unit(*killer))
            .is_some_and(|u| u.hero == Some(bota_proto::HeroId::SHADOW_FIEND))
        {
            continue;
        }
        // The fallen unit may be gone from the scene already: its last place in the view.
        let last = view
            .and_then(|v| v.unit(*unit))
            .or_else(|| core.before_event(n, *unit));
        let Some(victim) = last else {
            continue;
        };
        let before = core.before_event(n, *unit).unwrap_or(victim);
        if !sight.allows(event, crate::fog::EventSubject::Unit(before))
            || !sight.allows(event, crate::fog::EventSubject::Point(victim.pos))
            || sight.visible_unit(*killer).is_none()
        {
            continue;
        }
        let fallen = last.map(|u| {
            let (x, y) = (u.pos.x.to_f32(), u.pos.y.to_f32());
            Vec3::new(x, ground.at(x, y) + MISSILE_HEIGHT, -y)
        });
        let (Some(from), Some(to)) = (shooters.hit(*unit).or(fallen), shooters.hit(*killer)) else {
            continue;
        };
        if let Some(handle) = play_once(
            library.as_deref(),
            &mut playing,
            NECRO_SOUL,
            from,
            unit.idx,
            EffectSight::Witnessed(sight.knowledge(before.seen_by)),
        ) {
            if let Some(instance) = playing.get_mut(handle) {
                instance.set_control_point(1, from_world(to), Vec3::ZERO, true);
            }
            flying.souls.push((handle, *killer));
        }
    }
    // The soul follows its hero while it flies.
    flying.souls.retain(|(handle, hero)| {
        if sight.visible_unit(*hero).is_none() {
            playing.cancel(*handle);
            return false;
        }
        let Some(instance) = playing.get_mut(*handle) else {
            return false;
        };
        if let Some(to) = shooters.hit(*hero) {
            instance.set_control_point(1, from_world(to), Vec3::ZERO, false);
        }
        true
    });
}

/// Sets off Dota's raze, its burst and the scorch it leaves, where each new raze mark appears.
fn bursts(
    session: NonSend<Session>,
    ground: Res<Ground>,
    mut flying: ResMut<Flying>,
    fog: Res<crate::fog::FogGrid>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    // A projectile out of view is forgotten; while in view it bursts once,
    // however many there are.
    let in_view: HashSet<EntityId> = view.projectiles.iter().map(|p| p.id).collect();
    flying.burst.retain(|id| in_view.contains(id));
    for p in &view.projectiles {
        let Some(by) = p.inflictor else {
            continue;
        };
        // What the watched side's fog hides is not drawn bursting.
        if fog.hides(p.seen_by) || !art_by(by).scorches || !flying.burst.insert(p.id) {
            continue;
        }
        let (x, y) = (p.pos.x.to_f32(), p.pos.y.to_f32());
        let at = Vec3::new(x, ground.at(x, y), -y);
        play_once(
            library.as_deref(),
            &mut playing,
            RAZE,
            at,
            p.id.idx,
            EffectSight::Point,
        );
    }
}

/// Dota's level up about a hero whenever it gains a level, following it while it plays.
fn level_ups(
    sight: crate::fog::EventSight,
    index: Res<UnitIndex>,
    drawn: Query<&Unit>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut flying: ResMut<Flying>,
) {
    let core = sight.core();
    let Some(view) = core.view() else {
        return;
    };
    // Where the hero is drawn this frame, so the effect moves with its model.
    let on_ground = |unit: &bota_proto::UnitView| {
        let at = index
            .0
            .get(&unit.id)
            .and_then(|e| drawn.get(*e).ok())
            .map_or(Vec2::new(unit.pos.x.to_f32(), unit.pos.y.to_f32()), |u| {
                u.at
            });
        Vec3::new(at.x, ground.at(at.x, at.y), -at.y)
    };
    for event in &core.told {
        let EventKind::LevelUp { unit, level } = event else {
            continue;
        };
        let Some(hero) = view.unit(*unit) else {
            continue;
        };
        if !sight.allows(event, crate::fog::EventSubject::Unit(hero)) {
            continue;
        }
        let at = on_ground(hero);
        let seed = unit.idx ^ u32::from(*level) << 24;
        if let Some(handle) = play_once(
            library.as_deref(),
            &mut playing,
            LEVEL_UP,
            at,
            seed,
            EffectSight::Unit(*unit),
        ) {
            flying.following.push((handle, *unit));
        }
    }
    // Each follows its hero while it plays and the hero is seen.
    flying.following.retain(|(handle, unit)| {
        let Some(hero) = view.unit(*unit).filter(|hero| sight.allows_unit(hero)) else {
            playing.cancel(*handle);
            return false;
        };
        let Some(instance) = playing.get_mut(*handle) else {
            return false;
        };
        instance.set_control_point(0, from_world(on_ground(hero)), Vec3::ZERO, false);
        true
    });
}

/// A hero's death effect where it falls, when Dota's hero has one (`<hero>.death`, Shadow
/// Fiend's `nevermore_death`).
pub(crate) fn deaths(
    sight: crate::fog::EventSight,
    manifest: Res<Manifest>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
) {
    let core = sight.core();
    for (n, event) in core.told.iter().enumerate() {
        let EventKind::Died { unit, .. } = event else {
            continue;
        };
        let fallen = core
            .view()
            .and_then(|v| v.unit(*unit))
            .or_else(|| core.prev_view().and_then(|v| v.unit(*unit)))
            .or_else(|| core.before_event(n, *unit));
        let Some(hero) = fallen.filter(|u| u.kind == bota_proto::UnitKind::Hero) else {
            continue;
        };
        // Corpse rows have lost their live visibility: keep the pre-death sight and check the spot.
        let before = core.before_event(n, *unit).unwrap_or(hero);
        if !sight.allows(event, crate::fog::EventSubject::Unit(before))
            || !sight.allows(event, crate::fog::EventSubject::Point(hero.pos))
        {
            continue;
        }
        let Some(name) = hero
            .hero
            .and_then(|h| crate::units::hero_effect(h, "death", &manifest))
        else {
            continue;
        };
        let (x, y) = (hero.pos.x.to_f32(), hero.pos.y.to_f32());
        let at = Vec3::new(x, ground.at(x, y), -y);
        if let Some(handle) = play_once(
            library.as_deref(),
            &mut playing,
            &name,
            at,
            unit.idx,
            EffectSight::Witnessed(sight.knowledge(before.seen_by)),
        ) && let Some(instance) = playing.get_mut(handle)
        {
            // Dota's places its parts about control point 1, the hero.
            instance.set_control_point(1, from_world(at), Vec3::ZERO, true);
        }
    }
}
