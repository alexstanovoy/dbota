//! Tiered lane bodies use the install's model and scale through the real drawing path.
use bevy::prelude::*;
use bevy::world_serialization::WorldAssetRoot;
use bota_proto::{CreepTier, EntityId, HeroId, Team, UnitKind, Vec2 as Spot};

use super::creep_walk::{ControlledFrames, tell};
use super::harness::{Client, Server};
use crate::units::{ModelBank, ModelOf, Rig, Unit};

struct CreepCase {
    team: Team,
    kind: UnitKind,
    tier: CreepTier,
    npc: String,
    logical: String,
}

fn creep_cases() -> Vec<CreepCase> {
    let mut cases = Vec::new();
    for (team, faction, side) in [
        (Team::Radiant, "goodguys", "radiant"),
        (Team::Dire, "badguys", "dire"),
    ] {
        for (kind, name) in [
            (UnitKind::CreepMelee, "melee"),
            (UnitKind::CreepRanged, "ranged"),
        ] {
            for (tier, suffix) in [(CreepTier::Super, ""), (CreepTier::Mega, "_mega")] {
                cases.push(CreepCase {
                    team,
                    kind,
                    tier,
                    npc: format!("npc_dota_creep_{faction}_{name}_upgraded{suffix}"),
                    logical: format!("creep.{side}.{name}{suffix}.model"),
                });
            }
        }
    }
    cases
}

fn show_creep(client: &mut Client, case: &CreepCase, index: usize) -> EntityId {
    let mut view = client.me().clone();
    view.id = EntityId {
        idx: 50_000 + index as u32,
        generation: 1,
    };
    view.kind = case.kind;
    view.team = case.team;
    view.hero = None;
    view.owner = None;
    view.cast = None;
    view.cast_rates = None;
    view.forced = None;
    view.channel = None;
    view.town_portal = None;
    view.queue.clear();
    view.creep_tier = Some(case.tier);
    view.abilities.clear();
    view.items.clear();
    view.effects.clear();
    view.pos += Spot::from_ints(100, 0);
    let id = view.id;
    tell(client, view);
    client.until("the tiered creep model is drawn", |c| {
        c.body(id)
            .is_some_and(|body| c.world().get::<Rig>(body).is_some())
    });
    id
}

fn script_text<'a>(row: &'a heatel::keyvalues::Kv, field: &str) -> &'a str {
    let Some(heatel::keyvalues::Kv::Text(value)) = row.get(field) else {
        panic!("missing {field} in creep script")
    };
    value
}

fn assert_creep_model(
    client: &Client,
    case: &CreepCase,
    id: EntityId,
    script: &heatel::keyvalues::Kv,
    catalog: &dbota_import::catalog::Catalog,
) {
    let world = client.world();
    let body = client.body(id).expect("creep body");
    let unit = world.get::<Unit>(body).expect("drawn unit");
    let bank = world.resource::<ModelBank>();
    let model = &bank.0[unit.model.expect("model")].model;
    assert_eq!(model.logical, case.logical);
    assert_creep_fx(client, case, id);
    let entry = catalog
        .lines
        .iter()
        .find_map(|line| match line {
            dbota_import::catalog::Line::Model(entry) if entry.logical == model.logical => {
                Some(entry)
            }
            _ => None,
        })
        .expect("imported model source");
    assert_eq!(
        entry.src,
        format!("{}_c", script_text(script, "Model")),
        "{} source model",
        case.npc
    );
    assert_eq!(
        model.path,
        format!("cache://{}", entry.out),
        "the drawn asset is the imported source model"
    );
    let pivot = world.get::<ModelOf>(body).expect("model pivot").0;
    let scene = world
        .get::<Children>(pivot)
        .expect("scene children")
        .iter()
        .find(|child| world.get::<WorldAssetRoot>(*child).is_some())
        .expect("drawn scene");
    let scale = world
        .get::<Transform>(scene)
        .expect("scene transform")
        .scale
        .x
        / crate::map::UNITS_PER_METRE;
    let expected: f32 = script_text(script, "ModelScale")
        .parse()
        .expect("installed scale");
    assert!(
        (scale - expected).abs() < 0.0001,
        "{}: scale {scale}, installed {expected}",
        case.npc
    );
}

#[test]
fn super_and_mega_creeps_draw_their_installed_models_and_scales() {
    use heatel::keyvalues::{parse_kv, text};
    let source = heatel::DirSource::new(super::harness::game_dir());
    let package = heatel::Vpk::open(&source, "pak01_dir.vpk").expect("install package");
    let bytes = package
        .read_file(&source, "scripts/npc/npc_units.txt")
        .expect("unit scripts");
    let scripts = parse_kv(text(&bytes).expect("script text")).expect("unit rows");
    let units = scripts.get("DOTAUnits").expect("unit table");
    let catalog =
        dbota_import::catalog::parse(dbota_import::import::MANIFEST, dbota_import::import::IDS)
            .expect("manifest");
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    server.frozen(|| {
        let _live = std::mem::replace(
            &mut client.core_mut().source,
            bota_client_core::Source::Live(Box::new(ControlledFrames)),
        );
        for (index, case) in creep_cases().into_iter().enumerate() {
            let id = show_creep(&mut client, &case, index);
            assert_creep_model(
                &client,
                &case,
                id,
                units.get(&case.npc).expect("installed creep"),
                &catalog,
            );
        }
    });
}

#[test]
fn mega_creeps_queue_the_side_appropriate_installed_announcement() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let mine = client.me().team;
    let theirs = if mine == Team::Radiant {
        Team::Dire
    } else {
        Team::Radiant
    };
    client.core_mut().told = vec![
        bota_proto::EventKind::MegaCreeps { team: mine },
        bota_proto::EventKind::MegaCreeps { team: theirs },
    ];
    let (spoken, loaded) = crate::voice::announce_buildings_once(client.world_mut());
    assert_eq!(spoken, 2, "each new mega side queues one announcer line");
    assert_eq!(
        loaded, 40,
        "36 building lines and four mega variants from the install"
    );
    let heard = &client.world().resource::<crate::voice::BuildingsHeard>().0;
    assert_eq!(
        heard,
        &["sound.announcer.mega.you", "sound.announcer.mega.enemy"]
    );
}

struct Frames(Vec<bota_proto::ServerMsg>);
impl bota_client_core::LiveLink for Frames {
    fn poll(&mut self) -> Vec<bota_proto::ServerMsg> {
        std::mem::take(&mut self.0)
    }
    fn send(&mut self, _: &bota_proto::ClientMsg) -> std::io::Result<()> {
        Ok(())
    }
    fn is_closed(&self) -> bool {
        false
    }
}

#[test]
fn mega_creep_events_sound_the_global_stinger_and_the_enemy_ancient_layer() {
    use bota_client_core::CueKind;
    use bota_proto::{EventKind, ServerMsg};
    let server = Server::start(1, &["--map", "0"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(bota_proto::Cheat::Invulnerable { on: true });
    let ancient = bota_core::rules::DIRE_ANCIENT_POS;
    client.stand_at((ancient.x.to_int() - 300, ancient.y.to_int()));
    server.frozen(|| {
        assert_public_horn_anchor(&mut client, ancient);
        let mut view = client.view().clone();
        view.tick += 1;
        let tick = view.tick;
        let _live = std::mem::replace(
            &mut client.core_mut().source,
            bota_client_core::Source::Live(Box::new(Frames(vec![
                ServerMsg::Snapshot { view },
                ServerMsg::Events {
                    tick,
                    events: vec![EventKind::MegaCreeps {
                        team: Team::Radiant,
                    }],
                },
            ]))),
        );
        client.until("both installed mega stinger layers sound", |c| {
            crate::sound::cues_sounded(c.world(), CueKind::MegaCreeps(Team::Radiant)) == 1
                && crate::sound::cues_sounded(c.world(), CueKind::MegaCreepsHorn(Team::Dire)) == 1
        });
        assert_eq!(
            crate::sound::cues_sounded(client.world(), CueKind::MegaCreepsHorn(Team::Radiant)),
            0
        );
    });
}

fn assert_public_horn_anchor(client: &mut Client, ancient: Spot) {
    use bevy::ecs::system::RunSystemOnce;
    let hero = client.me().clone();
    client
        .world_mut()
        .run_system_once(move |sight: crate::fog::EventSight| {
            let event = bota_proto::EventKind::MegaCreeps {
                team: Team::Radiant,
            };
            assert!(sight.allows(&event, crate::fog::EventSubject::Event));
            assert!(sight.allows(&event, crate::fog::EventSubject::Point(ancient)));
            assert!(!sight.allows(
                &event,
                crate::fog::EventSubject::Point(ancient + Spot::from_ints(1, 0))
            ));
            assert!(!sight.allows(&event, crate::fog::EventSubject::Unit(&hero)));
        })
        .expect("public horn anchor validation");
}

fn assert_creep_fx(client: &Client, case: &CreepCase, id: EntityId) {
    let manifest = client.world().resource::<crate::manifest::Manifest>();
    let unit = client.view().unit(id).expect("shown creep");
    let normal = case
        .npc
        .replace("_upgraded_mega", "")
        .replace("_upgraded", "");
    for (npc, expected) in [(case.npc.as_str(), true), (normal.as_str(), false)] {
        let subject = format!("unit.{npc}.worn");
        let fx =
            crate::fx::line::Fx::parse(&[&subject, "at=unit", "particle=ward.observer.ambient"])
                .expect("unit effect");
        let rule = crate::fx::rules::Rule::resolve(&fx, manifest).expect("NPC effect rule");
        let crate::fx::rules::Subject::Units(units) = rule.subject else {
            panic!("unit rule")
        };
        assert_eq!(units.is(unit), expected, "{npc} against {}", case.npc);
    }
}
