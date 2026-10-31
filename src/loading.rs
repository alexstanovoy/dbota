//! The loading phase, as Dota's: when a match connects (a game opened from
//! the menu, the command line's game, a replay) the client puts up the
//! loading screen and loads what the match draws and sounds behind it: the
//! world (the map's ground, its trees and buildings), every unit model, the
//! particles and their models, the sounds, the minimap, and the render
//! pipelines of all of it. The game shows once all of that is resident and,
//! when the match has begun, its world is built (the forest planted, the
//! heroes on the map), so its first frames load nothing.
//!
//! Each part reports how far it is in [`Parts`]; the loaders of the parts
//! run when [`Loading::requested`] says so, once, at the first match.
//! A per-match world (a map named by the match) is loaded here too.

use std::collections::BTreeMap;

use bevy::prelude::*;

use crate::session::Session;

mod screen;

/// Where the loading stands.
#[derive(Resource, Default)]
pub struct Loading {
    /// The game the loading is for (`GameEpoch`), once one connected.
    game: Option<u32>,
    /// Whether the asset loaders run this frame: the frame the first match
    /// connected.
    requested: bool,
    /// Whether the loaders have run.
    asked: bool,
    /// Whether the loading screen covers the game.
    pub shown: bool,
    /// Done parts over all parts, 0 to 1.
    pub progress: f32,
    /// Seconds since the start when the loading began and ended.
    began: f32,
    ended: Option<f32>,
    /// Whether the match's world stood once: from then on the game stays on
    /// screen whatever comes and goes.
    world_stood: bool,
}

impl Loading {
    /// Whether everything the game needs is resident and its world built:
    /// the game is on screen.
    pub fn done(&self) -> bool {
        self.game.is_some() && !self.shown
    }
}

/// Each part of the loading: how many of its pieces are resident, of how
/// many.
#[derive(Resource, Default)]
pub struct Parts(pub BTreeMap<&'static str, (usize, usize)>);

impl Parts {
    /// Says how far a part is.
    pub fn set(&mut self, part: &'static str, done: usize, total: usize) {
        let entry = (done.min(total), total);
        if self.0.get(part) != Some(&entry) {
            self.0.insert(part, entry);
        }
    }

    fn complete(&self) -> bool {
        self.0.values().all(|(done, total)| done >= total)
    }

    fn share(&self) -> f32 {
        let (done, total) = self
            .0
            .values()
            .fold((0, 0), |(d, t), (done, total)| (d + done, t + total));
        if total == 0 {
            1.0
        } else {
            done as f32 / total as f32
        }
    }
}

/// The systems that say how far their part is, before the loading is worked
/// out.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct Report;

/// Seconds the loading screen waits at most: a part that never finishes (a
/// file the cache lacks that never fails either) is named on the log and the
/// game goes on without it, as it would have drawn without it before.
const PATIENCE: f32 = 120.0;

/// Whether the asset server is done with a handle: loaded with everything it
/// needs, or failed (nothing to wait for).
pub fn settled<A: Asset>(server: &AssetServer, handle: &Handle<A>) -> bool {
    match server.get_load_state(handle) {
        // Made in the client, not read: resident.
        None => true,
        Some(state) if state.is_failed() => true,
        Some(_) => server.is_loaded_with_dependencies(handle),
    }
}

/// The run condition of the asset loaders: the frame the first match
/// connected.
pub fn requested(loading: Res<Loading>) -> bool {
    loading.requested
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Loading>()
        .init_resource::<Parts>()
        .add_systems(First, begin)
        .add_systems(PostUpdate, track.after(Report))
        .add_systems(Update, slow_load.run_if(requested))
        .add_systems(Last, |mut loading: ResMut<Loading>| {
            if loading.requested {
                loading.requested = false;
            }
        });
    screen::plugin(app);
}

/// Debug `--slow-load`: the main thread held, as a slow machine's loading may
/// hold it. The connection's reading, its answers to the server's pings and
/// its redialing live on threads of their own, which this does not stop.
fn slow_load(args: Res<crate::Args>) {
    if let Some(seconds) = args.slow_load {
        std::thread::sleep(std::time::Duration::from_secs_f32(seconds.clamp(0.0, 60.0)));
    }
}

/// Starts the loading when a match connects: the game the command line
/// names at once, a game from the menu when the menu opens it.
fn begin(
    args: Res<crate::Args>,
    epoch: Res<crate::main_menu::GameEpoch>,
    mut loading: ResMut<Loading>,
) {
    let connected = epoch.0 > 0 || !args.wants_menu();
    if !connected || loading.game == Some(epoch.0) {
        return;
    }
    loading.game = Some(epoch.0);
    loading.shown = true;
    loading.began = crate::since_start();
    loading.ended = None;
    loading.world_stood = false;
    if !loading.asked {
        loading.asked = true;
        loading.requested = true;
    }
    info!("loading: began for game {}", epoch.0);
}

/// Works out how far the loading is and lifts the screen once every part is
/// resident and, in a match under way, its world stands.
fn track(
    session: NonSend<Session>,
    parts: Res<Parts>,
    ready: Res<crate::trees::SceneryReady>,
    photo: Res<crate::minimap::PhotoTaken>,
    units: Res<crate::units::UnitIndex>,
    fog: Res<crate::fog::FogGrid>,
    mut loading: ResMut<Loading>,
) {
    if loading.game.is_none() || loading.world_stood {
        return;
    }
    let core = &session.0;
    let world = match core.view() {
        // Before the match: what it will draw.
        None => true,
        Some(view) => {
            ready.0
                && photo.0
                && view
                    .units
                    .iter()
                    .filter(|u| u.kind == bota_proto::UnitKind::Hero && !fog.hides(u.seen_by))
                    .all(|u| units.0.contains_key(&u.id))
        }
    };
    let progress = parts.share();
    if loading.progress != progress {
        loading.progress = progress;
    }
    let waited = crate::since_start() - loading.began > PATIENCE;
    if waited && loading.ended.is_none() {
        let left: Vec<_> = parts.0.iter().filter(|(_, (d, t))| d < t).collect();
        warn!(
            "loading: still waiting after {PATIENCE} s: {left:?}, world {world}: the game goes on"
        );
    }
    let shown = !(waited || parts.complete() && world);
    if loading.shown != shown {
        loading.shown = shown;
    }
    if !shown && core.view().is_some() {
        loading.world_stood = true;
    }
    if !shown && loading.ended.is_none() {
        let now = crate::since_start();
        loading.ended = Some(now);
        info!(
            "loading: done in {:.2} s ({} parts)",
            now - loading.began,
            parts.0.len()
        );
    }
}
