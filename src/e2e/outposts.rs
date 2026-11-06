//! The Outposts as Dota shows who holds them: each drawn in its holder's colours (the disc and
//! its ambient teal for the Radiant, red for the Dire), the channel's rings while a hero captures
//! one, and the burst when it is taken, after which it wears the taker's colours.

use std::time::Duration;

use bota_client_core::Intent;
use bota_proto::{Cheat, EntityId, HeroId, ModifierSpec, Target, Team, UnitKind, Vec2};

use super::harness::{Client, Server};
use crate::particles::{Library, Playing};
use crate::units::{ModelBank, Unit};

/// Patience for what follows at once from what the test did.
const SOON: Duration = Duration::from_secs(60);

/// A match on the Dota map with its Outposts and one seat, the Radiant's, the game begun.
fn outpost_match() -> (Server, Client) {
    let server = Server::start(1, &["--map", "0", "--objectives", "outposts"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert_eq!(client.core().my_team(), Some(Team::Radiant), "one's side");
    client.cheat(Cheat::StartGame);
    (server, client)
}

/// The Outpost of the view standing at a place of the map.
fn outpost_at(client: &Client, at: Vec2) -> Option<EntityId> {
    client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Outpost && u.pos == at)
        .map(|u| u.id)
}

/// The model a unit is drawn with, once it is drawn.
fn drawn_model(client: &Client, id: EntityId) -> Option<String> {
    let unit = client.world().get::<Unit>(client.body(id)?)?;
    Some(
        client.world().resource::<ModelBank>().0[unit.model?]
            .model
            .logical
            .clone(),
    )
}

/// How many times the particle effect of a logical name started.
fn started(client: &Client, effect: &str) -> u32 {
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect(effect)
        .unwrap_or_else(|| panic!("{effect} in the asset cache"));
    world.resource::<Playing>().started(effect)
}

/// How many instances of the particle effect of a logical name play on, not ended.
fn playing(client: &Client, effect: &str) -> usize {
    let world = client.world();
    let effect = world
        .resource::<Library>()
        .effect(effect)
        .unwrap_or_else(|| panic!("{effect} in the asset cache"));
    world
        .resource::<Playing>()
        .instances_of(effect)
        .filter(|instance| !instance.ending())
        .count()
}

/// How many times a bound sound of a logical name was heard.
fn heard(client: &Client, logical: &str) -> u32 {
    let heard = &client.world().resource::<crate::fx::Heard>().0;
    heard.get(logical).copied().unwrap_or_default()
}

/// What one's hero has as long as the test needs it: no death, and blows that fell towers fast.
fn tower_breaker(client: &mut Client) {
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::Levels { count: 24 });
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            physical_damage: ModifierSpec::MAX_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
}

/// One's hero stands beside an enemy tower and attacks it until it falls.
fn fell_tower(client: &mut Client, at: Vec2) {
    let tower = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Tower && u.pos == at)
        .expect("the tower standing")
        .id;
    client.stand_at((at.x.to_int() - 250, at.y.to_int()));
    client.core_mut().act(Intent::Smart {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(tower),
    });
    client.until("the tower fallen", |c| c.view().unit(tower).is_none());
}

/// Both Outposts start held by the side on whose half they stand: each is drawn in that side's
/// material group and, seen, wears that side's ambient, the Radiant's teal and the Dire's red.
#[test]
fn each_outpost_is_drawn_in_its_holders_colours() {
    let (_server, mut client) = outpost_match();
    for outpost in bota_core::DOTA_OUTPOSTS {
        let (model, ambient) = match outpost.home {
            Team::Radiant => ("objective.outpost.skin1.model", "outpost.radiant_ambient"),
            _ => ("objective.outpost.skin2.model", "outpost.dire_ambient"),
        };
        client.stand_at((outpost.at.x.to_int() - 300, outpost.at.y.to_int()));
        let id = outpost_at(&client, outpost.at).expect("the outpost in the view");
        client.within(SOON, "the outpost drawn in its holder's skin", |c| {
            drawn_model(c, id).as_deref() == Some(model)
        });
        client.within(SOON, "the outpost wearing its holder's ambient", |c| {
            playing(c, ambient) > 0
        });
    }
}

/// Once the Dire's tier two has fallen, one's hero captures the Dire's Outpost: the channel's
/// rings play with Outpost.Channel while it channels, the burst with Outpost.Captured when it is
/// taken, and from then on it is drawn in the Radiant's colours and wears their ambient, the
/// Dire's ended.
#[test]
fn a_captured_outpost_shows_the_channel_and_the_burst_then_the_takers_colours() {
    let (_server, mut client) = outpost_match();
    tower_breaker(&mut client);
    let tier = |tier: u8| {
        bota_core::rules::DIRE_TOWERS
            .iter()
            .find(|(lane, t, _)| *lane == bota_core::rules::LANE_MID && *t == tier)
            .expect("a Dire mid tower")
            .2
    };
    fell_tower(&mut client, tier(1));
    fell_tower(&mut client, tier(2));
    let outpost = bota_core::DOTA_OUTPOSTS
        .into_iter()
        .find(|o| o.home == Team::Dire)
        .expect("the Dire's Outpost")
        .at;
    client.stand_at((outpost.x.to_int() - 250, outpost.y.to_int()));
    let id = outpost_at(&client, outpost).expect("the outpost in the view");
    client.within(SOON, "the outpost drawn in the Dire's skin", |c| {
        drawn_model(c, id).as_deref() == Some("objective.outpost.skin2.model")
    });
    client.core_mut().act(Intent::Smart {
        at: (outpost.x.to_f32(), outpost.y.to_f32()),
        unit: Some(id),
    });
    client.within(SOON, "the channel's rings and sound", |c| {
        playing(c, "outpost.channel") == 1 && heard(c, "sound.outpost.channel") > 0
    });
    client.within(SOON, "the outpost taken", |c| {
        c.view().unit(id).is_some_and(|u| u.team == Team::Radiant)
    });
    client.within(SOON, "the burst and its sound", |c| {
        started(c, "outpost.captured") == 1 && heard(c, "sound.outpost.captured") == 1
    });
    client.within(SOON, "the outpost drawn in the Radiant's skin", |c| {
        drawn_model(c, id).as_deref() == Some("objective.outpost.skin1.model")
    });
    // The Radiant's home Outpost is out of one's sight here: its effects show to nobody.
    client.within(SOON, "the taker's ambient, the channel ended", |c| {
        playing(c, "outpost.radiant_ambient") == 1
            && playing(c, "outpost.dire_ambient") == 0
            && playing(c, "outpost.channel") == 0
    });
}
