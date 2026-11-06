//! Dota's minimap marks: a hero seen is the filled arrow whichever side it
//! is on; a teleport shows at both ends.

use std::time::Duration;

use bevy::prelude::*;
use bota_client_core::{Intent, Slot, TOWN_PORTAL_SLOT};
use bota_proto::{Cheat, HeroId, Team, UnitKind};

use super::harness::{Client, MID, Server, quiet_seat};

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

/// The ids of the wards of a side in the view.
fn wards_of(client: &Client, side: Team, own: bool) -> Vec<bota_proto::EntityId> {
    client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Ward && (u.team == side) == own)
        .map(|u| u.id)
        .collect()
}

/// Plants one of one's own wards at a spot beside the hero, and says where the new one stands.
fn plant(client: &mut Client, item: bota_proto::ItemId, beside: (f32, f32)) -> (f32, f32) {
    let mine = client.me().team;
    let before = wards_of(client, mine, true);
    client.cheat(Cheat::Item { item });
    let slot = client.slot_of(item).expect("the ward in the bag");
    client.core_mut().act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    client.core_mut().act(Intent::Aimed {
        at: beside,
        unit: None,
    });
    client.until("the ward planted", |c| {
        wards_of(c, mine, true).len() > before.len()
    });
    let new = wards_of(client, mine, true)
        .into_iter()
        .find(|id| !before.contains(id))
        .expect("the new ward");
    let ward = client.view().unit(new).expect("in the view");
    (ward.pos.x.to_f32(), ward.pos.y.to_f32())
}

/// Where the first ward of another side stands, as the view tells it.
fn enemy_ward(client: &Client, mine: Team) -> (f32, f32) {
    let id = wards_of(client, mine, false)[0];
    let ward = client.view().unit(id).expect("in the view");
    (ward.pos.x.to_f32(), ward.pos.y.to_f32())
}

/// Whether the minimap marks a spot of the world with the image of a logical name.
fn marked(client: &mut Client, logical: &str, at: (f32, f32)) -> bool {
    let icon = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file(logical)
        .unwrap_or_else(|| panic!("the install's {logical}"));
    let middle = crate::minimap::on_map(at.0, at.1);
    crate::minimap::shown_unit_marks(client.world_mut())
        .iter()
        .any(|(path, found)| path.as_deref() == Some(icon.as_str()) && found.distance(middle) < 1.0)
}

/// One's own Observer Ward and Sentry Ward, planted, are marked on the minimap with Dota's icon
/// of each, where they stand (units' marks left wards out).
#[test]
fn planted_wards_are_marked_on_the_minimap_with_dotas_icon_of_each() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    let observer = plant(
        &mut client,
        bota_proto::ItemId::OBSERVER_WARD,
        (MID.0 as f32 + 150.0, MID.1 as f32 + 150.0),
    );
    client.within(Duration::from_secs(20), "the Observer marked", |c| {
        marked(c, "icon.minimap.ward_observer", observer)
    });
    let sentry = plant(
        &mut client,
        bota_proto::ItemId::SENTRY_WARD,
        (MID.0 as f32 - 150.0, MID.1 as f32 + 150.0),
    );
    client.within(Duration::from_secs(20), "the Sentry marked", |c| {
        marked(c, "icon.minimap.ward_sentry", sentry)
    });
    assert!(
        !marked(&mut client, "icon.minimap.ward_observer", sentry),
        "the Sentry wears the Observer's icon"
    );
}

/// An enemy Observer Ward nobody sees is no mark on the minimap, whoever stands beside it; a
/// Sentry Ward's true sight shows it, and then it is marked with its icon, as Dota marks the
/// wards one sees.
#[test]
fn an_enemy_ward_is_marked_on_the_minimap_only_when_it_is_seen() {
    let server = Server::start(2, &[]);
    let planted = bota_proto::Vec2::from_ints(MID.0 + 300, MID.1);
    super::harness::ward_planter_seat(
        &server,
        HeroId::SHADOW_FIEND,
        planted,
        bota_proto::ItemId::OBSERVER_WARD,
    );
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    let until = client.tick() + 150;
    client.until("the enemy's ward long planted", |c| c.tick() >= until);
    let mine = client.me().team;
    assert!(
        !client
            .view()
            .units
            .iter()
            .any(|u| u.kind == UnitKind::Ward && u.team != mine),
        "an Observer Ward is seen without true sight"
    );
    let spot = (MID.0 as f32 + 300.0, MID.1 as f32);
    assert!(
        !marked(&mut client, "icon.minimap.ward_observer", spot),
        "an enemy ward nobody sees is marked"
    );
    plant(
        &mut client,
        bota_proto::ItemId::SENTRY_WARD,
        (MID.0 as f32 + 150.0, MID.1 as f32 + 100.0),
    );
    client.until("the enemy's ward seen", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.kind == UnitKind::Ward && u.team != mine)
    });
    let seen = enemy_ward(&client, mine);
    client.within(Duration::from_secs(20), "the seen ward marked", |c| {
        marked(c, "icon.minimap.ward_observer", seen)
    });
}
