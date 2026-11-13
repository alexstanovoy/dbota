//! Synthetic recordings for visual checks. Runtime results never derive curves from totals.

use super::{
    graphs::Metric,
    summary::{Detail, MatchSummary},
    view::{Context, Kind, Page, Replay, View},
};
use bota_proto::{HeroId, ItemId, SlotId, UnitKind};
use bota_replay::meta::{MetaHeader, MetaRecord, Who, Writer, stat};
use serde::Deserialize;
use std::{
    collections::HashMap,
    sync::{Arc, OnceLock},
};

pub const NAMES: &[&str] = &[
    "match_details",
    "match_details_foreign_profile",
    "match_details_abandoned",
    "match_details_board_hover",
    "match_details_dire_hover",
    "match_details_graphs_focus_hover",
    "match_details_graphs_hover",
    "match_details_graphs_menu",
    "match_details_left_board",
    "match_details_noreplay",
    "match_details_player_bot",
    "match_details_solo",
    "match_details_solo_board",
    "match_details_solo_draw",
    "match_details_solo_graphs",
    "match_details_solo_radiant",
    "match_details_statsonly_postgame",
    "match_dire",
    "match_graphs",
    "match_history",
    "match_player",
    "match_postgame",
    "match_radiant",
    "match_scoreboard",
    "match_graph_assists",
    "match_graph_building_damage",
    "match_graph_deaths",
    "match_graph_denies",
    "match_graph_experience",
    "match_graph_healing",
    "match_graph_hero_damage",
    "match_graph_kills",
    "match_graph_last_hits",
    "match_graph_level",
    "match_graph_net_worth",
    "match_graph_team_kills",
    "match_graph_team_net_worth",
];

#[derive(Deserialize)]
struct Player {
    slot: u8,
    team: u8,
    name: String,
    bot: bool,
    hero: String,
    gpm: i64,
    xpm: i64,
    items: Vec<Option<String>>,
    backpack: Vec<Option<String>>,
    purchases: Vec<(String, i64)>,
}
#[derive(Deserialize)]
struct Kill {
    second: u32,
    killer: Option<u8>,
    victim: u8,
    gold: i32,
}
#[derive(Deserialize)]
struct Building {
    second: u32,
    kind: String,
    team: u8,
}
#[derive(Deserialize)]
struct Levels {
    slot: u8,
    times: Vec<u32>,
}
#[derive(Deserialize)]
struct Leaver {
    slot: u8,
    at: u32,
}
#[derive(Deserialize)]
struct Template {
    seconds: u32,
    winner: Option<u8>,
    reason: String,
    id: u64,
    mode: String,
    when: String,
    rating: [i32; 2],
    players: Vec<Player>,
    samples: Vec<[i64; 13]>,
    kills: Vec<Kill>,
    structures: Vec<Building>,
    levels: Vec<Levels>,
    leaver: Option<Leaver>,
}

fn templates() -> &'static HashMap<String, Template> {
    static TEMPLATES: OnceLock<HashMap<String, Template>> = OnceLock::new();
    TEMPLATES.get_or_init(|| {
        serde_json::from_str(include_str!("preview_data.json"))
            .expect("the synthetic visual-check recordings")
    })
}

fn template(name: &str) -> &'static Template {
    let key = if name.contains("abandoned") {
        "abandoned"
    } else if name.contains("left_board") {
        "left"
    } else if name.contains("solo_draw") {
        "draw"
    } else if name.contains("solo") {
        "solo"
    } else {
        "full"
    };
    &templates()[key]
}

pub fn metadata(name: &str) -> Vec<u8> {
    let template = template(name);
    let mut writer = Writer::default();
    let header = MetaHeader {
        magic: MetaHeader::MAGIC,
        schema: MetaHeader::SCHEMA,
        wire_fingerprint: bota_core::WIRE_FINGERPRINT,
        rules_version: 0,
        match_id: template.id,
        map: if template.players.len() == 2 { 2 } else { 0 },
        tick_rate: 30,
        pregame_ticks: 2700,
        catalogue_items: bota_core::Catalogue::FULL.items(),
        catalogue_digest: bota_core::Catalogue::FULL.prefix().digest,
    };
    assert!(writer.push(&MetaRecord::Header(header), false));
    for player in &template.players {
        let hero = if player.hero == "Pudge" {
            HeroId::PUDGE
        } else {
            HeroId::SHADOW_FIEND
        };
        assert!(writer.push(
            &MetaRecord::Seat {
                slot: player.slot,
                team: player.team,
                hero: hero.0
            },
            false
        ));
        assert!(writer.push(
            &MetaRecord::SeatName {
                slot: player.slot,
                name: player.name.clone(),
                bot: player.bot
            },
            false
        ));
    }
    samples(&mut writer, template);
    events(&mut writer, template);
    for player in &template.players {
        let items = player
            .items
            .iter()
            .chain(&player.backpack)
            .map(|item| item.as_deref().and_then(item_id).map(|item| item.0))
            .collect();
        assert!(writer.push(
            &MetaRecord::Inventory {
                tick: tick(template.seconds),
                slot: player.slot,
                items
            },
            false
        ));
    }
    if let Some(leaver) = &template.leaver {
        assert!(writer.push(
            &MetaRecord::Leave {
                tick: tick(leaver.at),
                slot: leaver.slot
            },
            false
        ));
    }
    if template.reason != "ABANDONED" {
        assert!(writer.push(
            &MetaRecord::End {
                tick: tick(template.seconds),
                winner: template.winner
            },
            true
        ));
    }
    writer.into_bytes()
}

fn samples(writer: &mut Writer, template: &Template) {
    let ids = [
        stat::NET_WORTH,
        stat::XP,
        stat::LEVEL,
        stat::LAST_HITS,
        stat::DENIES,
        stat::KILLS,
        stat::DEATHS,
        stat::ASSISTS,
        stat::HERO_DAMAGE,
        stat::STRUCTURE_DAMAGE,
        stat::HEALING,
    ];
    for sample in &template.samples {
        let second = sample[0] as u32;
        let mut stats: Vec<_> = ids.into_iter().zip(sample[2..].iter().copied()).collect();
        if second == template.seconds {
            let player = &template.players[sample[1] as usize];
            stats.push((
                stat::GOLD_EARNED,
                (player.gpm * i64::from(second) + 59) / 60,
            ));
            if let Some((_, xp)) = stats.iter_mut().find(|(id, _)| *id == stat::XP) {
                *xp = (player.xpm * i64::from(second) + 59) / 60;
            }
        }
        assert!(writer.push(
            &MetaRecord::Sample {
                tick: tick(second),
                slot: sample[1] as u8,
                stats
            },
            false
        ));
    }
}

fn events(writer: &mut Writer, template: &Template) {
    for kill in &template.kills {
        assert!(writer.push(
            &MetaRecord::Kill {
                tick: tick(kill.second),
                victim: Who::Hero(kill.victim),
                killer: kill.killer.map(Who::Hero),
                gold: kill.gold
            },
            false
        ));
    }
    for building in &template.structures {
        let kind = match building.kind.as_str() {
            "Ancient" => UnitKind::Ancient,
            "Barracks" => UnitKind::Barracks,
            _ => UnitKind::Tower,
        };
        assert!(writer.push(
            &MetaRecord::Structure {
                tick: tick(building.second),
                kind: kind as u8,
                team: building.team
            },
            false
        ));
    }
    for levels in &template.levels {
        for (index, second) in levels.times.iter().enumerate() {
            assert!(writer.push(
                &MetaRecord::LevelUp {
                    tick: tick(*second),
                    slot: levels.slot,
                    level: index as u8 + 2
                },
                false
            ));
        }
    }
    for player in &template.players {
        for (item, second) in &player.purchases {
            if *second > i64::from(template.seconds) {
                continue;
            }
            if let Some(item) = item_id(item) {
                assert!(writer.push(
                    &MetaRecord::Purchase {
                        tick: (2700_i64 + second * 30).max(0) as u32,
                        slot: player.slot,
                        item: item.0
                    },
                    false
                ));
            }
        }
    }
}

fn tick(second: u32) -> u32 {
    2700 + second * 30
}

fn item_id(name: &str) -> Option<ItemId> {
    bota_core::ITEMS
        .iter()
        .find(|item| item.key.strip_prefix("item_") == Some(name))
        .map(|item| item.id)
}

pub fn view(name: &str) -> Option<View> {
    if !NAMES.contains(&name) || name == "match_history" {
        return None;
    }
    let template = template(name);
    let meta = bota_replay::meta::decode(&metadata(name)).expect("preview recording decodes");
    let mut summary = MatchSummary::from_meta(&meta).expect("preview recording has samples");
    let stats_only = name.contains("statsonly");
    if stats_only {
        summary.detail = Detail::Stats;
        summary.recording = None;
        summary.points.clear();
        for row in &mut summary.rows {
            row.net_worth = None;
            row.healing = None;
            row.items = [None; super::summary::BAG];
        }
    }
    let context = Context {
        match_id: (!stats_only).then_some(template.id),
        mode: template.mode.clone(),
        when: template.when.clone(),
        kind: if stats_only {
            Kind::Direct
        } else {
            Kind::Ranked
        },
        rating: (!stats_only).then_some((
            template.rating[0],
            if template.reason == "ABANDONED" {
                template.rating[0]
            } else {
                template.rating[1]
            },
        )),
        reason: template.reason.clone(),
        owner: Some(SlotId(0)),
        history: !name.contains("postgame"),
        replay: (!stats_only && !name.contains("noreplay"))
            .then(|| Replay::Saved("preview.brp".to_owned())),
        ..Default::default()
    };
    let mut view = View::new(Arc::new(summary), context);
    if name == "match_details_foreign_profile" {
        view.page = Page::Scoreboard;
        view.notice = Some(format!(
            "The file claims this profile is at wss://match.example/{}. This claim is not verified. Select that matchmaker explicitly to view it.",
            "public_namespace/".repeat(20)
        ));
    }

    if !stats_only {
        view.illustrated_items = template
            .players
            .iter()
            .map(|player| super::view::IllustratedInventory {
                slot: SlotId(player.slot),
                items: player
                    .items
                    .iter()
                    .chain(&player.backpack)
                    .cloned()
                    .collect(),
                purchases: player.purchases.clone(),
            })
            .collect();
    }
    if name.contains("board") || name.contains("scoreboard") || name.contains("abandoned") {
        view.page = Page::Scoreboard;
    }
    if name.contains("radiant") {
        view.page = Page::Radiant;
    }
    if name == "match_dire" || name.contains("dire_hover") {
        view.page = Page::Dire;
    }
    if name.contains("graph") {
        view.page = Page::Graphs;
        view.metric = metric(name);
    }
    if name.contains("graphs_menu") {
        view.picker = true;
    }
    if name.contains("focus_hover") {
        view.metric = Metric::LastHits;
        view.focus = Some(SlotId(5));
        view.hover_second = Some(1800);
    } else if name.contains("graphs_hover") {
        view.hover_second = Some(1440);
    }
    if name == "match_player" || name.contains("player_bot") {
        view.page = Page::Scoreboard;
        view.player = Some(SlotId(if name.contains("bot") { 7 } else { 0 }));
    }
    if name.contains("dire_hover") {
        view.hover_player = Some(SlotId(7));
    }
    if name.contains("board_hover") {
        view.hover_player = Some(SlotId(0));
    }
    Some(view)
}

fn metric(name: &str) -> Metric {
    if name.contains("team_net_worth") {
        return Metric::TeamNetWorth;
    }
    if name.contains("team_kills") {
        return Metric::TeamKills;
    }
    [
        ("net_worth", Metric::NetWorth),
        ("experience", Metric::Experience),
        ("level", Metric::Level),
        ("last_hits", Metric::LastHits),
        ("denies", Metric::Denies),
        ("kills", Metric::Kills),
        ("deaths", Metric::Deaths),
        ("assists", Metric::Assists),
        ("hero_damage", Metric::HeroDamage),
        ("building_damage", Metric::BuildingDamage),
        ("healing", Metric::Healing),
    ]
    .into_iter()
    .find(|(key, _)| name.ends_with(key))
    .map_or(Metric::Advantage, |(_, metric)| metric)
}
