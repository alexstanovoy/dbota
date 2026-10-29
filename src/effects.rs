//! What flies and bursts, as Dota's particle effects: attack missiles (each unit's
//! `ProjectileModel`), a Blood Grenade's flight and burst, Shadow Fiend's abilities, a hero's
//! level up and Shadow Fiend's death.

use std::collections::{HashMap, HashSet};

use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};
use bota_client_core::flights;
use bota_proto::{EntityId, EventKind, Team};

use crate::ability_art::{Flight, art_by};
use crate::anim::Pose;
use crate::manifest::Manifest;
use crate::map::Ground;
use crate::particles::{Library, Playing};
use crate::session::Session;
use crate::units::{Attachments, Unit, UnitIndex};
use crate::warmup::Warmup;

/// How high missiles fly over the ground, in world units.
const FLIGHT: f32 = 110.0;

/// A missile on the map, by its wire handle.
#[derive(Component)]
struct Missile;

/// How a missile is drawn: its source's attack effect, or a plain body.
#[derive(Clone, Copy)]
enum Body {
    Effect(crate::particles::Handle),
    /// A line of a requiem: its effect flies on its own, along the velocity it started with.
    Line(crate::particles::Handle),
    /// A Blood Grenade: its effect flies where the grenade does and bursts as its end.
    Grenade(crate::particles::Handle),
    Bolt(Entity),
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

/// The shared meshes and materials of the effects.
#[derive(Resource)]
struct Art {
    bolt: Handle<Mesh>,
    soul: Handle<Mesh>,
    radiant: Handle<StandardMaterial>,
    dire: Handle<StandardMaterial>,
    soul_mat: Handle<StandardMaterial>,
    grenade: Handle<StandardMaterial>,
}

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
    /// How many grenades whose flight was drawn burst this frame, as their effects' ends.
    burst_grenades: usize,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Flying>()
        .add_systems(Startup, make_art)
        .add_systems(Update, (missiles, bursts, casts, souls, level_ups, deaths));
}

fn glow(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        emissive: color.to_linear() * 3.0,
        unlit: true,
        ..default()
    }
}

fn make_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let art = Art {
        bolt: meshes.add(Sphere::new(14.0)),
        soul: meshes.add(Cuboid::new(16.0, 16.0, 120.0)),
        radiant: materials.add(glow(Color::srgb(0.6, 1.0, 0.5))),
        dire: materials.add(glow(Color::srgb(1.0, 0.45, 0.25))),
        soul_mat: materials.add(glow(Color::srgb(0.75, 0.3, 1.0))),
        grenade: materials.add(glow(Color::srgb(0.75, 0.04, 0.06))),
    };
    // Every kind of plain body, drawn while loading.
    let pieces = [
        (&art.bolt, &art.radiant),
        (&art.bolt, &art.dire),
        (&art.bolt, &art.grenade),
        (&art.soul, &art.soul_mat),
    ];
    for (mesh, material) in pieces {
        commands.spawn((
            Warmup,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
    commands.insert_resource(art);
}

/// What a missile's effect reads of the units: their roots, models, poses and attachments.
#[derive(bevy::ecs::system::SystemParam)]
struct Shooters<'w, 's> {
    index: Res<'w, UnitIndex>,
    manifest: Res<'w, Manifest>,
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
}

impl Shooters<'_, '_> {
    /// Where unit `id`'s missile leaves: the hand of the attack it swings.
    fn launch(&self, id: EntityId) -> Option<Vec3> {
        let (_, pose, attachments) = self.units.get(*self.index.0.get(&id)?).ok()?;
        let hand = usize::from(pose.is_some_and(|p| p.clip() == "attack2"));
        let attachments = attachments?;
        let at = attachments.attack[hand].or(attachments.attack[0])?;
        Some(self.places.get(at).ok()?.translation())
    }

    /// Where a missile hits unit `id`.
    fn hit(&self, id: EntityId) -> Option<Vec3> {
        let (_, _, attachments) = self.units.get(*self.index.0.get(&id)?).ok()?;
        Some(self.places.get(attachments?.hitloc?).ok()?.translation())
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
        )?;
        let instance = playing.get_mut(handle)?;
        let launch = f.missile.source.and_then(|s| self.launch(s)).unwrap_or(at);
        instance.set_control_point(LAUNCH, from_world(launch), Vec3::ZERO, true);
        instance.set_control_point(TARGET, from_world(at), Vec3::ZERO, true);
        Some(handle)
    }

    /// Where missile `f` hits: its target's hit location, else where it is now.
    fn aim(&self, f: &bota_client_core::Flight, at: Vec3) -> Vec3 {
        f.missile
            .target
            .and_then(|t| self.hit(t).or_else(|| self.at_flight(t)))
            .unwrap_or(at)
    }

    /// Unit `id`'s root in the world.
    fn place(&self, id: EntityId) -> Option<Vec3> {
        Some(self.places.get(*self.index.0.get(&id)?).ok()?.translation())
    }

    /// Unit `id`'s place at missile height, when it has no attachment for it.
    fn at_flight(&self, id: EntityId) -> Option<Vec3> {
        let root = *self.index.0.get(&id)?;
        Some(self.places.get(root).ok()?.translation() + Vec3::Y * FLIGHT)
    }
}

/// Keeps every missile in flight drawn where the frame puts it: an attack as its source's
/// particle effect (ended, playing its impact, when the missile goes), anything else as a
/// plain body.
#[allow(clippy::too_many_arguments)]
fn missiles(
    mut commands: Commands,
    session: NonSend<Session>,
    art: Option<Res<Art>>,
    ground: Res<Ground>,
    mut flying: ResMut<Flying>,
    mut transforms: Query<&mut Transform, With<Missile>>,
    shooters: Shooters,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
) {
    let (core, Some(art)) = (&session.0, art) else {
        return;
    };
    let Some(view) = core.view() else {
        return;
    };
    let shown = flights(view, core.prev_view(), core.smooth.alpha());
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
        let at = Vec3::new(f.x, ground.at(f.x, f.y) + FLIGHT, -f.y);
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
                let line = (flight == Flight::Soul)
                    .then(|| start_line(f, at, dir, &ground, library.as_deref(), &mut playing))
                    .flatten()
                    .map(Body::Line);
                let grenade = (flight == Flight::Grenade)
                    .then(|| shooters.start_grenade(f, at, library.as_deref(), &mut playing))
                    .flatten()
                    .map(Body::Grenade);
                let body = match line.or(grenade).or_else(|| {
                    shooters
                        .start(f, at, library.as_deref(), &mut playing)
                        .map(Body::Effect)
                }) {
                    Some(body) => body,
                    None => Body::Bolt(spawn_body(&mut commands, &art, f, flight, at, facing(dir))),
                };
                flying.missiles.insert(f.missile.id, body);
                body
            }
        };
        match body {
            Body::Effect(handle) => shooters.steer(f, at, dir, &mut playing, handle),
            // Its effect is pulled to where it will be a tick on.
            Body::Grenade(handle) => steer(at + dir, dir, &mut playing, handle),
            Body::Line(_) => {}
            Body::Bolt(e) => {
                if let Ok(mut t) = transforms.get_mut(e) {
                    t.translation = at;
                    t.rotation = facing(dir);
                }
            }
        }
    }
    land(&mut flying, &mut alive, &mut commands, &mut playing);
    grenades(
        &mut flying,
        &ground,
        library.as_deref(),
        &mut playing,
        &core.told,
    );
}

/// Ends what draws the missiles that are gone: a body goes, an effect plays its end cap.
fn land(
    flying: &mut Flying,
    alive: &mut [EntityId],
    commands: &mut Commands,
    playing: &mut Playing,
) {
    // Sorted for the lookups below: thousands of missiles each searched
    // through all the others every frame.
    alive.sort_unstable();
    let is_alive = |id: &EntityId| alive.binary_search(id).is_ok();
    let mut burst = 0;
    flying.missiles.retain(|id, body| {
        let keep = is_alive(id);
        if !keep {
            match *body {
                Body::Bolt(e) => commands.entity(e).despawn(),
                Body::Effect(handle) | Body::Line(handle) | Body::Grenade(handle) => {
                    burst += usize::from(matches!(body, Body::Grenade(_)));
                    if let Some(instance) = playing.get_mut(handle) {
                        instance.end();
                    }
                }
            }
        }
        keep
    });
    flying.burst_grenades = burst;
}

/// Bursts each grenade where the server says it landed, which a side is told whenever it sees
/// the spot, whether or not it saw the flight: one whose flight was drawn burst as its effect's
/// end; the others play the effect there, ended at once, its burst alone.
fn grenades(
    flying: &mut Flying,
    ground: &Ground,
    library: Option<&Library>,
    playing: &mut Playing,
    told: &[EventKind],
) {
    let landed = told.iter().filter_map(|event| match event {
        EventKind::Landed { inflictor, at, .. } if art_by(*inflictor).flight == Flight::Grenade => {
            Some((at.x.to_f32(), at.y.to_f32()))
        }
        _ => None,
    });
    for (n, (x, y)) in landed.enumerate() {
        if n < flying.burst_grenades {
            continue;
        }
        let at = Vec3::new(x, ground.at(x, y), -y);
        if let Some(handle) = play_once(library, playing, GRENADE, at, n as u32)
            && let Some(instance) = playing.get_mut(handle)
        {
            instance.set_control_point(TARGET, from_world(at), Vec3::ZERO, true);
            instance.end();
        }
    }
}

/// Pulls a flying effect to `to` at the speed of a tick's move `dir`.
fn steer(to: Vec3, dir: Vec3, playing: &mut Playing, handle: crate::particles::Handle) {
    let Some(instance) = playing.get_mut(handle) else {
        return;
    };
    instance.set_control_point(TARGET, from_world(to), Vec3::ZERO, false);
    let speed = dir.length() * bota_core::rules::TICKS_PER_SECOND as f32;
    if speed > 0.0 {
        instance.set_control_point(SPEED, Vec3::new(speed, 0.0, 0.0), Vec3::ZERO, false);
    }
}

/// The rotation that faces a missile along `dir` (unturned when it has not moved).
fn facing(dir: Vec3) -> Quat {
    if dir.length_squared() > 1e-3 {
        Transform::default()
            .looking_to(dir.normalize(), Vec3::Y)
            .rotation
    } else {
        Quat::IDENTITY
    }
}

/// A plain body for a missile with no effect of its own: a soul, a grenade, or a bolt in its
/// side's colour.
fn spawn_body(
    commands: &mut Commands,
    art: &Art,
    f: &bota_client_core::Flight,
    flight: Flight,
    at: Vec3,
    rotation: Quat,
) -> Entity {
    let (mesh, material) = match flight {
        Flight::Soul => (art.soul.clone(), art.soul_mat.clone()),
        Flight::Grenade => (art.bolt.clone(), art.grenade.clone()),
        _ if f.missile.team == Team::Dire => (art.bolt.clone(), art.dire.clone()),
        _ => (art.bolt.clone(), art.radiant.clone()),
    };
    commands
        .spawn((
            Missile,
            Mesh3d(mesh),
            MeshMaterial3d(material),
            Transform::from_translation(at).with_rotation(rotation),
            NotShadowCaster,
            NotShadowReceiver,
        ))
        .id()
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
) -> Option<crate::particles::Handle> {
    let handle = playing.start(library?.effect(name)?, seed)?;
    playing
        .get_mut(handle)?
        .set_control_point(0, from_world(at), Vec3::ZERO, true);
    Some(handle)
}

/// A requiem's cast: Dota's burst of souls about its caster as it goes off.
fn casts(
    session: NonSend<Session>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    shooters: Shooters,
    ground: Res<Ground>,
) {
    for event in &session.0.told {
        let EventKind::AbilityCast { caster, ability } = event else {
            continue;
        };
        if *ability != bota_proto::AbilityId::REQUIEM {
            continue;
        }
        if let Some(at) = shooters.place(*caster) {
            let at = Vec3::new(at.x, ground.at(at.x, -at.z), at.z);
            play_once(library.as_deref(), &mut playing, REQUIEM, at, caster.idx);
        }
    }
}

/// Necromastery: a soul flies from each unit Shadow Fiend brings down to him, as Dota shows
/// the soul he gains.
fn souls(
    session: NonSend<Session>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    shooters: Shooters,
    mut flying: ResMut<Flying>,
) {
    for event in &session.0.told {
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
        let view = session.0.view();
        if !view
            .and_then(|v| v.unit(*killer))
            .is_some_and(|u| u.hero == Some(bota_proto::HeroId::SHADOW_FIEND))
        {
            continue;
        }
        // The fallen unit may be gone from the scene already: its last place in the view.
        let last = view
            .and_then(|v| v.unit(*unit))
            .or_else(|| session.0.prev_view().and_then(|v| v.unit(*unit)));
        let fallen = last.map(|u| {
            let (x, y) = (u.pos.x.to_f32(), u.pos.y.to_f32());
            Vec3::new(x, ground.at(x, y) + FLIGHT, -y)
        });
        let (Some(from), Some(to)) = (shooters.hit(*unit).or(fallen), shooters.hit(*killer)) else {
            continue;
        };
        if let Some(handle) =
            play_once(library.as_deref(), &mut playing, NECRO_SOUL, from, unit.idx)
        {
            if let Some(instance) = playing.get_mut(handle) {
                instance.set_control_point(1, from_world(to), Vec3::ZERO, true);
            }
            flying.souls.push((handle, *killer));
        }
    }
    // The soul follows its hero while it flies.
    flying.souls.retain(|(handle, hero)| {
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
        play_once(library.as_deref(), &mut playing, RAZE, at, p.id.idx);
    }
}

/// Dota's level up about a hero whenever it gains a level, following it while it plays.
fn level_ups(
    session: NonSend<Session>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
    mut flying: ResMut<Flying>,
) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let on_ground = |unit: &bota_proto::UnitView| {
        let (x, y) = (unit.pos.x.to_f32(), unit.pos.y.to_f32());
        Vec3::new(x, ground.at(x, y), -y)
    };
    for event in &core.told {
        let EventKind::LevelUp { unit, level } = event else {
            continue;
        };
        let Some(hero) = view.unit(*unit) else {
            continue;
        };
        let at = on_ground(hero);
        let seed = unit.idx ^ u32::from(*level) << 24;
        if let Some(handle) = play_once(library.as_deref(), &mut playing, LEVEL_UP, at, seed) {
            flying.following.push((handle, *unit));
        }
    }
    // Each follows its hero while it plays and the hero is seen.
    flying.following.retain(|(handle, unit)| {
        let Some(instance) = playing.get_mut(*handle) else {
            return false;
        };
        if let Some(hero) = view.unit(*unit) {
            instance.set_control_point(0, from_world(on_ground(hero)), Vec3::ZERO, false);
        }
        true
    });
}

/// A hero's death effect where it falls, when Dota's hero has one (`<hero>.death`, Shadow
/// Fiend's `nevermore_death`).
fn deaths(
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    ground: Res<Ground>,
    library: Option<Res<Library>>,
    mut playing: ResMut<Playing>,
) {
    let core = &session.0;
    for event in &core.told {
        let EventKind::Died { unit, .. } = event else {
            continue;
        };
        let fallen = core
            .view()
            .and_then(|v| v.unit(*unit))
            .or_else(|| core.prev_view().and_then(|v| v.unit(*unit)));
        let Some(hero) = fallen.filter(|u| u.kind == bota_proto::UnitKind::Hero) else {
            continue;
        };
        let Some(name) = hero
            .hero
            .and_then(|h| crate::units::hero_effect(h, "death", &manifest))
        else {
            continue;
        };
        let (x, y) = (hero.pos.x.to_f32(), hero.pos.y.to_f32());
        let at = Vec3::new(x, ground.at(x, y), -y);
        if let Some(handle) = play_once(library.as_deref(), &mut playing, &name, at, unit.idx)
            && let Some(instance) = playing.get_mut(handle)
        {
            // Dota's places its parts about control point 1, the hero.
            instance.set_control_point(1, from_world(at), Vec3::ZERO, true);
        }
    }
}
