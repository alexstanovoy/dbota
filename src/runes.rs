//! Runes, as Dota draws them: the model of every rune the watched side sees, turning in its idle
//! clip where it lies (its glow is Dota's particle, bound by the manifest's `fx` lines); what
//! the side remembers of the runes it saw, which the minimap shows until the spot is seen
//! without them; and the names Dota's art gives each kind, which the Bottle's states name.

use std::collections::HashMap;

use bevy::prelude::*;
use bota_proto::{EntityId, EventKind, RuneKind, Team, WorldView};

use crate::fog::FogGrid;
use crate::loot::{Idle, play_idle};
use crate::map::{Ground, UNITS_PER_METRE};
use crate::session::Session;
use crate::units::ModelBank;

/// Runes put on the map a frame at most: the horn brings four.
const SPAWNS_PER_FRAME: usize = 4;
/// Runes remembered at most: a view lists [`bota_proto::MAX_SNAPSHOT_RUNES`], but a spot keeps
/// [`bota_proto::MAX_RUNES_AT_SPOT`] at most and a map has a handful of spots.
pub const REMEMBERED: usize = 64;

/// The name Dota's art gives a kind of rune: its model, glow, sound, Bottle icon and the hero
/// lines about it are named after it (the manifest's catalog reads `rune=` by it).
pub const fn art(kind: RuneKind) -> &'static str {
    dbota_import::catalog::rune_art(kind)
}

/// The name Dota's interface texts give a kind of rune: `DOTA_Chat_Rune_<it>`,
/// `DOTA_HUD_Rune_<it>` and the like.
pub const fn text_name(kind: RuneKind) -> &'static str {
    match kind {
        RuneKind::Water => "Water",
        RuneKind::Bounty => "Bounty",
        RuneKind::Haste => "Haste",
        RuneKind::AmplifyDamage => "DoubleDamage",
        RuneKind::Arcane => "Arcane",
        RuneKind::Regeneration => "Regeneration",
        RuneKind::Illusion => "Illusion",
        RuneKind::Invisibility => "Invisibility",
        RuneKind::Shield => "Shield",
    }
}

/// The kind of rune whose art has a name.
#[cfg(test)]
pub fn of_art(name: &str) -> Option<RuneKind> {
    RuneKind::ALL.into_iter().find(|kind| art(*kind) == name)
}

/// The rune's 32-pixel cell of the minimap sheet (`scripts/mod_textures.txt`,
/// `minimap_rune_*`), its top-left corner in the sheet's pixels.
pub const fn sheet_cell(kind: RuneKind) -> (f32, f32) {
    match kind {
        RuneKind::Invisibility => (448.0, 256.0),
        RuneKind::Haste => (480.0, 256.0),
        RuneKind::Illusion => (448.0, 288.0),
        RuneKind::AmplifyDamage => (480.0, 288.0),
        RuneKind::Regeneration => (448.0, 320.0),
        RuneKind::Bounty => (480.0, 320.0),
        RuneKind::Arcane => (448.0, 352.0),
        RuneKind::Water => (480.0, 448.0),
        // The file says 502; the shield's cell starts at 512.
        RuneKind::Shield => (512.0, 448.0),
    }
}

/// The side of the sheet's rune cells, in its pixels.
pub const SHEET_CELL: f32 = 32.0;

/// A rune on the map; the model it is drawn with, when the cache has it, is its [`Idle`].
#[derive(Component, Debug)]
pub struct RuneBody;

/// The entity of every rune drawn, by its wire handle.
#[derive(Resource, Default)]
struct RuneIndex(HashMap<EntityId, Entity>);

/// A rune as the watched side last saw it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Remembered {
    pub id: EntityId,
    /// Where it lies, in bota's world units.
    pub at: Vec2,
    pub kind: RuneKind,
    /// Whether the side sees it now.
    pub seen: bool,
    /// The tick of the last view that held it.
    pub tick: u32,
}

/// What the watched side remembers of the runes it saw: Dota keeps a rune the fog took on the
/// minimap until its spot is seen empty, or until the river's runes turn over.
#[derive(Resource, Default)]
pub struct RuneMemory {
    /// The vision the memory is of: another side's memory is not this one's.
    vision: Option<(bool, Option<Team>)>,
    /// At most [`REMEMBERED`], the oldest first.
    pub runes: Vec<Remembered>,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<RuneIndex>()
        .init_resource::<RuneMemory>()
        .add_systems(Update, (sync_runes, remember));
}

/// The runes of the view the watched vision sees: all of a side's view, and of a whole view
/// those the vision shown sees.
pub fn shown<'a>(
    view: &'a WorldView,
    fog: &'a FogGrid,
) -> impl Iterator<Item = &'a bota_proto::RuneView> {
    view.runes.iter().filter(|rune| !fog.hides(rune.seen_by))
}

/// Puts each rune the vision sees on the map, where it lies, and takes away those taken,
/// destroyed or gone from sight.
fn sync_runes(
    mut commands: Commands,
    session: NonSend<Session>,
    bank: Res<ModelBank>,
    ground: Res<Ground>,
    fog: Res<FogGrid>,
    mut index: ResMut<RuneIndex>,
) {
    let Some(view) = session.0.view() else {
        return;
    };
    let mut spawned = 0;
    for rune in shown(view, &fog) {
        if index.0.contains_key(&rune.id) || spawned >= SPAWNS_PER_FRAME {
            continue;
        }
        let name = format!("rune.{}.model", art(rune.kind));
        let entry = bank.0.iter().find(|m| m.model.logical == name);
        // Not before the model has loaded whole; without the cache, a rune has only its glow.
        if entry.is_some_and(|entry| !entry.ready()) {
            continue;
        }
        let body = entry.and_then(|entry| {
            let scenes = entry.scenes.as_ref()?;
            Some((scenes.body.clone(), entry.model.scale))
        });
        let (x, y) = (rune.pos.x.to_f32(), rune.pos.y.to_f32());
        let mut root = commands.spawn((
            RuneBody,
            Transform::from_xyz(x, ground.at(x, y), -y),
            Visibility::default(),
        ));
        if let Some((body, scale)) = body {
            let scale = Transform::from_scale(Vec3::splat(UNITS_PER_METRE * scale));
            root.insert(Idle(name))
                .with_child((WorldAssetRoot(body), scale))
                .observe(play_idle);
        }
        index.0.insert(rune.id, root.id());
        spawned += 1;
    }
    index.0.retain(|id, entity| {
        let keep = shown(view, &fog).any(|rune| rune.id == *id);
        if !keep {
            commands.entity(*entity).despawn();
        }
        keep
    });
}

/// Keeps what the watched vision has seen of the runes: a rune in sight is remembered; one
/// remembered is forgotten once its spot is in sight without it (sight worked out for a later
/// view than the last that held it: the fog's cells come a frame behind the view), and a water or power rune
/// once the river's runes turn over (a new one appears there: the old are gone). Another
/// vision to watch through starts from nothing.
pub fn remember(session: NonSend<Session>, fog: Res<FogGrid>, mut memory: ResMut<RuneMemory>) {
    let core = &session.0;
    let Some(view) = core.view() else {
        return;
    };
    let vision = Some((fog.on, fog.team));
    if memory.vision != vision {
        memory.vision = vision;
        memory.runes.clear();
    }
    for rune in &mut memory.runes {
        rune.seen = false;
    }
    for rune in shown(view, &fog) {
        let at = Vec2::new(rune.pos.x.to_f32(), rune.pos.y.to_f32());
        if let Some(known) = memory.runes.iter_mut().find(|r| r.id == rune.id) {
            known.at = at;
            known.seen = true;
            known.tick = view.tick;
        } else if memory.runes.len() < REMEMBERED {
            memory.runes.push(Remembered {
                id: rune.id,
                at,
                kind: rune.kind,
                seen: true,
                tick: view.tick,
            });
        }
    }
    let turned = core.told.iter().any(
        |event| matches!(event, EventKind::RuneSpawned { kind, .. } if *kind != RuneKind::Bounty),
    );
    // The sight worked out for a view after the last that held the rune, which did not.
    // Without fog everything is in sight.
    let sighted = |rune: &Remembered| {
        !fog.on
            || fog.worked_out_at().is_some_and(|tick| tick > rune.tick)
                && fog.lit(rune.at.x, rune.at.y)
    };
    memory
        .runes
        .retain(|rune| rune.seen || !(sighted(rune) || turned && rune.kind != RuneKind::Bounty));
}
