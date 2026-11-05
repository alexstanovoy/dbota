//! The viewer's fog, the image drawn over the minimap, and private map effects in a real match.

use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{Cheat, HeroId, Team, UnitKind};

use super::harness::{Client, Server, quiet_seat};
use crate::fog::{FogGrid, VeilImage};

pub(super) fn veil_alpha(client: &Client, at: Vec2) -> u8 {
    let grid = client.world().resource::<FogGrid>();
    let image = &client.world().resource::<VeilImage>().0;
    let image = client
        .world()
        .resource::<Assets<Image>>()
        .get(image)
        .expect("the fog image");
    let x = (at.x / crate::terrain::CELL) as usize;
    let y = grid.n - 1 - (at.y / crate::terrain::CELL) as usize;
    image.data.as_ref().expect("fog texels")[(y * grid.n + x) * 4 + 3]
}

#[test]
fn fog_reveals_our_hero_creeps_and_towers_and_masks_enemy_towers_on_the_minimap() {
    for map in ["0", "2"] {
        for dire in [false, true] {
            check_side_vision(map, dire);
        }
    }
}

fn check_side_vision(map: &str, dire: bool) {
    let server = Server::start(if dire { 2 } else { 1 }, &["--map", map]);
    if dire {
        quiet_seat(&server, HeroId::SHADOW_FIEND);
    }
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.within(Duration::from_secs(30), "fog and the minimap ready", |c| {
        c.world().resource::<FogGrid>().worked_out_at().is_some()
            && c.world().resource::<crate::minimap::PhotoTaken>().0
            && c.view()
                .units
                .iter()
                .any(|u| u.kind == UnitKind::CreepMelee && u.team == c.me().team)
    });
    // Let the veil's ordinary easing reach its final alpha without depending on frame speed.
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(100),
        ));
    client.frames(30);
    let team = client.me().team;
    eprintln!("checking fog image: map={map}, team={team:?}");
    assert_eq!(team, if dire { Team::Dire } else { Team::Radiant });
    assert_side_vision(&mut client, team);
}

fn assert_side_vision(client: &mut Client, team: Team) {
    let own = |kind| {
        client
            .view()
            .units
            .iter()
            .find(|u| u.kind == kind && u.team == team)
            .expect("an allied viewer")
    };
    let hero = client.me().pos;
    let tower = own(UnitKind::Tower).pos;
    let creep = own(UnitKind::CreepMelee).pos;
    let enemy = client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Tower && u.team != team)
        .max_by_key(|u| u.pos.distance_squared(hero))
        .expect("a distant enemy tower")
        .pos;
    assert_eq!(client.world().resource::<FogGrid>().team, Some(team));
    for (name, at) in [("hero", hero), ("tower", tower), ("creep", creep)] {
        let at = Vec2::new(at.x.to_f32(), at.y.to_f32());
        assert!(
            client.world().resource::<FogGrid>().lit(at.x, at.y),
            "our {name} reveals its ground"
        );
        assert!(
            veil_alpha(client, at) < 8,
            "our {name}'s minimap vision must be clear"
        );
    }
    let enemy = Vec2::new(enemy.x.to_f32(), enemy.y.to_f32());
    assert!(
        !client.world().resource::<FogGrid>().lit(enemy.x, enemy.y),
        "the enemy's tower does not reveal ground to us"
    );
    assert!(
        veil_alpha(client, enemy) > 170,
        "the minimap must mask enemy tower vision"
    );
    let veil = client.world().resource::<VeilImage>().0.clone();
    let world = client.world_mut();
    assert!(
        world
            .query::<&ImageNode>()
            .iter(world)
            .any(|node| node.image == veil),
        "the minimap uses this fog image"
    );
}
