//! The words of every hero's talents: Dota's own, with the rules' numbers filled in, as the
//! window shows them, and each fitting the box of its choice at a small window and at 4K.

use bevy::prelude::*;
use bota_proto::HeroId;

use super::harness::{Client, Server};
use crate::dota_text::Texts;
use crate::faces::talents;
use crate::talents::{overflowing, shown};

fn dota_texts() -> Texts {
    let cache = std::env::var("DBOTA_ASSETS").expect("DBOTA_ASSETS names a test asset cache");
    let texts = Texts::load(std::path::Path::new(&cache));
    assert!(!texts.is_empty(), "the cache holds Dota's texts");
    texts
}

#[test]
fn shadow_fiends_742_tree_uses_imported_labels_in_server_slot_order() {
    let labels = serde_json::json!({
        "special_bonus_unique_nevermore_frenzy_max_collection_count": {"name": "Souls +{s:bonus_soul_collection_per_hero}"},
        "special_bonus_unique_nevermore_7": {"name": "Stack +{s:bonus_stack_bonus_damage}"},
        "special_bonus_unique_nevermore_frenzy_heal_per_soul": {"name": "Heal {s:bonus_heal_per_soul}"},
        "special_bonus_unique_nevermore_3": {"name": "Presence {s:bonus_presence_armor_reduction}/{s:bonus_presence_magic_resistance_reduction}"},
        "special_bonus_unique_nevermore_6": {"name": "Fear {s:bonus_requiem_fear_duration}"},
        "special_bonus_unique_nevermore_1": {"name": "Cap +{s:bonus_necromastery_max_souls}"},
        "special_bonus_unique_nevermore_raze_procsattacks": {"name": "Raze Attack"},
        "special_bonus_unique_nevermore_frenzy_castspeed": {"name": "Cast +{s:bonus_cast_speed_pct}"},
        "special_bonus_unique_nevermore_4": {"name": "Retired"}
    });
    let root = crate::test_support::scratch_path(&format!("talent_locale_{}", std::process::id()));
    std::fs::create_dir_all(root.join("text")).expect("a test locale directory");
    std::fs::write(root.join("text/tooltips.json"), labels.to_string())
        .expect("a test tooltip cache");
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.world_mut().insert_resource(Texts::load(&root));
    client.click_named("talent button");
    client.step_frames(2);
    assert_eq!(
        shown(client.world_mut()).texts,
        [
            "Souls +2",
            "Stack +30",
            "Heal 25",
            "Presence 1/2",
            "Fear 0.5",
            "Cap +5",
            "Raze Attack",
            "Cast +30"
        ]
    );
}

#[test]
fn talent_choices_use_the_imported_hud_face() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.click_named("talent button");
    let font = shown(client.world_mut()).choice_font;
    let bevy::text::FontSource::Handle(handle) = font else {
        panic!("the talent choice uses an asset font");
    };
    let path = client
        .world()
        .resource::<AssetServer>()
        .get_path(handle.id());
    assert_eq!(
        path.as_ref().and_then(|path| path.path().to_str()),
        Some("fonts/radiance-regular.otf")
    );
    client.within(std::time::Duration::from_secs(30), "Radiance loaded", |c| {
        c.world().resource::<Assets<Font>>().contains(&handle)
    });
}

#[test]
fn no_talent_of_any_hero_shows_a_raw_key_or_a_variable() {
    let texts = dota_texts();
    for hero in bota_core::HEROES
        .iter()
        .filter(|hero| hero.talents.is_some())
    {
        let faces = talents(hero.id.0, &texts).expect("a tree");
        let tree = hero.talents.expect("a tree");
        for (slot, (talent, face)) in tree.iter().zip(&faces).enumerate() {
            for words in [&face.name, &face.blurb] {
                assert!(
                    !words.contains(['{', '}', '_']),
                    "{} talent {slot} shows {words:?}",
                    hero.key
                );
            }
            assert!(
                !face.name.trim().is_empty(),
                "{} talent {slot} has a name",
                hero.key
            );
            assert_eq!(
                face.name == "—",
                texts.get(talent.key()).is_none(),
                "{} talent {slot} resolves its installed template if present",
                hero.key
            );
        }
    }
}

/// Every hero's eight choices fit their boxes at a window size.
fn texts_fit_at(window: &str) {
    for hero in bota_core::HEROES
        .iter()
        .filter(|hero| hero.talents.is_some())
    {
        let server = Server::start(1, &[]);
        let mut client = Client::join(&server, hero.id, &["--window", window]);
        client
            .world_mut()
            .resource_mut::<crate::input::Panels>()
            .talents = true;
        client.frames(6);
        let window_shown = shown(client.world_mut());
        let boxes = overflowing(client.world_mut());
        assert!(
            window_shown.texts.iter().all(|words| !words.is_empty()),
            "{} at {window}: every choice says something: {:?}",
            hero.key,
            window_shown.texts
        );
        assert!(
            boxes.is_empty(),
            "{} at {window}: words overrun their box (words, box, size): {boxes:?}",
            hero.key
        );
    }
}

#[test]
fn every_heros_talents_fit_their_boxes_at_1280_by_720() {
    texts_fit_at("1280x720");
}

#[test]
fn every_heros_talents_fit_their_boxes_at_4k() {
    texts_fit_at("3840x2160");
}

#[test]
fn a_closed_window_does_not_prepare_its_title_or_talent_labels() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let closed = shown(client.world_mut());
    assert_eq!(closed.mode, crate::talents::Mode::Closed);
    assert!(
        closed.title.is_empty(),
        "the closed window prepared its title"
    );
    assert!(
        closed.texts.iter().all(String::is_empty),
        "the closed window prepared its talent labels"
    );
    client.click_named("talent button");
    assert!(
        !shown(client.world_mut()).title.is_empty(),
        "opening prepares the title"
    );
}

#[test]
fn the_pinned_locale_shows_a_dash_for_the_new_feast_talent() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let tree = bota_core::hero_def(HeroId::SHADOW_FIEND)
        .unwrap()
        .talents
        .unwrap();
    let key = bota_core::Talent::FeastHealPerSoul.key();
    let slot = tree
        .iter()
        .position(|talent| talent.key() == key)
        .expect("the shared tree offers the new talent");
    let texts = client.world().resource::<Texts>();
    let from_cache = texts.get(key).is_some();
    client.click_named("talent button");
    let names = shown(client.world_mut()).texts;
    // The current 7.41f cache has no 7.42 label. A newer cache may have it.
    assert_eq!(
        names[slot] == "—",
        !from_cache,
        "the dash signals a missing installed talent label"
    );
    if !from_cache {
        assert_eq!(names.iter().filter(|name| *name == "—").count(), 1);
    }
}

#[test]
fn the_installed_feast_cast_speed_label_matches_the_server_cast_point_reduction() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let hero = bota_core::hero_def(HeroId::SHADOW_FIEND).expect("the hero");
    let talent = bota_core::Talent::FeastCastSpeed;
    let slot = hero
        .talents
        .expect("the tree")
        .iter()
        .position(|held| *held == talent)
        .expect("the talent's slot");
    let reduction = 100 - bota_core::rules::TALENT_FEAST_CAST_POINT_PCT;
    let expected = client
        .world()
        .resource::<Texts>()
        .talent_name(
            talent.key(),
            &[("bonus_cast_speed_pct".to_string(), reduction.to_string())],
        )
        .expect("the install's cast-speed line");
    client.click_named("talent button");
    assert_eq!(shown(client.world_mut()).texts[slot], expected);
}

#[test]
fn the_legacy_requiem_template_does_not_claim_a_per_line_increase() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    let talent = bota_core::Talent::RequiemFear;
    let tree = bota_core::hero_def(HeroId::SHADOW_FIEND)
        .unwrap()
        .talents
        .unwrap();
    let slot = tree.iter().position(|held| *held == talent).unwrap();
    let texts = client.world().resource::<Texts>();
    let installed = texts
        .get(talent.key())
        .expect("the installed talent template");
    let legacy = installed.name.contains("bonus_requiem_slow_duration");
    let name = texts
        .get("nevermore_requiem")
        .expect("the installed ability name")
        .name
        .clone();
    client.click_named("talent button");
    if legacy {
        let shown = &shown(client.world_mut()).texts[slot];
        assert!(
            shown.contains(&name),
            "the outdated template is still shown: {shown}"
        );
        assert!(
            shown.contains("+0.5s"),
            "the base duration increases by half a second: {shown}"
        );
    }
}
