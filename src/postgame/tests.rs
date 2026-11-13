//! The summary of a match, read from the files a real game server leaves of it: a scripted
//! match with purchases and a kill whose every figure is known.

use std::path::PathBuf;
use std::sync::OnceLock;

use bevy::prelude::*;
use bevy::ui_widgets::Button;
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
    crate::test_support::scratch_path(&format!("postgame/{}-{name}", std::process::id()))
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
    assert_eq!(summary.rows[0].name.as_deref(), Some("postgame"));
    assert_eq!(summary.rows[0].bot, Some(true));
    assert_eq!(summary.rows[0].left_at, None);
    assert!(
        summary
            .structures
            .iter()
            .any(|fall| fall.kind == UnitKind::Tower
                && fall.team == Team::Dire
                && fall.lane == Some(bota_core::rules::LANE_MID)
                && fall.tier == Some(1)),
        "the actual replay retains the deciding tower's location"
    );
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
    assert_eq!(
        me.gpm,
        Some((i64::from(own.net_worth) * 60 / i64::from(summary.seconds.max(1))) as i32)
    );
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
    let written_meta = bota_replay::meta::decode(&written).expect("metadata");
    assert!(written_meta.profiles().is_none(),
        "a launch without a matchmaker or accounts writes no provenance record");
    assert_eq!(worked,written,"the pure replay recorder agrees with the complete server sidecar");
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

// Encode the extension records as tuples so these regressions also compile against the
// reader from before their enum variants existed. Their tags are pinned by the meta registry.
fn recording_extensions() -> Vec<u8> {
    use bota_replay::meta::{MetaHeader, MetaRecord, Writer, stat};
    let mut out = Writer::default();
    for record in [
        MetaRecord::Header(MetaHeader {
            magic: MetaHeader::MAGIC,
            schema: 1,
            wire_fingerprint: 1,
            rules_version: 0,
            match_id: 1,
            map: 0,
            tick_rate: 30,
            pregame_ticks: 0,
            catalogue_items: 0,
            catalogue_digest: 0,
        }),
        MetaRecord::Seat {
            slot: 0,
            team: 0,
            hero: HeroId::SHADOW_FIEND.0,
        },
        MetaRecord::Sample {
            tick: 1800,
            slot: 0,
            stats: vec![(stat::NET_WORTH, 1000)],
        },
        MetaRecord::Structure {
            tick: 900,
            kind: UnitKind::Tower as u8,
            team: 1,
        },
        MetaRecord::End {
            tick: 1800,
            winner: Some(0),
        },
    ] {
        assert!(out.push(&record, false));
    }
    let mut bytes = out.into_bytes();
    bytes.extend(bota_proto::encode_frame_to_vec(&(10_u32, 1234_u32, 0_u8)).expect("leave frame"));
    bytes.extend(
        bota_proto::encode_frame_to_vec(&(11_u32, 0_u8, "offline player", false))
            .expect("name frame"),
    );
    bytes.extend(
        bota_proto::encode_frame_to_vec(&(
            12_u32,
            0_u32,
            Some(bota_core::rules::LANE_MID),
            Some(2_u8),
            None::<bool>,
        ))
        .expect("location frame"),
    );
    bytes
}

#[test]
fn a_standalone_replay_summary_retains_its_recorded_player_name() {
    let summary = summary_of(&recording_extensions());
    assert_eq!(summary.rows[0].name.as_deref(), Some("offline player"));
    assert_eq!(summary.rows[0].bot, Some(false));
}

#[test]
fn a_standalone_replay_summary_identifies_the_leaver_and_leave_tick() {
    let summary = summary_of(&recording_extensions());
    assert_eq!(summary.rows[0].left_at, Some(1234));
}

#[test]
fn a_short_match_uses_its_elapsed_seconds_for_gold_per_minute() {
    use bota_replay::meta::{MetaRecord, stat};
    let mut meta = bota_replay::meta::decode(&recording_extensions()).expect("metadata");
    for record in &mut meta.records {
        match record {
            MetaRecord::Sample { tick, stats, .. } => {
                *tick = 238 * 30;
                stats.push((stat::GOLD_EARNED, 1507));
            }
            MetaRecord::End { tick, .. } => *tick = 238 * 30,
            _ => {}
        }
    }
    let summary = MatchSummary::from_meta(&meta).expect("a summary");
    assert_eq!(summary.rows[0].gpm, Some(1507 * 60 / 238));
}

#[test]
fn team_graphs_leave_missing_samples_as_gaps_instead_of_zero() {
    use bota_replay::meta::{MetaRecord, stat};
    let mut meta = bota_replay::meta::decode(&recording_extensions()).expect("metadata");
    for record in &mut meta.records {
        if let MetaRecord::End { tick, .. } = record {
            *tick = 4500;
        }
    }
    meta.records.extend([
        MetaRecord::Seat {
            slot: 1,
            team: 1,
            hero: HeroId::PUDGE.0,
        },
        MetaRecord::Sample {
            tick: 2700,
            slot: 1,
            stats: vec![(stat::NET_WORTH, 500)],
        },
        MetaRecord::Sample {
            tick: 3600,
            slot: 0,
            stats: vec![(stat::XP, 50)],
        },
        MetaRecord::Sample {
            tick: 3600,
            slot: 1,
            stats: vec![(stat::NET_WORTH, 550)],
        },
        MetaRecord::Sample {
            tick: 4500,
            slot: 0,
            stats: vec![(stat::NET_WORTH, 1200)],
        },
        MetaRecord::Sample {
            tick: 4500,
            slot: 1,
            stats: vec![(stat::NET_WORTH, 600)],
        },
    ]);
    let summary = MatchSummary::from_meta(&meta).expect("summary");
    let curves = super::graphs::curves(&summary, super::graphs::Metric::Advantage);
    assert_eq!(
        curves.len(),
        1,
        "there is no recorded experience for both seats"
    );
    let points: Vec<_> = curves[0]
        .values
        .iter()
        .map(|point| (point.second, point.value, point.connected))
        .collect();
    assert_eq!(points, [(150, 600, false)]);
}

#[test]
fn known_graph_stats_survive_unknown_ids_and_pregame_purchases_keep_their_sign() {
    use bota_replay::meta::{MetaRecord, stat};
    let mut meta = bota_replay::meta::decode(&recording_extensions()).expect("metadata");
    meta.header.pregame_ticks = 900;
    for record in &mut meta.records {
        if let MetaRecord::Sample { stats, .. } = record {
            *stats = (1000..1100).map(|id| (id, 123)).collect();
            stats.push((stat::NET_WORTH, 42));
        }
    }
    meta.records.push(MetaRecord::Purchase {
        tick: 899,
        slot: 0,
        item: ItemId::BOTTLE.0,
    });
    let summary = MatchSummary::from_meta(&meta).expect("summary");
    let recording = summary.recording.as_ref().expect("timed recording");
    assert_eq!(
        recording.value(bota_proto::SlotId(0), stat::NET_WORTH, 30),
        Some(42)
    );
    assert_eq!(recording.purchases[0].second, -1);
}

#[test]
fn graph_axes_have_round_steps_and_keep_the_exact_end_label() {
    use super::graphs::{Axis, Curve, Point, label_positions, time_ticks};
    let curve = Curve {
        name: "recorded".to_owned(),
        slot: None,
        colour: [0; 3],
        values: vec![Point {
            second: 0,
            value: 130,
            connected: false,
        }],
    };
    let axis = Axis::of(&[curve], false);
    assert_eq!(axis.ticks, [150, 125, 100, 75, 50, 25, 0]);
    assert_eq!(time_ticks(38 * 60 + 12), [0, 600, 1200, 1800, 2292]);
    let labels = label_positions(&[0.5; 10], 50.0);
    assert!(
        labels.windows(2).all(|pair| pair[1] - pair[0] >= 16.99),
        "the label container can scroll, but labels never collide"
    );
}

#[test]
fn a_standalone_replay_summary_keeps_the_towers_lane_and_tier() {
    let summary = summary_of(&recording_extensions());
    assert_eq!(summary.structures.len(), 1);
    let building = &summary.structures[0];
    assert_eq!(building.team, Team::Dire);
    assert_eq!(building.kind, UnitKind::Tower);
    assert_eq!(building.lane, Some(bota_core::rules::LANE_MID));
    assert_eq!(building.tier, Some(2));
}

#[test]
fn unknown_buildings_and_hostile_location_indices_do_not_shift_a_known_fall() {
    use bota_replay::meta::MetaRecord;
    let mut meta = bota_replay::meta::decode(&recording_extensions()).expect("metadata");
    meta.records.insert(
        0,
        MetaRecord::Structure {
            tick: 800,
            kind: 255,
            team: 1,
        },
    );
    for record in &mut meta.records {
        if let MetaRecord::StructureLocation { index, .. } = record {
            *index = 1;
        }
    }
    meta.records.push(MetaRecord::StructureLocation {
        index: u32::MAX,
        lane: Some(2),
        tier: Some(4),
        ranged: Some(true),
    });
    let summary = MatchSummary::from_meta(&meta).expect("summary");
    assert_eq!(summary.structures.len(), 1);
    assert_eq!(summary.structures[0].lane, Some(bota_core::rules::LANE_MID));
    assert_eq!(summary.structures[0].tier, Some(2));
}

#[test]
fn old_metadata_keeps_optional_identity_and_departure_fields_unknown() {
    let mut meta = bota_replay::meta::decode(&recording_extensions()).expect("metadata");
    meta.records.retain(|record| {
        !matches!(
            record,
            bota_replay::meta::MetaRecord::SeatName { .. }
                | bota_replay::meta::MetaRecord::Leave { .. }
                | bota_replay::meta::MetaRecord::StructureLocation { .. }
        )
    });
    let summary = MatchSummary::from_meta(&meta).expect("old summary");
    assert_eq!(summary.rows[0].name, None);
    assert_eq!(summary.rows[0].bot, None);
    assert_eq!(summary.rows[0].left_at, None);
    assert_eq!(summary.structures[0].lane, None);
    assert_eq!(summary.structures[0].tier, None);
}

/// The screen a match ends on: a replay played to its end shows a row for every seat, the bought
/// items as icons, and a point a game minute on each graph.
#[test]
fn the_screen_at_the_end_shows_every_seat_and_a_point_a_minute() {
    use super::graphs::Metric;
    use super::render::{Dot, ItemSlot, SeatRow, Segment};
    use crate::e2e::harness::Client;
    let played = play();
    let metadata = bota_replay::meta::decode(&std::fs::read(&played.meta).expect("metadata bytes"))
        .expect("metadata");
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
    result_click(&mut client, "results scoreboard");
    let world = client.world_mut();
    let rows: Vec<_> = world
        .query::<&SeatRow>()
        .iter(world)
        .map(|row| row.0)
        .collect();
    assert_eq!(
        rows,
        summary.rows.iter().map(|row| row.slot).collect::<Vec<_>>(),
        "a row for every seat"
    );
    let icons = world
        .query::<(&ItemSlot, &ImageNode)>()
        .iter(world)
        .filter(|(_, image)| image.color != Color::NONE)
        .count();
    assert_eq!(icons, PURCHASES.len(), "the three items bought are drawn");
    for graph in [Metric::NetWorth, Metric::Experience] {
        result_click(&mut client, "results graphs");
        result_click(&mut client, "results graph picker");
        result_click(
            &mut client,
            &format!("results graph {}", graph.label().to_lowercase()),
        );
        let world = client.world_mut();
        let dots = world
            .query::<&Dot>()
            .iter(world)
            .filter(|dot| dot.graph == graph)
            .count();
        let lines = world
            .query::<&Segment>()
            .iter(world)
            .filter(|line| line.0 == graph)
            .count();
        let stat = match graph {
            Metric::NetWorth => bota_replay::meta::stat::NET_WORTH,
            Metric::Experience => bota_replay::meta::stat::XP,
            _ => unreachable!(),
        };
        let mut samples = std::collections::BTreeMap::<u8, std::collections::BTreeSet<u32>>::new();
        for record in &metadata.records {
            if let bota_replay::meta::MetaRecord::Sample { tick, slot, stats } = record
                && stats.iter().any(|(id, _)| *id == stat)
            {
                samples.entry(*slot).or_default().insert(
                    tick.saturating_sub(metadata.header.pregame_ticks)
                        / u32::from(metadata.header.tick_rate),
                );
            }
        }
        assert!(
            samples.values().all(|times| times.len() <= 12),
            "the real fixture is short enough to draw sample dots"
        );
        let expected_dots: usize = samples.values().map(std::collections::BTreeSet::len).sum();
        assert_eq!(
            dots, expected_dots,
            "one dot for each recorded sample, independently of curve generation"
        );
        assert_eq!(
            lines,
            expected_dots - samples.len(),
            "adjacent complete recorded samples are joined"
        );
        let (place, node) = world
            .query::<(&Name, &UiGlobalTransform, &ComputedNode)>()
            .iter(world)
            .find(|(name, _, _)| name.as_str() == "results graph plot")
            .map(|(_, place, node)| (place.translation, node.size()))
            .expect("plot");
        let right = world
            .query::<(&Dot, &UiGlobalTransform)>()
            .iter(world)
            .filter(|(dot, _)| dot.graph == graph)
            .map(|(_, place)| place.translation.x)
            .fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (right - (place.x + node.x / 2.0)).abs() < 1.0,
            "the final recorded point is drawn at the exact right edge"
        );
    }
}

#[test]
fn recorded_results_offer_the_graph_picker_and_no_timeline_page() {
    use crate::e2e::harness::Client;
    let played = play();
    let mut client = Client::run(&["--replay", played.replay.to_str().expect("UTF-8")]);
    client.until("the replay ended", |client| {
        if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
            player.advance_ticks(1.0e7);
        }
        client
            .world()
            .non_send::<super::PostGame>()
            .summary()
            .is_some()
    });
    client.frames(4);
    let world = client.world_mut();
    let buttons: Vec<String> = world
        .query_filtered::<&Name, With<Button>>()
        .iter(world)
        .map(ToString::to_string)
        .collect();
    assert!(
        buttons.iter().any(|name| name == "results graphs"),
        "graph pager missing: {buttons:?}"
    );
    assert!(
        !buttons.iter().any(|name| name.contains("timeline")),
        "Timeline is no results page"
    );
}

/// A game of the service has its metadata fetched by the menu: when the file comes the screen
/// shows the summary, and when the service has none it shows the final numbers.
#[test]
fn a_game_of_the_service_shows_the_fetched_file_or_the_final_numbers_when_there_is_none() {
    use super::{Fetched, Origin, PostGame};
    use crate::e2e::harness::Client;
    use bota_radovid::proto::MatchId;
    let played = play();
    let meta = std::fs::read(&played.meta).expect("the meta file");
    for (bytes, detail) in [(Some(meta), Detail::Meta), (None, Detail::Stats)] {
        let mut client = Client::run(&["--replay", played.replay.to_str().expect("UTF-8")]);
        let id = MatchId(7);
        // Past the start-up, which sets the origin the command line names.
        client.frames(3);
        {
            let mut post = client.world_mut().non_send_mut::<PostGame>();
            post.expect(Origin::Service(id));
            post.begin();
        }
        client.until("the replay played to its end", |c| {
            if let bota_client_core::Source::Replay(player) = &mut c.core_mut().source {
                player.advance_ticks(1.0e7);
            }
            c.core().over.is_some()
        });
        client.frames(3);
        assert!(
            client
                .world()
                .non_send::<PostGame>()
                .summary()
                .is_some_and(|summary| summary.detail == Detail::Stats),
            "final numbers are shown immediately while metadata is unavailable"
        );
        client
            .world_mut()
            .non_send_mut::<PostGame>()
            .provide(Fetched {
                match_id: id,
                bytes,
            });
        client.until("the requested detail shown", |c| {
            c.world()
                .non_send::<PostGame>()
                .summary()
                .is_some_and(|summary| summary.detail == detail)
        });
        let summary = client
            .world()
            .non_send::<PostGame>()
            .summary()
            .cloned()
            .expect("a summary");
        assert_eq!(summary.detail, detail);
        assert_eq!(summary.winner, Some(Team::Radiant));
        if detail == Detail::Meta {
            check(&summary, &played.stats);
        } else {
            client
                .world_mut()
                .non_send_mut::<PostGame>()
                .provide(Fetched {
                    match_id: MatchId(8),
                    bytes: Some(std::fs::read(&played.meta).expect("local metadata")),
                });
            client.frames(3);
            assert_eq!(
                client
                    .world()
                    .non_send::<PostGame>()
                    .summary()
                    .expect("summary")
                    .detail,
                Detail::Stats,
                "a different match cannot replace this result"
            );
            client
                .world_mut()
                .non_send_mut::<PostGame>()
                .provide(Fetched {
                    match_id: id,
                    bytes: Some(std::fs::read(&played.meta).expect("local metadata")),
                });
            client.until("late local metadata upgrades the open result", |client| {
                client
                    .world()
                    .non_send::<PostGame>()
                    .summary()
                    .is_some_and(|summary| summary.detail == Detail::Meta)
            });
        }
    }
}

/// A metadata file is someone else's (a replay's tail, or the service's): it is read whole but
/// says only what a match can. Seats past a match's, a seat named again and samples past the
/// longest match are left out, so the screen's rows and graph points stay as few as a match has.
#[test]
fn a_file_of_more_seats_and_samples_than_a_match_has_is_cut_to_a_match() {
    use bota_replay::meta::{Meta, MetaHeader, MetaRecord, stat};
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
    let seat = |slot: u8| MetaRecord::Seat {
        slot,
        team: slot % 2,
        hero: 3,
    };
    let mut records: Vec<MetaRecord> = (0..=255).chain([0, 0]).map(seat).collect();
    let sample = |slot: u8, minute: u32| MetaRecord::Sample {
        tick: 900 + minute * 1800,
        slot,
        stats: vec![(stat::NET_WORTH, i64::from(minute))],
    };
    records.extend((0..=255).map(|slot| sample(slot, 0)));
    records.extend((1..100_000).map(|minute| sample(0, minute)));
    let meta = Meta {
        damaged: false,
        header,
        records,
        skipped: 0,
        complete: false,
    };

    let summary = MatchSummary::from_meta(&meta).expect("a summary");

    let longest = bota_replay::limits::MAX_MINUTES as usize;
    assert_eq!(summary.rows.len(), bota_proto::limits::MAX_SEATS);
    assert_eq!(
        summary.points.len(),
        longest + 1,
        "minute 0 to the longest's last"
    );
}

fn result_click(client: &mut crate::e2e::harness::Client, name: &str) {
    client.click_named(name);
}

#[test]
fn recorded_graphs_and_player_pages_use_the_recorded_match_without_changing_the_session() {
    use super::{graphs::Metric, view::Page};
    use crate::e2e::harness::Client;
    let played = play();
    let mut client = Client::run(&["--replay", played.replay.to_str().expect("UTF-8")]);
    if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
        player.advance_ticks(1.0e7);
    }
    client.until("recorded results", |client| {
        client
            .world()
            .non_send::<super::PostGame>()
            .summary()
            .is_some()
    });
    client.frames(4);
    let tick = client.tick();
    result_click(&mut client, "results graphs");
    for metric in Metric::ALL {
        result_click(&mut client, "results graph picker");
        result_click(
            &mut client,
            &format!("results graph {}", metric.label().to_lowercase()),
        );
        let view = client
            .world()
            .non_send::<super::PostGame>()
            .overlay
            .as_ref()
            .expect("modal");
        assert_eq!(view.page, Page::Graphs);
        assert_eq!(view.metric, metric);
        let world = client.world_mut();
        let grids: Vec<_> = world
            .query::<&super::render::chart::Grid>()
            .iter(world)
            .collect();
        assert!(grids.iter().any(|line| line.vertical), "minute grid");
        assert!(
            grids.iter().any(|line| !line.vertical && line.major),
            "zero line"
        );
        assert!(
            client
                .shown_texts()
                .iter()
                .any(|text| text == metric.label())
        );
    }
    let seconds = client
        .world()
        .non_send::<super::PostGame>()
        .summary()
        .expect("summary")
        .seconds;
    let world = client.world_mut();
    let (centre, size) = world
        .query::<(&Name, &UiGlobalTransform, &ComputedNode)>()
        .iter(world)
        .find(|(name, _, _)| name.as_str() == "results graph plot")
        .map(|(_, place, node)| (place.translation, node.size()))
        .expect("plot");
    client.mouse(centre + bevy::prelude::Vec2::new(size.x / 2.0 - 2.0, 0.0));
    client.frames(3);
    let expected = format!("{} · HEALING", super::graphs::clock(seconds));
    assert!(
        client.shown_texts().contains(&expected),
        "the right edge hovers the exact end: {expected}"
    );
    result_click(&mut client, "results graph picker");
    client.key(KeyCode::Escape, bevy::input::ButtonState::Pressed);
    client.key(KeyCode::Escape, bevy::input::ButtonState::Released);
    client.frames(3);
    assert!(
        client.world().non_send::<super::PostGame>().visible(),
        "Escape dismisses the picker first"
    );
    assert!(
        !client
            .world()
            .non_send::<super::PostGame>()
            .overlay
            .as_ref()
            .expect("modal")
            .picker
    );
    result_click(&mut client, "results graph picker");
    client.mouse(centre);
    client.click(MouseButton::Left);
    client.frames(3);
    assert!(
        !client
            .world()
            .non_send::<super::PostGame>()
            .overlay
            .as_ref()
            .expect("modal")
            .picker,
        "outside click dismisses the picker"
    );
    let focus_slot = client
        .world()
        .non_send::<super::PostGame>()
        .summary()
        .expect("summary")
        .rows[0]
        .slot;
    result_click(&mut client, "results graph picker");
    result_click(&mut client, "results graph team advantage");
    result_click(&mut client, &format!("results focus {}", focus_slot.0));
    let focused = {
        let post = client.world().non_send::<super::PostGame>();
        let view = post.overlay.as_ref().expect("modal");
        (view.metric, view.focus)
    };
    result_click(&mut client, "results graph picker");
    result_click(&mut client, "results graph team advantage");
    let cleared = client
        .world()
        .non_send::<super::PostGame>()
        .overlay
        .as_ref()
        .expect("modal")
        .focus;
    assert_eq!(
        (focused, cleared),
        ((Metric::NetWorth, Some(focus_slot)), None),
        "player focus opens net worth from advantage; returning to advantage clears it"
    );
    result_click(&mut client, "results scoreboard");
    let slot = client
        .world()
        .non_send::<super::PostGame>()
        .summary()
        .expect("summary")
        .rows[0]
        .slot;
    result_click(&mut client, &format!("results player {}", slot.0));
    assert_eq!(
        client
            .world()
            .non_send::<super::PostGame>()
            .overlay
            .as_ref()
            .expect("modal")
            .player,
        Some(slot)
    );
    let texts = client.shown_texts();
    assert!(texts.iter().any(|text| text == "OVER THE MATCH"));
    result_click(&mut client, "results ← scoreboard");
    assert!(
        client
            .world()
            .non_send::<super::PostGame>()
            .overlay
            .as_ref()
            .expect("modal")
            .player
            .is_none()
    );
    assert_eq!(
        client.tick(),
        tick,
        "inspection never seeks or restarts the game"
    );
}

#[test]
fn the_approved_draw_and_player_pages_keep_the_name_prominent_and_timeline_absent() {
    use crate::e2e::harness::Client;
    use bevy::text::{FontSize, TextLayoutInfo};
    for name in [
        "match_details_solo_draw",
        "match_player",
        "match_details_player_bot",
    ] {
        let mut client = Client::run(&["--menu-preview", name, "--window", "1440x900"]);
        client.until("result name shaped", |client| {
            let world = client.world_mut();
            world
                .query::<(&Text, &TextLayoutInfo)>()
                .iter(world)
                .any(|(text, layout)| {
                    matches!(text.0.as_str(), "mira#2207" | "Thistle") && !layout.glyphs.is_empty()
                })
        });
        client.frames(4);
        let post = client.world().non_send::<super::PostGame>();
        let view = post.overlay.as_ref().expect("result modal");
        if name.ends_with("draw") {
            assert_eq!(view.page, super::view::Page::Mvp);
            assert_eq!(view.summary.winner, Some(Team::Neutral));
        }
        let expected = if name.contains("bot") {
            "Thistle"
        } else {
            "mira#2207"
        };
        let world = client.world_mut();
        let (font, node, layout, place) = world
            .query::<(
                &Text,
                &TextFont,
                &ComputedNode,
                &TextLayoutInfo,
                &UiGlobalTransform,
            )>()
            .iter(world)
            .find(|(text, font, _, _, _)| {
                text.0 == expected && matches!(font.font_size,FontSize::Px(size)if size>=28.0)
            })
            .map(|(_, font, node, layout, place)| (font, node, layout, place))
            .expect("large player name");
        let FontSize::Px(size) = font.font_size else {
            panic!("pixel role");
        };
        assert!(size >= 28.0);
        if name.ends_with("draw") {
            assert!(
                (place.translation.x - 720.0).abs() < 24.0,
                "MVP name is centered on the window"
            );
            let left = layout
                .glyphs
                .iter()
                .map(|glyph| glyph.position.x)
                .fold(f32::INFINITY, f32::min);
            let right = layout
                .glyphs
                .iter()
                .map(|glyph| glyph.position.x)
                .fold(f32::NEG_INFINITY, f32::max);
            assert!(
                ((left + right) / 2.0 - node.size().x / 2.0).abs() < 24.0,
                "MVP name is visibly centered"
            );
        }
        assert!(!client.shown_texts().iter().any(|text| text == "TIMELINE"));
    }
}

#[test]
fn a_replay_viewer_without_a_dashboard_keeps_the_game_hud_scale() {
    let played = play();
    let mut client = crate::e2e::harness::Client::run(&[
        "--replay",
        played.replay.to_str().expect("UTF-8"),
        "--window",
        "1440x900",
    ]);
    client
        .world_mut()
        .remove_non_send::<crate::main_menu::MenuCore>();
    client.until(
        "the replay finishes loading without a dashboard",
        |client| client.world().resource::<crate::loading::Loading>().done(),
    );
    client.frames(3);
    assert!(!client.world().non_send::<super::PostGame>().visible());
    assert!(
        client.world().resource::<crate::ui::HudShown>().0,
        "the browser replay page has no dashboard"
    );
    assert!(
        client.world().resource::<UiScale>().0 < 0.99,
        "game HUD fit is preserved without a menu core"
    );
}

/// The same real recording drives history contracts.
pub(crate) fn recorded_match_for_menu() -> (PathBuf, PathBuf) {
    (play().replay.clone(), play().meta.clone())
}

fn incomplete_roster_client() -> crate::e2e::harness::Client {
    let mut client = crate::e2e::harness::Client::run(&["--menu-preview", "match_scoreboard"]);
    client.frames(4);
    let mut meta =
        bota_replay::meta::decode(&super::preview::metadata("match_details")).expect("fixture");
    meta.records.retain(|record| {
        !matches!(
            record,
            bota_replay::meta::MetaRecord::Sample { slot: 5, .. }
        )
    });
    let summary = std::sync::Arc::new(MatchSummary::from_meta(&meta).expect("remaining seats"));
    {
        let mut post = client.world_mut().non_send_mut::<super::PostGame>();
        post.overlay = Some(super::view::View::new(summary, Default::default()));
        post.presented += 1;
    }
    client.frames(4);
    client
}
#[test]
fn an_incomplete_roster_selects_the_available_scoreboard_page() {
    let mut client = incomplete_roster_client();
    let world = client.world_mut();
    let border = world
        .query::<(&Name, &BorderColor)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "results scoreboard")
        .map(|(_, border)| border.top)
        .expect("scoreboard pager");
    assert_eq!(
        border,
        crate::theme::INK_3,
        "the displayed scoreboard is the selected page"
    );
    assert!(
        !world
            .query::<&Name>()
            .iter(world)
            .any(|name| name.as_str() == "results mvp")
    );
}
#[test]
fn an_incomplete_roster_never_presents_a_partial_score_as_the_team_total() {
    let mut client = incomplete_roster_client();
    let world = client.world_mut();
    let score = world
        .query::<(&Name, &Text)>()
        .iter(world)
        .find(|(name, _)| name.as_str() == "results radiant score")
        .map(|(_, text)| text.0.as_str())
        .expect("score");
    assert_eq!(score, "—", "a missing seat makes the aggregate unknown");
}

fn team_card(client: &mut crate::e2e::harness::Client, slot: u8) -> (Entity, Entity, Rect) {
    let world = client.world_mut();
    let name = format!("results card {slot}");
    let card = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, label)| label.as_str() == name)
        .map(|(entity, _)| entity)
        .expect("team card");
    let frame = world
        .query_filtered::<Entity, With<super::render::Portrait>>()
        .iter(world)
        .find(|entity| crate::e2e::harness::descends_from(world, *entity, card))
        .expect("portrait");
    let place = world
        .get::<UiGlobalTransform>(frame)
        .expect("placed")
        .translation;
    let size = world.get::<ComputedNode>(frame).expect("laid out").size();
    (card, frame, Rect::from_center_size(place, size))
}
#[test]
fn hovering_a_team_card_highlights_the_portrait_and_shows_details() {
    let mut client = crate::e2e::harness::Client::run(&["--menu-preview", "match_dire"]);
    client.frames(5);
    let (card, frame, _) = team_card(&mut client, 7);
    let original = client.world().get::<BorderColor>(frame).expect("edge").top;
    client.hover_ui(card, std::time::Duration::from_secs(20));
    client.frames(3);
    assert_eq!(
        client.world().get::<BorderColor>(frame).expect("edge").top,
        crate::theme::INK_2,
        "the actual hover highlights the frame"
    );
    let world = client.world_mut();
    assert!(
        world
            .query::<(&super::render::hover::DetailsHint, &InheritedVisibility)>()
            .iter(world)
            .any(|(hint, visible)| hint.button == card && visible.get()),
        "the card's hint is visible"
    );
    client.mouse(bevy::prelude::Vec2::new(10.0, 10.0));
    client.frames(3);
    assert_eq!(
        client.world().get::<BorderColor>(frame).expect("edge").top,
        original
    );
}
#[test]
fn team_card_statistics_do_not_paint_into_the_player_name_below_the_portrait() {
    use bevy::ui::CalculatedClip;
    let mut client = crate::e2e::harness::Client::run(&["--menu-preview", "match_radiant"]);
    client.until("wrapped card label laid out", |client| {
        let world = client.world_mut();
        world
            .query::<(&Text, &ComputedNode)>()
            .iter(world)
            .any(|(text, node)| text.0 == "THIRD MOST BUILDING DAMAGE" && node.size().y > 0.0)
    });
    client.frames(3);
    let (_, _, frame) = team_card(&mut client, 4);
    let world = client.world_mut();
    let (place, node, clip) = world
        .query::<(
            &Text,
            &UiGlobalTransform,
            &ComputedNode,
            Option<&CalculatedClip>,
        )>()
        .iter(world)
        .find(|(text, _, _, _)| text.0 == "THIRD MOST BUILDING DAMAGE")
        .map(|(_, place, node, clip)| (place, node, clip))
        .expect("label");
    let label = Rect::from_center_size(place.translation, node.size());
    if label.max.y > frame.max.y + 1.0 {
        let outside = bevy::prelude::Vec2::new(label.center().x, frame.max.y + 1.0);
        assert!(
            clip.is_some_and(|clip| !clip.contains_point(outside)),
            "text below the portrait is clipped rather than covering the name"
        );
    }
}

#[test]
fn graph_strokes_distinguish_team_totals_and_a_focused_player() {
    use crate::e2e::harness::Client;
    let mut mismatches = Vec::new();
    for preview in [
        "match_graph_team_net_worth",
        "match_graph_team_kills",
        "match_details_graphs_focus_hover",
    ] {
        let mut client = Client::run(&["--menu-preview", preview]);
        client.until("recorded curves are drawn", |client| {
            let world = client.world_mut();
            world
                .query::<&super::render::Segment>()
                .iter(world)
                .next()
                .is_some()
        });
        let world = client.world_mut();
        let widths: Vec<_> = world
            .query::<(&super::render::Segment, &Node)>()
            .iter(world)
            .filter_map(|(_, node)| match node.height {
                Val::Px(width) => Some(width),
                _ => None,
            })
            .collect();
        let correct = if preview.contains("focus") {
            widths.contains(&1.4)
                && widths.contains(&3.5)
                && widths.iter().all(|width| [1.4, 3.5].contains(width))
        } else {
            widths.iter().all(|width| *width == 3.0)
        };
        if !correct {
            mismatches.push(preview);
        }
    }
    assert!(
        mismatches.is_empty(),
        "graph strokes differ from their roles: {mismatches:?}"
    );
}

#[test]
fn focusing_a_graph_player_dims_the_other_portraits() {
    use crate::e2e::harness::Client;
    let mut client = Client::run(&["--menu-preview", "match_details_graphs_focus_hover"]);
    client.until("the graph portraits are drawn", |client| {
        let world = client.world_mut();
        world
            .query::<&super::render::Segment>()
            .iter(world)
            .next()
            .is_some()
    });
    let world = client.world_mut();
    let portraits: Vec<_> = world
        .query::<(&ChildOf, &ImageNode)>()
        .iter(world)
        .filter_map(|(parent, image)| {
            let name = world.get::<Name>(parent.parent())?;
            name.as_str()
                .starts_with("results focus ")
                .then_some((name.as_str().to_owned(), image.color.alpha()))
        })
        .collect();
    assert_eq!(portraits.len(), 10, "all recorded players have a portrait");
    for (name, alpha) in portraits {
        assert_eq!(
            alpha,
            if name == "results focus 5" { 1.0 } else { 0.55 },
            "{name} respects the selected player"
        );
    }
}

#[test]
fn metadata_without_earned_gold_keeps_gpm_unknown_even_without_timed_samples() {
    use crate::e2e::harness::Client;
    use bota_replay::meta::{MetaRecord, stat};
    let mut meta =
        bota_replay::meta::decode(&super::preview::metadata("match_details")).expect("fixture");
    for record in &mut meta.records {
        if let MetaRecord::Sample { stats, .. } = record {
            stats.retain(|(id, _)| *id != stat::GOLD_EARNED);
        }
    }
    let mut summary = MatchSummary::from_meta(&meta).expect("summary");
    // from_meta always retains timed samples: this deliberately pins the legacy/unreachable-state guard.
    summary.recording = None;
    let mut client = Client::run(&[]);
    client.frames(3);
    {
        let mut post = client.world_mut().non_send_mut::<super::PostGame>();
        post.overlay = Some(super::view::View::new(
            std::sync::Arc::new(summary),
            super::view::Context {
                history: true,
                ..Default::default()
            },
        ));
        post.presented += 1;
    }
    client.frames(4);
    client.click_named("results scoreboard");
    let world = client.world_mut();
    let rates: Vec<_> = world
        .query::<(&super::render::Cell, &Text)>()
        .iter(world)
        .filter(|(cell, _)| **cell == super::render::Cell::Gpm)
        .map(|(_, text)| text.0.clone())
        .collect();
    assert_eq!(
        rates,
        vec!["—"; 10],
        "net worth never supplies missing earned gold"
    );
}

#[test]
fn missing_and_known_experience_render_distinct_xpm_with_and_without_timed_samples() {
    use crate::e2e::harness::Client;
    use bota_replay::meta::{MetaRecord, stat};
    for (xp, expected) in [(None, "—"), (Some(0), "0"), (Some(1200), "1200")] {
        let mut meta =
            bota_replay::meta::decode(&super::preview::metadata("match_details")).expect("fixture");
        meta.header.pregame_ticks = 0;
        for record in &mut meta.records {
            match record {
                MetaRecord::Sample { tick, stats, .. } => {
                    *tick = 60 * u32::from(meta.header.tick_rate);
                    stats.retain(|(id, _)| *id != stat::XP);
                    if let Some(xp) = xp {
                        stats.push((stat::XP, xp));
                    }
                }
                MetaRecord::End { tick, .. } => *tick = 60 * u32::from(meta.header.tick_rate),
                _ => {}
            }
        }
        for timed in [true, false] {
            let mut summary = MatchSummary::from_meta(&meta).expect("summary");
            // The false case is an explicit guard: from_meta itself never drops this data.
            if !timed {
                summary.recording = None;
            }
            let mut client = Client::run(&[]);
            client.frames(3);
            {
                let mut post = client.world_mut().non_send_mut::<super::PostGame>();
                post.overlay = Some(super::view::View::new(
                    std::sync::Arc::new(summary),
                    super::view::Context {
                        history: true,
                        ..Default::default()
                    },
                ));
                post.presented += 1;
            }
            client.frames(4);
            client.click_named("results scoreboard");
            let world = client.world_mut();
            let rates: Vec<_> = world
                .query::<(&super::render::Cell, &Text)>()
                .iter(world)
                .filter(|(cell, _)| **cell == super::render::Cell::Xpm)
                .map(|(_, text)| text.0.clone())
                .collect();
            assert_eq!(
                rates,
                vec![expected; 10],
                "XP={xp:?}, timed={timed}: missing is distinct from a recorded zero or a known one-minute total"
            );
        }
    }
}

#[test]
fn old_and_hostile_replay_provenance_keeps_playback_without_automatic_profile_connections() {
    use crate::e2e::harness::Client;
    use bevy::prelude::*;
    let played = play();
    let original = std::fs::read(&played.replay).expect("recorded replay");
    let carried = bota_client_core::meta_in_replay(&mut std::io::Cursor::new(&original))
        .expect("read")
        .expect("trailer");
    let meta = bota_replay::meta::decode(&carried).expect("metadata");
    let mut writer = bota_replay::meta::Writer::default();
    assert!(writer.push(&bota_replay::meta::MetaRecord::Header(meta.header), false));
    for record in meta
        .records
        .iter()
        .filter(|record| !matches!(record, bota_replay::meta::MetaRecord::Profiles { .. }))
    {
        assert!(writer.push(record, true));
    }
    let legacy = writer.into_bytes();
    let stream = &original[..original.len() - carried.len() - bota_replay::trailer::OVERHEAD];
    let trap = std::net::TcpListener::bind("127.0.0.1:0").expect("foreign service trap");
    trap.set_nonblocking(true).expect("nonblocking");
    let private_uri = format!("ws://{}/control", trap.local_addr().expect("address"));
    let uri = "wss://foreign.example/control";
    let claim = |uri: &str| {
        bota_proto::encode_frame_to_vec(&(13_u8, Some(uri), vec![(0_u8, Some(42_u64))]))
            .expect("provenance frame")
    };
    for (label, tail, link) in [
        ("old", None, false),
        (
            "corrupt",
            Some(claim("wss://user:secret@example.org")),
            false,
        ),
        ("private", Some(claim(&private_uri)), false),
        ("foreign", Some(claim(uri)), true),
    ] {
        let file = scratch(&format!("profile-{label}.brp"));
        let mut bytes = stream.to_vec();
        if let Some(tail) = tail {
            bytes.extend(
                bota_replay::trailer::frame(&[legacy.clone(), tail].concat())
                    .expect("metadata trailer"),
            );
        }
        std::fs::write(&file, bytes).expect("replay fixture");
        let mut client = Client::run(&["--replay", file.to_str().expect("UTF-8")]);
        client.until(
            "the legacy replay ends and its statistics are readable",
            |client| {
                if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
                    player.advance_ticks(1.0e7);
                }
                client.world().non_send::<super::PostGame>().visible()
            },
        );
        client.click_named("results scoreboard");
        let world = client.world_mut();
        let links = world
            .query_filtered::<&Name, With<bevy::ui_widgets::Button>>()
            .iter(world)
            .filter(|name| name.as_str().starts_with("results profile "))
            .count();
        assert_eq!(
            links,
            usize::from(link),
            "old and corrupt files have no profile link"
        );
        if link {
            client.click_named("results profile 0");
            assert!(
                client.shown_texts().iter().any(|text| text.contains(uri)),
                "the foreign identity is plainly visible"
            );
        }
        client.frames(5);
        assert!(
            matches!(client.core().source, bota_client_core::Source::Replay(_)),
            "profile inspection never switches the game source"
        );
        assert!(
            matches!(trap.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock),
            "untrusted replay metadata never opens a connection"
        );
        drop(client);
        std::fs::remove_file(file).expect("remove replay fixture");
    }
}
