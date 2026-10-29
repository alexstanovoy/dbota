//! Dota's minimap marks: a hero seen is the filled arrow whichever side it
//! is on; a teleport shows at both ends.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{Intent, Slot, TOWN_PORTAL_SLOT};
use bota_proto::{Cheat, HeroId, Team, UnitKind};

use super::harness::{Client, Server, quiet_seat};

/// The image file each shown hero mark wears, by the side of its hero.
fn hero_marks(client: &mut Client) -> Vec<(Team, Option<String>)> {
    let world = client.world_mut();
    let server = world.resource::<AssetServer>().clone();
    world
        .query::<(&crate::minimap::HeroMark, &ImageNode, &Visibility)>()
        .iter(world)
        .filter(|(_, _, shown)| **shown != Visibility::Hidden)
        .filter_map(|(mark, image, _)| {
            let (team, _) = mark.0?;
            Some((
                team,
                server.get_path(image.image.id()).map(|p| p.to_string()),
            ))
        })
        .collect()
}

/// Both heroes seen (the enemy's through the vision cheat), each is Dota's filled arrow on the
/// minimap, one's own and the enemy's alike (an enemy wore the hollow one, which is for an
/// invisible hero only).
#[test]
fn a_seen_hero_is_the_filled_arrow_whichever_side() {
    let server = Server::start(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::AllVision { on: true });
    let arrow = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file("icon.minimap.hero_arrow")
        .expect("the install's arrow");
    client.within(Duration::from_secs(20), "both heroes marked", |c| {
        let marks = hero_marks(c);
        marks.iter().any(|m| m.0 == Team::Radiant) && marks.iter().any(|m| m.0 == Team::Dire)
    });
    for (team, worn) in hero_marks(&mut client) {
        assert_eq!(worn.as_deref(), Some(arrow.as_str()), "{team:?}'s hero");
    }
}

/// The minimap's rings in a colour that show.
fn rings_in(client: &mut Client, colour: Color) -> usize {
    let world = client.world_mut();
    world
        .query::<(&BorderColor, &Visibility, &Node)>()
        .iter(world)
        .filter(|(border, shown, node)| {
            **shown != Visibility::Hidden
                && border.top == colour
                && node.border_radius == BorderRadius::MAX
        })
        .count()
}

/// One's hero channelling a town portal, in sight: the minimap rings both ends in the side's
/// colour, where it lands and where it leaves from (only the landing was ringed).
#[test]
fn a_teleport_shows_on_the_minimap_at_both_ends() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::Refresh);
    let mine = client.me().team;
    let colour = crate::overlay::team_color(mine);
    let before = rings_in(&mut client, colour);
    let tower = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Tower && u.team == mine)
        .map(|u| (u.id, u.pos))
        .expect("a tower of one's side");
    let core = client.core_mut();
    core.act(Intent::Press {
        slot: Slot::Item(TOWN_PORTAL_SLOT),
        ctrl: false,
    });
    core.act(Intent::Aimed {
        at: (tower.1.x.to_f32(), tower.1.y.to_f32()),
        unit: Some(tower.0),
    });
    let me = client.me().id;
    client.within(
        Duration::from_secs(10),
        "the town portal channelling",
        |c| crate::teleport::channel_share(c.view(), me).is_some(),
    );
    client.frames(3);
    assert_eq!(
        rings_in(&mut client, colour),
        before + 2,
        "the landing and the leaving"
    );
}
