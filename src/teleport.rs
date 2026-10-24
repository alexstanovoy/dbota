//! Town portal scrolls on the map: the ring where a hero will land, shown to
//! both sides through the fog as Dota does, and the ring at the feet of the
//! hero that channels it while it is in sight.

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use bevy::{
    light::{NotShadowCaster, NotShadowReceiver},
    prelude::*,
};
use bota_proto::{EntityId, SlotId, TeleportView, WorldView};

use crate::manifest::Manifest;
use crate::map::Ground;
use crate::session::Session;
use crate::warmup::Warmup;

/// World units from the centre to the outer edge of the ring.
const RADIUS: f32 = 175.0;
/// World units the ring starts out wider than it ends, and closes in by.
const CLOSING: f32 = 0.6;
/// World units the light column over a landing rises.
const COLUMN: f32 = 520.0;
/// The meshes and the texture the rings share.
#[derive(Resource)]
struct Art {
    ring: Handle<Mesh>,
    disc: Handle<Mesh>,
    column: Handle<Mesh>,
    swirl: Option<Handle<Image>>,
}

/// One end of a teleport: where the hero lands, or where it stands.
#[derive(Component)]
struct Portal {
    materials: [Handle<StandardMaterial>; 3],
}

/// The portal entities by seat and end.
#[derive(Resource, Default)]
struct Portals(HashMap<(SlotId, bool), Entity>);

pub fn plugin(app: &mut App) {
    app.init_resource::<Portals>()
        .add_systems(Startup, make_art)
        .add_systems(Update, portals);
}

fn make_art(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    let art = Art {
        ring: meshes.add(Annulus::new(RADIUS - 22.0, RADIUS)),
        disc: meshes.add(Circle::new(RADIUS)),
        column: meshes.add(Cylinder::new(26.0, COLUMN)),
        swirl: manifest
            .file("particle.ring_wavy")
            .map(|path| server.load(path)),
    };
    // A landing's parts, drawn while loading.
    let parts = [
        (&art.ring, None),
        (&art.disc, art.swirl.clone()),
        (&art.column, None),
    ];
    for (mesh, texture) in parts {
        commands.spawn((
            Warmup,
            Mesh3d(mesh.clone()),
            MeshMaterial3d(materials.add(light(Color::WHITE, texture))),
            NotShadowCaster,
            NotShadowReceiver,
        ));
    }
    commands.insert_resource(art);
}

/// How much of a hero's channel has run, 0 to 1, when it channels a teleport.
pub fn channel_share(view: &WorldView, unit: EntityId) -> Option<f32> {
    let player = view.players.iter().find(|p| p.unit == Some(unit))?;
    let teleport = view.teleports.iter().find(|t| t.slot == player.slot)?;
    Some(progress(teleport))
}

/// How much of a teleport's channel has run, 0 to 1.
pub fn progress(teleport: &TeleportView) -> f32 {
    1.0 - teleport.ticks_left as f32 / teleport.ticks_total.max(1) as f32
}

/// Where the channelling hero of a teleport stands, when it is in sight.
pub fn origin(view: &WorldView, teleport: &TeleportView) -> Option<(f32, f32)> {
    let unit = view.player(teleport.slot)?.unit?;
    let unit = view.unit(unit)?;
    Some((unit.pos.x.to_f32(), unit.pos.y.to_f32()))
}

/// Keeps a ring at the landing of every teleport and at the feet of every
/// hero in sight that channels one.
fn portals(
    mut commands: Commands,
    session: NonSend<Session>,
    art: Option<Res<Art>>,
    ground: Res<Ground>,
    time: Res<Time>,
    colours: Res<crate::player_colour::PlayerColours>,
    mut known: ResMut<Portals>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut rings: Query<(&Portal, &mut Transform)>,
) {
    let (core, Some(art)) = (&session.0, art) else {
        return;
    };
    let Some(view) = core.view() else {
        return;
    };
    let mut wanted = Vec::with_capacity(2 * view.teleports.len());
    for teleport in &view.teleports {
        let landing = (
            teleport.destination.x.to_f32(),
            teleport.destination.y.to_f32(),
        );
        wanted.push((teleport, true, landing));
        if let Some(at) = origin(view, teleport) {
            wanted.push((teleport, false, at));
        }
    }
    let gone: Vec<(SlotId, bool)> = known
        .0
        .keys()
        .filter(|key| {
            !wanted
                .iter()
                .any(|(t, landing, _)| (t.slot, *landing) == **key)
        })
        .copied()
        .collect();
    for key in gone {
        let Some(entity) = known.0.remove(&key) else {
            continue;
        };
        if let Ok((portal, _)) = rings.get(entity) {
            for handle in &portal.materials {
                materials.remove(handle);
            }
        }
        commands.entity(entity).despawn();
    }
    for (teleport, landing, (x, y)) in wanted {
        let key = (teleport.slot, landing);
        let at = Vec3::new(x, ground.at(x, y) + 6.0, -y);
        let share = progress(teleport);
        let entity = *known.0.entry(key).or_insert_with(|| {
            let color = colours.of(view, teleport.slot);
            spawn(&mut commands, &art, &mut materials, landing, color)
        });
        let Ok((portal, mut transform)) = rings.get_mut(entity) else {
            continue;
        };
        let pulse = 1.0 + 0.06 * (time.elapsed_secs() * 7.0).sin();
        let closing = 1.0 + CLOSING * (1.0 - share);
        transform.translation = at;
        transform.scale = Vec3::new(pulse * closing, 1.0, pulse * closing);
        transform.rotation = Quat::from_rotation_y(time.elapsed_secs() * 1.4);
        let [ring, disc, column] = &portal.materials;
        for (handle, alpha) in [
            (ring, 0.4 + 0.6 * share),
            (disc, 0.12 + 0.3 * share),
            (column, 0.05 + 0.5 * share * share),
        ] {
            if let Some(mut material) = materials.get_mut(handle) {
                material.base_color.set_alpha(alpha);
            }
        }
    }
}

/// A glowing, additive material of a colour.
fn light(color: Color, texture: Option<Handle<Image>>) -> StandardMaterial {
    StandardMaterial {
        base_color: color,
        base_color_texture: texture,
        alpha_mode: AlphaMode::Add,
        unlit: true,
        ..default()
    }
}

/// Spawns one end of a teleport: the ring, the swirl inside it, and over a
/// landing the column of light.
fn spawn(
    commands: &mut Commands,
    art: &Art,
    materials: &mut Assets<StandardMaterial>,
    landing: bool,
    color: Color,
) -> Entity {
    let flat = Quat::from_rotation_x(-FRAC_PI_2);
    let handles = [
        materials.add(light(color, None)),
        materials.add(light(color, art.swirl.clone())),
        materials.add(light(color, None)),
    ];
    let parent = commands
        .spawn((
            Portal {
                materials: handles.clone(),
            },
            Transform::default(),
            Visibility::default(),
        ))
        .id();
    let part = |mesh: &Handle<Mesh>, material: &Handle<StandardMaterial>, at: Transform| {
        (
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            at,
            NotShadowCaster,
            NotShadowReceiver,
            ChildOf(parent),
        )
    };
    commands.spawn(part(&art.ring, &handles[0], Transform::from_rotation(flat)));
    commands.spawn(part(
        &art.disc,
        &handles[1],
        Transform::from_xyz(0.0, 2.0, 0.0).with_rotation(flat),
    ));
    if landing {
        commands.spawn(part(
            &art.column,
            &handles[2],
            Transform::from_xyz(0.0, COLUMN / 2.0, 0.0),
        ));
    }
    parent
}
