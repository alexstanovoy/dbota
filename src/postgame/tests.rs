//! The summary of a match, read from the files a real game server leaves of it: a scripted
//! match with purchases and a kill whose every figure is known.

use std::path::PathBuf;
use std::sync::OnceLock;

use bevy::prelude::*;
use bota_client_core::{IssuedOrder, Link};
use bota_proto::{Cheat, HeroId, ItemId, Order, Target, Team, UnitKind, Vec2};

use super::summary::{Detail, MatchSummary};
use crate::e2e::harness::Server;

const PURCHASES: [ItemId; 3] = [ItemId::CLARITY, ItemId::TANGO, ItemId::HEALING_SALVE];

/// The files one scripted match left.
struct Played {
    /// The replay, with the metadata file beside it as `.meta` and inside it as a trailer.
    replay: PathBuf,
    meta: PathBuf,
    stats: bota_proto::MatchStats,
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/postgame");
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    dir.join(format!("{}-{name}", std::process::id()))
}

/// A lone player on Map 2 buys three items, goes to level 30, fells a hero the game created
/// beside it, and then walks on the enemy's fountain until the map ends the match.
fn play() -> &'static Played {
    static PLAYED: OnceLock<Played> = OnceLock::new();
    PLAYED.get_or_init(|| {
        let (replay, meta) = (scratch("match.brp"), scratch("match.meta"));
        let mut server = Server::start(
            1,
            &[
                "--replay",
                replay.to_str().expect("UTF-8"),
                "--meta",
                meta.to_str().expect("UTF-8"),
                "--meta-trailer",
                // Heroes fall in this match.
            ],
        );
        let map = bota_core::map_of(bota_core::MAP2_ID);
        let middle = Vec2::from_ints(7500, 7500);
        let enemy_fountain = bota_core::fountain_pos(map, Team::Dire);
        let mut decisions = 0u32;
        let outcome = Link::play(
            &server.address,
            "postgame",
            HeroId::SHADOW_FIEND,
            |session| {
                decisions += 1;
                let hero = IssuedOrder::hero;
                let cheat = |cheat| hero(Order::Cheat { cheat });
                match decisions {
                    1 => return vec![cheat(Cheat::Levels { count: 29 })],
                    2 => return vec![cheat(Cheat::StartGame)],
                    3 => return PURCHASES.map(|item| hero(Order::Buy { item })).to_vec(),
                    4 => return vec![cheat(Cheat::Teleport { at: middle })],
                    5 => {
                        return vec![cheat(Cheat::CreateHero {
                            hero: HeroId::SHADOW_FIEND,
                            enemy: true,
                            at: middle + Vec2::from_ints(200, 0),
                        })];
                    }
                    _ => {}
                }
                let Some(view) = session.current() else {
                    return Vec::new();
                };
                let killed = session
                    .slot()
                    .and_then(|slot| view.player(slot))
                    .is_some_and(|me| me.kills >= 1);
                let foe = view
                    .units
                    .iter()
                    .find(|u| u.kind == UnitKind::Hero && u.team == Team::Dire);
                match foe {
                    Some(foe) if !killed => vec![hero(Order::Attack {
                        target: Target::Unit(foe.id),
                    })],
                    _ if killed && decisions.is_multiple_of(30) => vec![hero(Order::Attack {
                        target: Target::Pos(enemy_fountain),
                    })],
                    _ => Vec::new(),
                }
            },
        )
        .expect("the match is played to its end");
        assert_eq!(outcome.winner, Team::Radiant, "{outcome:?}");
        server.wait();
        let stats = records(&replay)
            .into_iter()
            .find_map(|record| match record {
                bota_proto::ReplayRecord::Msg(bota_proto::ServerMsg::MatchOver {
                    stats, ..
                }) => Some(stats),
                _ => None,
            })
            .expect("the replay ends with the match");
        Played {
            replay,
            meta,
            stats,
        }
    })
}

/// The records of a finished replay, read as a player of this build reads it.
fn records(path: &std::path::Path) -> Vec<bota_proto::ReplayRecord> {
    let file = std::fs::File::open(path).expect("the replay");
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut bota_client_core::UnpackedReplay::new(file), &mut bytes)
        .expect("a finished stream");
    let mut reader = bota_proto::FrameReader::new();
    reader.push(&bytes);
    std::iter::from_fn(|| reader.next_message().expect("a record")).collect()
}

fn summary_of(bytes: &[u8]) -> MatchSummary {
    let meta = bota_replay::meta::decode(bytes).expect("a metadata file");
    MatchSummary::from_meta(&meta).expect("a summary")
}

/// The numbers the script makes known, as rows tell them.
fn check(summary: &MatchSummary, stats: &bota_proto::MatchStats) {
    assert_eq!(summary.detail, Detail::Meta);
    assert_eq!(summary.winner, Some(Team::Radiant));
    let me = summary
        .rows
        .iter()
        .find(|r| r.slot == bota_proto::SlotId(0))
        .expect("the player's row");
    assert_eq!(me.team, Team::Radiant);
    assert_eq!(me.hero, HeroId::SHADOW_FIEND);
    assert_eq!(me.level, 30, "levelled by the script");
    assert_eq!(
        (me.kills, me.deaths, me.assists),
        (1, 0, 0),
        "one hero fell to it"
    );
    let held: Vec<ItemId> = me.items.iter().flatten().copied().collect();
    assert_eq!(held, PURCHASES, "the items bought, in the order bought");
    let own = stats
        .slots
        .iter()
        .find(|s| s.slot == me.slot)
        .expect("stats");
    assert_eq!(me.net_worth, Some(own.net_worth));
    assert_eq!(me.last_hits, Some(own.last_hits));
    assert_eq!(me.hero_damage, Some(own.hero_damage));
    assert!(own.hero_damage > 0, "the kill dealt damage");
    assert_eq!(me.tower_damage, Some(own.structure_damage));
    let minutes = i64::from(summary.seconds) / 60;
    assert_eq!(me.gpm, own.net_worth / minutes.max(1) as i32);
    // One point for the start and one for every whole minute of game time.
    assert_eq!(summary.points.len() as u32, summary.seconds / 60 + 1);
}

#[test]
fn a_scripted_match_reads_back_with_its_kill_and_its_purchases() {
    let played = play();
    check(
        &summary_of(&std::fs::read(&played.meta).expect("the meta file")),
        &played.stats,
    );
}

/// The same summary comes out of the file the server wrote, the one the replay carries and the
/// one the recorder works out of the replay's records.
#[test]
fn the_server_file_the_trailer_and_the_recorder_agree() {
    let played = play();
    let written = std::fs::read(&played.meta).expect("the meta file");
    let mut replay = std::fs::File::open(&played.replay).expect("the replay");
    let carried = bota_client_core::meta_in_replay(&mut replay)
        .expect("reads")
        .expect("a trailer");
    let rebuilt = super::meta_of_file(&played.replay).expect("the file");
    let worked = bota_client_core::meta_of_replay(std::fs::File::open(&played.replay).unwrap())
        .expect("the records");
    assert_eq!(carried, written);
    assert_eq!(worked, written);
    assert_eq!(rebuilt, written);
}

/// A file cut short (a server killed) tells the match up to there, with no winner.
#[test]
fn a_file_cut_short_still_gives_a_summary_without_a_winner() {
    let played = play();
    let bytes = std::fs::read(&played.meta).expect("the meta file");
    let meta = bota_replay::meta::decode(&bytes[..bytes.len() * 3 / 4]).expect("what there is");
    assert!(!meta.complete);
    let summary = MatchSummary::from_meta(&meta).expect("a summary");
    assert_eq!(summary.winner, None);
    assert!(!summary.rows.is_empty());
}

/// A metadata file of a later schema, with a record and a statistic this build has no number
/// for, still gives a summary.
#[test]
fn a_file_with_things_of_a_later_build_still_reads() {
    use bota_replay::meta::{MetaHeader, MetaRecord, Writer, stat};
    let mut out = Writer::default();
    let header = MetaHeader {
        magic: MetaHeader::MAGIC,
        schema: 2,
        wire_fingerprint: 1,
        rules_version: 1,
        match_id: 1,
        map: 2,
        tick_rate: 30,
        pregame_ticks: 900,
        catalogue_items: 0,
        catalogue_digest: 0,
    };
    for record in [
        MetaRecord::Header(header),
        MetaRecord::Seat {
            slot: 0,
            team: 0,
            hero: 3,
        },
        MetaRecord::Sample {
            tick: 900,
            slot: 0,
            stats: vec![(stat::NET_WORTH, 600), (9_999, 1)],
        },
        MetaRecord::Sample {
            tick: 900 + 1800,
            slot: 0,
            stats: vec![(stat::NET_WORTH, 1200), (stat::KILLS, 2)],
        },
    ] {
        assert!(out.push(&record, false));
    }
    let mut bytes = out.into_bytes();
    bytes.extend(4u32.to_le_bytes());
    bytes.extend([250, 1, 2, 3]);
    let summary = summary_of(&bytes);
    assert_eq!(summary.rows[0].kills, 2);
    assert_eq!(
        summary.points.len(),
        1,
        "the first sample is the start; the last is the end"
    );
}

/// The screen a match ends on: a replay played to its end shows a row for every seat, the bought
/// items as icons, and a point a game minute on each graph.
#[test]
fn the_screen_at_the_end_shows_every_seat_and_a_point_a_minute() {
    use super::draw::{Dot, Graph, ItemSlot, SeatRow, Segment};
    use crate::e2e::harness::Client;
    let played = play();
    let mut client = Client::run(&["--replay", played.replay.to_str().expect("UTF-8")]);
    client.until("the replay played to its end and read", |c| {
        if let bota_client_core::Source::Replay(player) = &mut c.core_mut().source {
            player.advance_ticks(1.0e7);
        }
        c.world().non_send::<super::PostGame>().summary().is_some()
    });
    client.frames(3);
    let summary = client
        .world()
        .non_send::<super::PostGame>()
        .summary()
        .cloned()
        .expect("a summary");
    check(&summary, &played.stats);
    let world = client.world_mut();
    let rows = world.query::<&SeatRow>().iter(world).count();
    assert_eq!(rows, summary.rows.len(), "a row for every seat");
    for graph in [Graph::NetWorth, Graph::Xp] {
        let dots = world
            .query::<&Dot>()
            .iter(world)
            .filter(|d| d.graph == graph)
            .count();
        let lines = world
            .query::<&Segment>()
            .iter(world)
            .filter(|s| s.0 == graph)
            .count();
        assert_eq!(dots, summary.points.len(), "{graph:?}: a point a minute");
        assert_eq!(lines, summary.points.len() - 1, "{graph:?}: joined");
    }
    let icons = world
        .query::<(&ItemSlot, &ImageNode)>()
        .iter(world)
        .filter(|(_, image)| image.color != Color::NONE)
        .count();
    assert_eq!(icons, PURCHASES.len(), "the three items bought are drawn");
}
