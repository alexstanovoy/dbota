//! The announcer's lines of the buildings that fall, as Dota's announcer rules choose them.

use bota_proto::{EventKind, HeroId, Team, UnitKind};

use super::harness::{Client, Server};
use crate::voice::{BuildingsHeard, announce_buildings_once, building_line};

fn fall(unit: u32, team: Team, kind: UnitKind, lane: u8) -> EventKind {
    EventKind::StructureDestroyed {
        unit: bota_proto::EntityId {
            idx: unit,
            generation: 1,
        },
        team,
        kind,
        lane: Some(lane),
        tier: Some(1),
        ranged: None,
    }
}

fn denied(unit: u32) -> EventKind {
    EventKind::Died {
        unit: bota_proto::EntityId {
            idx: unit,
            generation: 1,
        },
        killer: None,
        denied: true,
        gold: 0,
        bounty: None,
        assists: 0,
        awards: None,
        credited_hero: None,
    }
}

/// Every line Dota's announcer has for a tower or barracks falling is in the cache, and each fall
/// the core tells is announced as the rules say: one's own tower the side's `yr` line, the
/// other's `enm`, a tower one's own side struck its deny line, a barracks its own, by lane, and
/// no line for any other building.
#[test]
fn a_fallen_tower_or_barracks_is_announced_as_dotas_rules_choose_the_line() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let mine = client.me().team;
    let theirs = if mine == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    client.core_mut().told = vec![
        fall(1, mine, UnitKind::Tower, 1),
        fall(2, theirs, UnitKind::Tower, 0),
        denied(3),
        fall(3, mine, UnitKind::Tower, 2),
        fall(4, theirs, UnitKind::Barracks, 2),
        fall(5, theirs, UnitKind::Ancient, 0),
        fall(6, theirs, UnitKind::Fountain, 0),
    ];
    let (spoken, loaded) = announce_buildings_once(client.world_mut());
    assert_eq!(
        loaded, 40,
        "36 building lines and four mega lines from Dota are loaded"
    );
    let heard = &client.world().resource::<BuildingsHeard>().0;
    assert_eq!(
        heard,
        &[
            "sound.announcer.tower.fall.yr.top",
            "sound.announcer.tower.fall.enm.mid",
            "sound.announcer.tower.deny.yr.bot",
            "sound.announcer.rax.fall.enm.bot",
        ],
        "a line for a tower or a barracks, by whose it was, the lane and a deny"
    );
    assert_eq!(spoken, 4, "each is queued for the announcer to speak");
}

/// A muted client, or one run with `--mute`, queues nothing for the announcer: the fall is called
/// all the same.
#[test]
fn a_fallen_tower_is_not_announced_to_a_muted_client() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &["--mute"]);
    let mine = client.me().team;
    client.core_mut().told = vec![fall(1, mine, UnitKind::Tower, 1)];
    let (spoken, _) = announce_buildings_once(client.world_mut());
    assert_eq!(
        client.world().resource::<BuildingsHeard>().0,
        ["sound.announcer.tower.fall.yr.top"],
        "the fall is called"
    );
    assert_eq!(spoken, 0, "no line is queued for a muted client");
}

/// Every line `building_line` can call is one of Dota's the cache holds, whoever watches: the
/// hand-written part of the mapping (whose, what, which lane) cannot drift from the install's
/// sound events.
#[test]
fn every_building_line_the_client_can_call_is_in_the_cache() {
    let server = Server::start(1, &[]);
    let client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let mut called = std::collections::BTreeSet::new();
    for watcher in [None, Some(Team::Radiant), Some(Team::Dire)] {
        for lost in [Team::Radiant, Team::Dire] {
            for (kind, denied) in [
                (UnitKind::Tower, false),
                (UnitKind::Tower, true),
                (UnitKind::Barracks, false),
            ] {
                for lane in 0..3 {
                    let name = building_line(watcher, lost, kind, Some(lane), denied)
                        .expect("a line for a tower or barracks");
                    assert!(manifest.file(&name).is_some(), "{name} is not in the cache");
                    called.insert(name);
                }
            }
        }
    }
    assert_eq!(called.len(), 36, "all the lines of Dota's rules are called");
}
