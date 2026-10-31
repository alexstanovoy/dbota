//! Items lying on the ground, as Dota draws them: the item's own world model when the install has
//! one for it (`Model` in items.txt), Dota's default dropped-item box otherwise, turning in its idle
//! clip where the model has one, and the item's name over the one the cursor is on. They are seen
//! where the fog lets their spot be seen, and a right click on one takes it up (the client core).

use std::collections::HashMap;

use bevy::{prelude::*, world_serialization::WorldInstanceReady};
use bota_client_core::loot_under_cursor;
use bota_proto::{EntityId, LootView};

use crate::camera::CursorGround;
use crate::dota_text::Texts;
use crate::fog::FogGrid;
use crate::manifest::Manifest;
use crate::map::{Ground, UNITS_PER_METRE};
use crate::session::Session;
use crate::tip_content::Lookup;
use crate::ui::{self, Fonts};
use crate::units::ModelBank;

/// Items put on the ground a frame at most: a hero dying with a full bag drops nine.
const SPAWNS_PER_FRAME: usize = 4;
/// The size of an item's name over the cursor, in interface pixels.
const NAME_SIZE: f32 = 18.0;
/// Where the name stands from the cursor, in interface pixels.
const NAME_OFFSET: Vec2 = Vec2::new(18.0, -34.0);

/// An item on the ground.
#[derive(Component, Debug)]
pub struct Loot {
    /// The logical name of the model it is drawn with, when the cache has it.
    pub model: Option<String>,
}

/// The logical name of the model a thing on the ground is drawn with, an item's or a rune's: its
/// idle clip loops once the model is ready ([`play_idle`]).
#[derive(Component, Debug)]
pub struct Idle(pub String);

/// The entity of every item on the ground shown, by its wire handle.
#[derive(Resource, Default)]
struct LootIndex(HashMap<EntityId, Entity>);

/// The name over the hovered item.
#[derive(Component)]
struct ItemName;

pub fn plugin(app: &mut App) {
    app.init_resource::<LootIndex>()
        .add_systems(Startup, make_name)
        .add_systems(Update, (sync_loot, name_hovered));
}

/// The logical name of the model an item lies as: its own where items.txt gives it one (its
/// `ModelAlternate`, `item.<name>_empty.model`, once its charges are spent: the empty Bottle), a
/// recipe's, else Dota's default box.
fn model_name(item: &LootView, manifest: &Manifest) -> String {
    let own = manifest.dota_name("item", item.item.0).map(|name| {
        let empty = format!("item.{name}_empty.model");
        if item.charges == Some(0) && manifest.model(&empty).is_some() {
            empty
        } else {
            format!("item.{name}.model")
        }
    });
    match own {
        Some(name) if manifest.model(&name).is_some() => name,
        _ => "item.default.model".to_owned(),
    }
}

/// Puts each item the view holds on the ground, where it lies, and takes away those that were taken
/// up or left the sight.
fn sync_loot(
    mut commands: Commands,
    session: NonSend<Session>,
    manifest: Res<Manifest>,
    bank: Res<ModelBank>,
    ground: Res<Ground>,
    fog: Res<FogGrid>,
    mut index: ResMut<LootIndex>,
    mut lying: Query<&mut Transform, With<Loot>>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let shown = |l: &&LootView| !fog.hides(l.seen_by);
    let mut spawned = 0;
    for item in view.loot.iter().filter(shown) {
        let (x, y) = (item.pos.x.to_f32(), item.pos.y.to_f32());
        let place = Vec3::new(x, ground.at(x, y), -y);
        if let Some(entity) = index.0.get(&item.id) {
            if let Ok(mut transform) = lying.get_mut(*entity)
                && transform.translation != place
            {
                transform.translation = place;
            }
            continue;
        }
        if spawned >= SPAWNS_PER_FRAME {
            continue;
        }
        let name = model_name(item, &manifest);
        let Some(at) = bank.0.iter().position(|m| m.model.logical == name) else {
            continue;
        };
        let entry = &bank.0[at];
        // Not before the model has loaded whole.
        if !entry.ready() {
            continue;
        }
        let model = entry.scenes.as_ref().map(|scenes| scenes.body.clone());
        let mut root = commands.spawn((
            Loot {
                model: model.is_some().then(|| name.clone()),
            },
            Transform::from_translation(place),
            Visibility::default(),
        ));
        if let Some(body) = model {
            let scale = Transform::from_scale(Vec3::splat(UNITS_PER_METRE * entry.model.scale));
            root.insert(Idle(name.clone()))
                .with_child((WorldAssetRoot(body), scale))
                .observe(play_idle);
        }
        index.0.insert(item.id, root.id());
        spawned += 1;
    }
    let mut here: Vec<EntityId> = view.loot.iter().filter(shown).map(|l| l.id).collect();
    here.sort_unstable();
    index.0.retain(|id, entity| {
        let keep = here.binary_search(id).is_ok();
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
}

/// Gives the model of a thing on the ground (its parent wears [`Idle`]) its idle clip, looping.
pub(crate) fn play_idle(
    ready: On<WorldInstanceReady>,
    mut commands: Commands,
    parents: Query<&ChildOf>,
    idle: Query<&Idle>,
    children: Query<&Children>,
    mut players: Query<&mut AnimationPlayer>,
    bank: Res<ModelBank>,
) {
    let Some(entry) = parents
        .get(ready.entity)
        .ok()
        .and_then(|c| idle.get(c.parent()).ok())
        .and_then(|idle| bank.0.iter().find(|m| m.model.logical == idle.0))
    else {
        return;
    };
    let (Some(graph), Some((_, node, _, _))) = (
        entry.graph.clone(),
        entry.clips.iter().find(|c| c.0 == "idle"),
    ) else {
        return;
    };
    let Some(player) = children
        .iter_descendants(ready.entity)
        .find(|e| players.contains(*e))
    else {
        return;
    };
    commands.entity(player).insert(AnimationGraphHandle(graph));
    if let Ok(mut animation) = players.get_mut(player) {
        animation.play(*node).repeat();
    }
}

fn make_name(mut commands: Commands, fonts: Res<Fonts>) {
    commands.spawn((
        ItemName,
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        ui::text("", &fonts.bold, NAME_SIZE, ui::INK),
        ui::shadow(),
        Visibility::Hidden,
        Pickable::IGNORE,
        GlobalZIndex(30),
    ));
}

/// Shows the name of the item the cursor is on, as Dota does over an item on the ground.
fn name_hovered(
    session: NonSend<Session>,
    cursor: Res<CursorGround>,
    over: Res<crate::input::OverHud>,
    window: Single<&Window, With<bevy::window::PrimaryWindow>>,
    texts: Res<Texts>,
    manifest: Res<Manifest>,
    mut label: Single<(&mut Node, &mut Text, &mut Visibility), With<ItemName>>,
) {
    let view = session.0.view();
    let hovered = view
        .zip(cursor.0)
        .filter(|_| !over.0)
        .and_then(|(view, at)| {
            let id = loot_under_cursor(view, (at.x, -at.z))?;
            view.loot.iter().find(|l| l.id == id)
        });
    let look = Lookup {
        texts: &texts,
        manifest: &manifest,
    };
    let name = hovered.and_then(|l| look.item_name(l.item.0));
    let (Some(name), Some(at)) = (name, window.cursor_position()) else {
        label.2.set_if_neq(Visibility::Hidden);
        return;
    };
    let at = at + NAME_OFFSET;
    label.0.left = px(at.x);
    label.0.top = px(at.y);
    if label.1.0 != name {
        label.1.0 = name;
    }
    label.2.set_if_neq(Visibility::Inherited);
}
