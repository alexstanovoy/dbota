//! Installed names reach the actual client surfaces rather than stopping at tooltip lookups.
use super::menu::{act, local_menu};
use crate::main_menu::{Act, Page};

#[test]
fn the_hero_catalogue_draws_the_installs_name() {
    let root = crate::test_support::scratch_path("player_names_catalogue");
    std::fs::create_dir_all(root.join("text")).expect("fixture directory");
    std::fs::write(
        root.join("text/labels.json"),
        r#"{"npc_dota_hero_nevermore:n":"Installed Shadow Hero"}"#,
    )
    .expect("fixture labels");
    let mut client = local_menu();
    client
        .world_mut()
        .insert_resource(crate::dota_text::Texts::load(&root));
    act(
        &mut client,
        Act::InspectHero(bota_proto::HeroId::SHADOW_FIEND),
    );
    client.frames(5);
    let words = crate::main_menu::tests::page_words(client.world_mut());
    assert!(
        words.iter().any(|word| word == "INSTALLED SHADOW HERO"),
        "the hero page draws the installed hero name: {words:?}"
    );
    act(&mut client, Act::Go(Page::Heroes));
}

#[test]
fn installed_hero_and_creep_names_reach_the_live_hud_and_scoreboard() {
    use super::harness::{Client, Server};
    use bota_client_core::Intent;
    use bota_proto::{Cheat, HeroId, UnitKind};
    let root = crate::test_support::scratch_path("player_names_hud");
    std::fs::create_dir_all(root.join("text")).expect("fixture directory");
    std::fs::write(
        root.join("text/labels.json"),
        r#"{
        "npc_dota_hero_nevermore:n":"Installed Shadow Hero",
        "npc_dota_creep_goodguys_melee":"Installed Lane Unit",
        "npc_dota_creep_badguys_melee":"Installed Lane Unit"
    }"#,
    )
    .expect("fixture labels");
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client
        .world_mut()
        .insert_resource(crate::dota_text::Texts::load(&root));
    client.frames(4);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|word| word == "INSTALLED SHADOW HERO"),
        "the selected hero's HUD uses installed text"
    );
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .scoreboard = true;
    client.frames(4);
    client.within(
        std::time::Duration::from_secs(30),
        "installed hero name drawn in scoreboard",
        |client| {
            crate::scoreboard::drawn_names(client.world_mut())
                .iter()
                .any(|text| text == "INSTALLED SHADOW HERO")
        },
    );
    client
        .world_mut()
        .resource_mut::<crate::input::Panels>()
        .scoreboard = false;
    client.cheat(Cheat::CreateCreeps {
        enemy: false,
        at: client.me().pos,
    });
    let mine = client.me().team;
    let creep = |c: &Client| {
        c.view()
            .units
            .iter()
            .find(|unit| unit.team == mine && unit.kind == UnitKind::CreepMelee)
            .map(|unit| unit.id)
    };
    client.until("a visible friendly melee creep", |c| creep(c).is_some());
    let id = creep(&client).expect("creep");
    client.core_mut().act(Intent::SelectUnit(Some(id)));
    client.frames(4);
    assert!(
        client
            .shown_texts()
            .iter()
            .any(|word| word == "INSTALLED LANE UNIT"),
        "the selected creep's HUD uses its NPC key"
    );
}

#[test]
fn the_ability_tooltip_draws_a_client_fallback_when_the_install_name_is_empty() {
    use super::harness::{Client, Server};
    use bevy::prelude::*;
    let root = crate::test_support::scratch_path("player_names_tooltip");
    std::fs::create_dir_all(root.join("text")).expect("fixture directory");
    std::fs::write(
        root.join("text/tooltips.json"),
        r#"{"nevermore_shadowraze1":{"name":""}}"#,
    )
    .expect("empty installed name");
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    client
        .world_mut()
        .insert_resource(crate::dota_text::Texts::load(&root));
    let world = client.world_mut();
    let ability = world
        .query::<(Entity, &crate::hud::AbilityBox)>()
        .iter(world)
        .find(|(_, slot)| slot.0 == 0)
        .map(|(entity, _)| entity)
        .expect("first ability");
    client.hover_ui(ability, std::time::Duration::from_secs(30));
    client.until("fallback ability title drawn", |client| {
        client
            .shown_texts()
            .iter()
            .any(|text| text == "SHADOWRAZE (NEAR)")
    });
}

#[test]
fn installed_ability_and_modifier_names_reach_their_drawn_tooltips() {
    use super::harness::{Client, Server};
    use bevy::prelude::*;
    let root = crate::test_support::scratch_path("player_names_installed_tooltips");
    std::fs::create_dir_all(root.join("text")).expect("fixture directory");
    std::fs::write(
        root.join("text/tooltips.json"),
        r#"{"nevermore_shadowraze1":{"name":"Installed Raze"}}"#,
    )
    .expect("ability title");
    std::fs::write(
        root.join("text/labels.json"),
        r#"{"dota_tooltip_modifier_fountain_aura_buff":"Installed Fountain"}"#,
    )
    .expect("modifier title");
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, bota_proto::HeroId::SHADOW_FIEND, &[]);
    let fountain = bota_core::fountain_pos(
        client.core().session().expect("match").map(),
        client.me().team,
    );
    client.stand_at((fountain.x.to_int(), fountain.y.to_int()));
    client
        .world_mut()
        .insert_resource(crate::dota_text::Texts::load(&root));
    let world = client.world_mut();
    let ability = world
        .query::<(Entity, &crate::hud::AbilityBox)>()
        .iter(world)
        .find(|(_, slot)| slot.0 == 0)
        .map(|(entity, _)| entity)
        .expect("first ability");
    client.hover_ui(ability, std::time::Duration::from_secs(30));
    client.within(
        std::time::Duration::from_secs(30),
        "installed ability title drawn",
        |client| {
            client
                .shown_texts()
                .iter()
                .any(|text| text == "INSTALLED RAZE")
        },
    );
    let mut chip = None;
    client.within(
        std::time::Duration::from_secs(30),
        "fountain buff chip drawn",
        |client| {
            let world = client.world_mut();
            chip = world
                .query::<(Entity, &crate::buffs::BuffChip, &InheritedVisibility)>()
                .iter(world)
                .find(|(_, chip, seen)| {
                    seen.get() && chip.shown.is_some_and(|effect| effect.id == 3)
                })
                .map(|(entity, ..)| entity);
            chip.is_some()
        },
    );
    client.hover_ui(
        chip.expect("fountain buff chip"),
        std::time::Duration::from_secs(30),
    );
    client.within(
        std::time::Duration::from_secs(30),
        "installed modifier title drawn",
        |client| {
            client
                .shown_texts()
                .iter()
                .any(|text| text == "Installed Fountain")
        },
    );
    std::fs::remove_dir_all(root).expect("remove fixture");
}
