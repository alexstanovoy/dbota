//! What flies and bursts: attack missiles as Dota's particle effects (each unit's
//! `ProjectileModel`), the souls of a requiem, and the ground a raze burns.

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
/// Seconds a raze's burst lasts.
const BURST: f32 = 0.9;

/// A missile on the map, by its wire handle.
#[derive(Component)]
struct Missile;

/// How a missile is drawn: its source's attack effect, or a plain body.
#[derive(Clone, Copy)]
enum Body {
    Effect(crate::particles::Handle),
    /// A line of a requiem: its effect flies on its own, along the velocity it started with.
    Line(crate::particles::Handle),
    Bolt(Entity),
}

/// The effects of Shadow Fiend's abilities, by their manifest names.
const RAZE: &str = "hero.nevermore.raze";
const REQUIEM: &str = "hero.nevermore.requiem";
const REQUIEM_LINE: &str = "hero.nevermore.requiem_line";
const NECRO_SOUL: &str = "hero.nevermore.necro_soul";
/// Seconds a requiem line's effect may fly: past what a line flies; the line's end ends it.
const LINE_SECONDS: f32 = 4.0;

/// The control points a missile's effect is given, as Dota gives a projectile's: where it
/// left from, where it flies to, and its speed.
const LAUNCH: usize = 0;
const TARGET: usize = 1;
const SPEED: usize = 2;

/// A burst on the ground, aging to its end.
#[derive(Component)]
struct Burst {
    age: f32,
    material: Handle<StandardMaterial>,
    /// Its full size against a raze's.
    size: f32,
}

/// How wide a raze burns, in world units.
const RAZE_RADIUS: f32 = bota_core::rules::RAZE_RADIUS as f32;
/// How wide the ring of a raze's burst is.
const RAZE_RING_WIDTH: f32 = 30.0;
/// A grenade's burst against a raze's.
const GRENADE_SIZE: f32 = bota_core::rules::GRENADE_RADIUS as f32 / RAZE_RADIUS;

/// The shared meshes and materials of the effects.
#[derive(Resource)]
struct Art {
    bolt: Handle<Mesh>,
    soul: Handle<Mesh>,
    ring: Handle<Mesh>,
    disc: Handle<Mesh>,
    radiant: Handle<StandardMaterial>,
    dire: Handle<StandardMaterial>,
    soul_mat: Handle<StandardMaterial>,
    grenade: Handle<StandardMaterial>,
    cracks: Option<Handle<Image>>,
}

/// The missiles drawn, by wire handle, and the raze marks already burst.
#[derive(Resource, Default)]
struct Flying {
    missiles: HashMap<EntityId, Body>,
    /// The scorching projectiles already burst, while they are in view.
    burst: HashSet<EntityId>,
    /// The souls flying to the heroes that took them, by the hero.
    souls: Vec<(crate::particles::Handle, EntityId)>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Flying>()
        .add_systems(Startup, make_art)
        .add_systems(
            Update,
            (missiles, bursts, casts, souls, age_bursts, level_ups),
        );
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
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    let art = Art {
        bolt: meshes.add(Sphere::new(14.0)),
        soul: meshes.add(Cuboid::new(16.0, 16.0, 120.0)),
        ring: meshes.add(Annulus::new(RAZE_RADIUS - RAZE_RING_WIDTH, RAZE_RADIUS)),
        disc: meshes.add(Circle::new(RAZE_RADIUS)),
        radiant: materials.add(glow(Color::srgb(0.6, 1.0, 0.5))),
        dire: materials.add(glow(Color::srgb(1.0, 0.45, 0.25))),
        soul_mat: materials.add(glow(Color::srgb(0.75, 0.3, 1.0))),
        grenade: materials.add(glow(Color::srgb(0.75, 0.04, 0.06))),
        cracks: manifest
            .file("particle.ground_cracks")
            .map(|p| server.load(p)),
    };
    // Every kind of missile and burst, drawn while loading.
    let (color, glow) = (Color::WHITE, LinearRgba::WHITE);
    let pieces = [
        (&art.bolt, art.radiant.clone()),
        (&art.bolt, art.dire.clone()),
        (&art.bolt, art.grenade.clone()),
        (&art.soul, art.soul_mat.clone()),
        (&art.disc, materials.add(scorch(&art, color, glow))),
        (&art.ring, materials.add(flash(color))),
    ];
    for (mesh, material) in pieces {
        commands.spawn((
            Warmup,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material),
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
        let Some(instance) = playing.get_mut(handle) else {
            return;
        };
        instance.set_control_point(TARGET, from_world(self.aim(f, at)), Vec3::ZERO, false);
        let speed = dir.length() * bota_core::rules::TICKS_PER_SECOND as f32;
        if speed > 0.0 {
            instance.set_control_point(SPEED, Vec3::new(speed, 0.0, 0.0), Vec3::ZERO, false);
        }
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
    mut materials: ResMut<Assets<StandardMaterial>>,
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
                    .flatten();
                let body = match line.map(Body::Line).or_else(|| {
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
    grenades(&mut commands, &art, &mut materials, &ground, &core.told);
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
    flying.missiles.retain(|id, body| {
        let keep = is_alive(id);
        if !keep {
            match *body {
                Body::Bolt(e) => commands.entity(e).despawn(),
                Body::Effect(handle) | Body::Line(handle) => {
                    if let Some(instance) = playing.get_mut(handle) {
                        instance.end();
                    }
                }
            }
        }
        keep
    });
}

/// Bursts each grenade where the server says it landed, which a side is told whenever it sees
/// the spot, whether or not it saw the flight.
fn grenades(
    commands: &mut Commands,
    art: &Art,
    materials: &mut Assets<StandardMaterial>,
    ground: &Ground,
    told: &[EventKind],
) {
    // A grenade bursts where the server says it landed, which a side is
    // told whenever it sees the spot, whether or not it saw the flight.
    let landed = told.iter().filter_map(|event| match event {
        EventKind::Landed { inflictor, at, .. } if art_by(*inflictor).flight == Flight::Grenade => {
            Some((at.x.to_f32(), at.y.to_f32()))
        }
        _ => None,
    });
    for at in landed {
        burst(
            commands,
            art,
            materials,
            ground,
            at,
            GRENADE_SIZE,
            (
                Color::srgba(0.55, 0.02, 0.04, 0.9),
                LinearRgba::rgb(1.2, 0.05, 0.08),
            ),
        );
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
fn play_once(
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

/// Scorches the ground at a spot and rings it with a flash, `size` times a
/// raze's, in a colour.
fn burst(
    commands: &mut Commands,
    art: &Art,
    materials: &mut Assets<StandardMaterial>,
    ground: &Ground,
    (x, y): (f32, f32),
    size: f32,
    (color, glow): (Color, LinearRgba),
) {
    let flat = Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
    let base = Vec3::new(x, ground.at(x, y) + 6.0, -y);
    let scorch = materials.add(scorch(art, color, glow));
    let flash = materials.add(flash(color));
    commands.spawn((
        Burst {
            age: 0.0,
            material: scorch.clone(),
            size,
        },
        Mesh3d(art.disc.clone()),
        MeshMaterial3d(scorch),
        Transform::from_translation(base).with_rotation(flat),
        NotShadowCaster,
        NotShadowReceiver,
    ));
    commands.spawn((
        Burst {
            age: 0.0,
            material: flash.clone(),
            size,
        },
        Mesh3d(art.ring.clone()),
        MeshMaterial3d(flash),
        Transform::from_translation(base + Vec3::Y * 4.0).with_rotation(flat),
        NotShadowCaster,
        NotShadowReceiver,
    ));
}

/// The scorched ground of a burst: the cracks in a colour, glowing.
fn scorch(art: &Art, color: Color, glow: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        base_color_texture: art.cracks.clone(),
        emissive: glow,
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        ..default()
    }
}

/// The flash that rings a burst.
fn flash(color: Color) -> StandardMaterial {
    StandardMaterial {
        base_color: color.with_alpha(0.55),
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    }
}

/// A golden ring at a hero's feet when it gains a level.
fn level_ups(
    mut commands: Commands,
    session: NonSend<Session>,
    art: Option<Res<Art>>,
    ground: Res<Ground>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut levels: Local<HashMap<EntityId, u8>>,
) {
    let (core, Some(art)) = (&session.0, art) else {
        return;
    };
    let Some(view) = core.view() else {
        return;
    };
    levels.retain(|id, _| view.unit(*id).is_some());
    for u in view
        .units
        .iter()
        .filter(|u| u.kind == bota_proto::UnitKind::Hero)
    {
        let before = levels.insert(u.id, u.level);
        if before.is_some_and(|b| u.level > b) {
            burst(
                &mut commands,
                &art,
                &mut materials,
                &ground,
                (u.pos.x.to_f32(), u.pos.y.to_f32()),
                0.5,
                (
                    Color::srgba(1.0, 0.8, 0.3, 0.9),
                    LinearRgba::rgb(2.0, 1.5, 0.4),
                ),
            );
        }
    }
}

/// Ages every burst: the ring grows and every part fades away.
fn age_bursts(
    mut commands: Commands,
    time: Res<Time>,
    mut bursts: Query<(Entity, &mut Burst, &mut Transform)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, mut b, mut t) in &mut bursts {
        b.age += time.delta_secs();
        let k = (b.age / BURST).min(1.0);
        if k >= 1.0 {
            materials.remove(&b.material);
            commands.entity(e).despawn();
            continue;
        }
        t.scale = Vec3::splat(b.size * (0.6 + 0.4 * (1.0 - (1.0 - k) * (1.0 - k))));
        if let Some(mut m) = materials.get_mut(&b.material) {
            let a = (1.0 - k) * (1.0 - k);
            m.base_color.set_alpha(a);
        }
    }
}
