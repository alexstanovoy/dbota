//! The map's shop keepers, standing where Dota's map puts them and playing their idle loops: the
//! importer leaves them out of the map's static mesh (where they stood in the pose of their
//! model) and lists their places in `buildings.txt`; each is drawn with the manifest's
//! `prop.<model of the map>.model` (`.skin<n>.model` for a skin the map draws it with).

use bevy::prelude::*;
use bevy::world_serialization::WorldInstanceReady;
use dbota_import::import::MAP_PROPS;

use crate::fog::Fogged;
use crate::loot::{Idle, spawn_idle_model};
use crate::map::Ground;
use crate::session::MatchKnown;
use crate::units::{BuildingYaws, MapBuilding, ModelBank, map_scale};

pub fn plugin(app: &mut App) {
    app.add_systems(Update, stand);
}

/// A model the map places, standing at its place.
#[derive(Component)]
pub struct MapProp;

/// Where the props are: not looked at until the match is, then once.
#[derive(Default)]
enum Standing {
    #[default]
    Waiting,
    Done,
}

/// The logical name of the model the map's `stem` is drawn with in a skin.
fn prop_name(stem: &str, skin: u8) -> String {
    if skin == 0 {
        format!("prop.{stem}.model")
    } else {
        format!("prop.{stem}.skin{skin}.model")
    }
}

/// The bank's place of the model a placed prop is drawn with: its skin's, else the plain one.
fn model_of(bank: &ModelBank, building: &MapBuilding) -> Option<usize> {
    let named = |skin| {
        let name = prop_name(&building.model, skin);
        bank.0.iter().position(|m| m.model.logical == name)
    };
    named(building.skin).or_else(|| named(0))
}

/// Stands the map's props once the match is known, the models loaded and the ground read, and
/// takes them away when the match is gone.
fn stand(
    mut commands: Commands,
    known: Res<MatchKnown>,
    yaws: Res<BuildingYaws>,
    bank: Res<ModelBank>,
    ground: Res<Ground>,
    props: Query<Entity, With<MapProp>>,
    mut state: Local<Standing>,
) {
    if !known.0 {
        for prop in &props {
            commands.entity(prop).despawn();
        }
        *state = Standing::Waiting;
        return;
    }
    if matches!(*state, Standing::Done) || !ground.known() || bank.0.is_empty() {
        return;
    }
    let placed: Vec<(&MapBuilding, usize)> = yaws
        .0
        .iter()
        .filter(|b| MAP_PROPS.iter().any(|prop| b.model.starts_with(prop)))
        .filter_map(|b| {
            let model = model_of(&bank, b);
            if model.is_none() {
                warn!(
                    "the map places a {} but the cache has no prop model for it",
                    b.model
                );
            }
            Some((b, model?))
        })
        .collect();
    // All at once, once every model has loaded whole.
    if placed
        .iter()
        .any(|(_, model)| !bank.0[*model].ready() || bank.0[*model].scenes.is_none())
    {
        return;
    }
    *state = Standing::Done;
    for (building, model) in placed {
        let entry = &bank.0[model];
        let Some(scenes) = entry.scenes.as_ref() else {
            continue;
        };
        let (x, y) = (building.at.x, building.at.y);
        let prop = commands
            .spawn((
                MapProp,
                Idle(entry.model.logical.clone()),
                Transform::from_xyz(x, ground.at(x, y), -y)
                    .with_rotation(crate::coords::sim_facing_to_rotation(building.yaw)),
                Visibility::default(),
            ))
            .id();
        let scene = spawn_idle_model(
            &mut commands,
            prop,
            scenes.body.clone(),
            map_scale(building.scale, entry.model.scale),
        );
        commands.entity(scene).observe(fog_prop);
    }
}

/// Lets the fog darken a prop's meshes as it does the ground's.
fn fog_prop(ready: On<WorldInstanceReady>, mut commands: Commands, children: Query<&Children>) {
    for part in children.iter_descendants(ready.entity) {
        commands.entity(part).insert(Fogged);
    }
}
