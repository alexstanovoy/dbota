//! The loading phase, as Dota's: a match connected puts the loading screen up, everything the game
//! draws and sounds loads behind it, and the game shows only once that is resident and its world
//! built: the first frame of the game has the forest and the heroes, and the frames after it load
//! no file.

use bevy::asset::AssetEvent;
use bevy::ecs::message::Messages;
use bevy::prelude::*;
use bota_client_core::Intent;
use bota_proto::{HeroId, UnitKind};

use super::harness::{Client, Server};
use crate::loading::{Loading, Parts};

/// Frames after the game shows in which no file may finish loading.
const QUIET_FRAMES: usize = 90;

/// Whether `entity` or anything under it is a mesh drawn.
fn has_mesh(world: &World, entity: Entity) -> bool {
    if world.get::<Mesh3d>(entity).is_some() {
        return true;
    }
    world
        .get::<Children>(entity)
        .is_some_and(|children| children.iter().any(|child| has_mesh(world, child)))
}

/// The files that finished loading since the cursors last read, by kind.
struct Loads {
    images: bevy::ecs::message::MessageCursor<AssetEvent<Image>>,
    meshes: bevy::ecs::message::MessageCursor<AssetEvent<Mesh>>,
    gltfs: bevy::ecs::message::MessageCursor<AssetEvent<bevy::gltf::Gltf>>,
    sounds: bevy::ecs::message::MessageCursor<AssetEvent<AudioSource>>,
}

impl Loads {
    /// Cursors past everything sent so far.
    fn now(world: &World) -> Loads {
        Loads {
            images: world
                .resource::<Messages<AssetEvent<Image>>>()
                .get_cursor_current(),
            meshes: world
                .resource::<Messages<AssetEvent<Mesh>>>()
                .get_cursor_current(),
            gltfs: world
                .resource::<Messages<AssetEvent<bevy::gltf::Gltf>>>()
                .get_cursor_current(),
            sounds: world
                .resource::<Messages<AssetEvent<AudioSource>>>()
                .get_cursor_current(),
        }
    }

    fn read(&mut self, world: &World) -> usize {
        fn count<A: Asset>(
            world: &World,
            cursor: &mut bevy::ecs::message::MessageCursor<AssetEvent<A>>,
        ) -> usize {
            let messages = world.resource::<Messages<AssetEvent<A>>>();
            cursor
                .read(messages)
                .filter(|e| matches!(e, AssetEvent::LoadedWithDependencies { .. }))
                .count()
        }
        count(world, &mut self.images)
            + count(world, &mut self.meshes)
            + count(world, &mut self.gltfs)
            + count(world, &mut self.sounds)
    }
}

/// Connecting puts the loading screen up; the first frame of the game after the strategy phase
/// comes with every part resident, the forest standing and every hero drawn with its model; and
/// no file finishes loading in the frames after it.
#[test]
fn a_match_loads_behind_the_loading_screen_and_its_game_starts_whole() {
    let server = Server::start(1, &["--tactical-secs", "5"]);
    let mut client = Client::run(&["--addr", server.address.as_str()]);
    client.frames(2);
    assert!(
        client.world().resource::<Loading>().shown,
        "the loading screen is up once the match connects"
    );
    client.until("the game on screen", |c| {
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        c.core().view().is_some() && c.world().resource::<Loading>().done()
    });
    let world = client.world();
    let left: Vec<_> = world
        .resource::<Parts>()
        .0
        .iter()
        .filter(|(_, (d, t))| d < t)
        .collect();
    assert!(
        left.is_empty(),
        "every part resident in the first frame of the game: {left:?}"
    );
    for part in [
        "world",
        "models",
        "trees",
        "particles",
        "sounds",
        "voices",
        "pipelines",
    ] {
        assert!(
            world.resource::<Parts>().0.contains_key(part),
            "{part} loaded behind the screen"
        );
    }
    assert!(
        world.resource::<crate::trees::SceneryReady>().0,
        "the forest stands"
    );
    assert_eq!(
        crate::trees::drawn(world).len(),
        client.core().trees().len(),
        "every tree drawn"
    );
    let heroes: Vec<_> = client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Hero)
        .map(|u| u.id)
        .collect();
    assert!(!heroes.is_empty(), "the view holds the heroes");
    for id in heroes {
        let body = client.body(id);
        assert!(
            body.is_some_and(|body| has_mesh(client.world(), body)),
            "hero {id:?} drawn with its model in the first frame of the game"
        );
    }
    let mut loads = Loads::now(client.world());
    let mut loaded = 0;
    for _ in 0..QUIET_FRAMES {
        client.frames(1);
        loaded += loads.read(client.world());
    }
    assert_eq!(
        loaded, 0,
        "no file loads in the first {QUIET_FRAMES} frames of the game"
    );
}

/// A loading that holds the client's main thread longer than the server waits for a silent client
/// (`--idle-timeout-secs`) keeps the connection: the pings are answered on the connection's own
/// thread, and the client goes on into the match.
#[test]
fn a_slow_loading_keeps_the_connection_and_enters_the_match() {
    let server = Server::start(1, &["--idle-timeout-secs", "2", "--tactical-secs", "3"]);
    let mut client = Client::run(&["--addr", server.address.as_str(), "--slow-load", "5"]);
    client.until("the game on screen", |c| {
        assert!(!c.core().lost, "the connection was lost while loading");
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        c.core().view().is_some() && c.world().resource::<Loading>().done()
    });
    client.frames(30);
    assert!(!client.core().lost, "still connected in the match");
}
