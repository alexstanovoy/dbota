//! The hero pick screen end to end against a real server: hovering and choosing a hero, LOCK IN and
//! RANDOM sending the real orders, the seats filling as the others lock in, the pool of one
//! hero, and the "choose your hero" line starting with the selection and not before it. The
//! countdown, the late phase and what is sent are in `pregame_phases`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::prelude::*;
use bota_proto::{HeroId, PickPhase};

use super::harness::{Client, Server, quiet_seat, subtree};

const PATIENCE: Duration = Duration::from_secs(30);

/// A lobby of two seats whose first a bot holds, locked in with Pudge, and the client on the
/// second with the selection clock running (`pick_secs`) and no end to the tactical phase.
fn on_the_pick_screen(pick_secs: &str, extra: &[&str]) -> (Server, Client) {
    let mut options = vec!["--pick-secs", pick_secs, "--tactical-secs", "600"];
    options.extend_from_slice(extra);
    let server = Server::start(2, &options);
    quiet_seat(&server, HeroId::PUDGE);
    let mut client = Client::run(&["--addr", &server.address]);
    client.until("the selection clock running", |c| {
        c.core().lobby.pick != PickPhase::Waiting
    });
    wait_for_loading(&mut client);
    (server, client)
}

/// Waits until no loading screen blocks the interface.
fn wait_for_loading(client: &mut Client) {
    client.within(PATIENCE, "the loading screen lifted", |client| {
        let world = client.world();
        world.resource::<crate::loading::Loading>().done()
            && world.resource::<crate::trees::SceneryReady>().0
            && world.resource::<crate::minimap::PhotoTaken>().0
            && world
                .resource::<crate::loading::Parts>()
                .0
                .values()
                .all(|(done, total)| done == total)
    });
}

/// The entity of a node by its name.
fn named(client: &mut Client, name: &str) -> Option<Entity> {
    let world = client.world_mut();
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, label)| label.as_str() == name)
        .map(|(entity, _)| entity)
}

/// The hero a seat holds in the lobby the client was told.
fn picked(client: &Client, hero_of_me: bool) -> Option<HeroId> {
    let core = client.core();
    let slot = core.my_slot.filter(|_| hero_of_me)?;
    core.lobby
        .slots
        .iter()
        .find(|s| s.slot == slot)
        .and_then(|s| s.hero)
}

fn hover(client: &mut Client, name: &str) -> Entity {
    let name = name.to_owned();
    client.hover_ui_with(PATIENCE, move |c| named(c, &name))
}

fn said(client: &mut Client, text: &str) -> bool {
    client.shown_texts().iter().any(|t| t == text)
}

/// Under the pointer a card lights (grows a little, takes a ring) and the panel shows its hero;
/// the click chooses the hero and nothing is sent yet: the lobby does not hold it.
#[test]
fn a_hovered_card_lights_and_shows_its_hero_and_choosing_it_sends_nothing() {
    let (_server, mut client) = on_the_pick_screen("600", &[]);
    assert!(
        !said(&mut client, "SHADOW FIEND"),
        "the panel starts on the first hero of the grid"
    );
    let card = hover(&mut client, "pick shadow fiend");
    client.within(PATIENCE, "the hovered hero in the panel", |c| {
        said(c, "SHADOW FIEND")
    });
    let (scale, ring) = {
        let world = client.world();
        (
            world.get::<UiTransform>(card).expect("a transform").scale,
            world.get::<Outline>(card).expect("an outline").color,
        )
    };
    assert!(scale.x > 1.0 && scale.y > 1.0, "the card grew: {scale}");
    assert_ne!(ring, Color::NONE, "the card has a ring");
    client.click(MouseButton::Left);
    client.frames(30);
    assert_eq!(picked(&client, true), None, "choosing sends no pick");
    assert!(!client.core().ready, "choosing readies nobody");
}

/// LOCK IN sends the pick and the readiness the server makes final; with every other seat locked
/// in the tactical phase begins.
#[test]
fn lock_in_sends_the_chosen_hero_and_the_readiness() {
    let (_server, mut client) = on_the_pick_screen("600", &[]);
    client.click_named("pick shadow fiend");
    assert!(
        said(&mut client, "SHADOW FIEND"),
        "the chosen hero is on LOCK IN's button"
    );
    client.click_named("pick ready");
    client.within(PATIENCE, "the pick and readiness on the server", |c| {
        picked(c, true) == Some(HeroId::SHADOW_FIEND)
            && c.core()
                .lobby
                .slots
                .iter()
                .any(|s| Some(s.slot) == c.core().my_slot && s.ready)
    });
    client.within(PATIENCE, "every pick final", |c| {
        c.core().in_tactical_phase()
    });
}

/// LOCK IN does nothing before a hero is chosen.
#[test]
fn lock_in_with_no_hero_chosen_sends_nothing() {
    let (_server, mut client) = on_the_pick_screen("600", &[]);
    client.click_named("pick ready");
    client.frames(30);
    assert!(!client.core().ready, "nobody is readied");
    assert_eq!(picked(&client, true), None, "no pick was sent");
}

/// RANDOM draws among the heroes on offer by its die and locks that one in.
#[test]
fn random_locks_in_the_hero_the_die_draws_among_those_on_offer() {
    let (_server, mut client) = on_the_pick_screen("600", &[]);
    let seed = 7;
    client.world_mut().insert_resource(crate::select::Die(seed));
    let offered: Vec<u16> = (0..bota_core::HEROES.len() as u16).collect();
    let drawn = offered[(crate::select::Die(seed).roll() % offered.len() as u64) as usize];
    client.click_named("pick random");
    client.within(PATIENCE, "the drawn hero locked in", |c| {
        picked(c, true) == Some(HeroId(drawn)) && c.core().ready
    });
}

/// The top bar's seat holds the hero a player locked in: the bot's Pudge shows in its seat the
/// moment the client's pick screen is up, and one's own seat fills at LOCK IN.
#[test]
fn a_seat_fills_with_its_heros_picture_as_its_player_locks_in() {
    let (_server, mut client) = on_the_pick_screen("600", &[]);
    let icon = |client: &Client, hero: HeroId| {
        client
            .world()
            .resource::<crate::manifest::Manifest>()
            .hero_icon(hero.0)
            .expect("the hero's icon in the cache")
    };
    let shown = |client: &mut Client, seat: &str, path: &str| {
        let Some(frame) = named(client, seat) else {
            return false;
        };
        let world = client.world();
        let server = world.resource::<AssetServer>();
        subtree(world, frame).into_iter().any(|e| {
            world
                .get::<ImageNode>(e)
                .and_then(|image| server.get_path(image.image.id()))
                .is_some_and(|p| p.to_string() == path)
        })
    };
    let pudge = icon(&client, HeroId::PUDGE);
    client.within(PATIENCE, "the bot's Pudge in its seat", |c| {
        shown(c, "pick seat radiant 0", &pudge)
    });
    client.click_named("pick shadow fiend");
    client.click_named("pick ready");
    let fiend = icon(&client, HeroId::SHADOW_FIEND);
    client.within(PATIENCE, "one's own Shadow Fiend in its seat", |c| {
        shown(c, "pick seat dire 0", &fiend)
    });
}

/// The ability under the pointer in the panel opens its popup with the ability's name.
#[test]
fn an_ability_in_the_pick_panel_opens_its_popup() {
    let (_server, mut client) = on_the_pick_screen("600", &[]);
    hover(&mut client, "pick shadow fiend");
    client.within(PATIENCE, "the hovered hero's skills", |c| {
        named(c, "hero ability 0").is_some()
    });
    let ability = hover(&mut client, "hero ability 0");
    let id = client
        .world()
        .get::<crate::tips::AbilityTile>(ability)
        .expect("an ability tile")
        .0;
    let name = {
        let manifest = client.world().resource::<crate::manifest::Manifest>();
        let key = manifest.text_key("ability", id).expect("its text key");
        let texts = client.world().resource::<crate::dota_text::Texts>();
        texts
            .get(&key)
            .expect("its name in the install's words")
            .name
            .clone()
    };
    client.within(PATIENCE, "the ability's popup", |c| {
        c.shown_texts()
            .iter()
            .any(|t| t.eq_ignore_ascii_case(&name))
    });
}

/// The server's pool of one hero is a forced pick: the seat is given it, so while the other seats
/// are still to come the screen says so, shows that hero alone and offers neither LOCK IN nor
/// RANDOM.
#[test]
fn a_pool_of_one_hero_is_shown_without_lock_in_or_random() {
    let server = Server::start(
        2,
        &[
            "--pick-secs",
            "600",
            "--tactical-secs",
            "600",
            "--heroes",
            "1",
        ],
    );
    let mut client = Client::run(&["--addr", &server.address]);
    client.until("the forced pool told to a seat", |c| {
        c.core().my_slot.is_some() && c.core().lobby.heroes.len() == 1
    });
    wait_for_loading(&mut client);
    client.within(PATIENCE, "the forced pick's heading", |c| {
        said(c, "YOUR HERO")
    });
    assert!(named(&mut client, "pick shadow fiend").is_some());
    assert!(named(&mut client, "pick pudge").is_none(), "no other hero");
    assert!(named(&mut client, "pick ready").is_none(), "no LOCK IN");
    assert!(named(&mut client, "pick random").is_none(), "no RANDOM");
}

/// The announcer's "choose your hero" does not sound while the seats are still being taken (the
/// match is loading then); it sounds once the selection clock starts.
#[test]
fn the_choose_your_hero_line_starts_with_the_selection_and_not_before() {
    let server = Server::start(2, &["--pick-secs", "600", "--tactical-secs", "600"]);
    let mut client = Client::run(&["--addr", &server.address]);
    let heard = Arc::new(Mutex::new(Vec::<String>::new()));
    let into = heard.clone();
    client.world_mut().add_observer(
        move |add: On<Add<AudioPlayer>>, players: Query<&AudioPlayer>, server: Res<AssetServer>| {
            let path = players
                .get(add.entity)
                .ok()
                .and_then(|p| server.get_path(p.0.id()))
                .map(|p| p.to_string());
            into.lock().expect("unpoisoned").extend(path);
        },
    );
    let lines: Vec<String> = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .sound_files("sound.announcer.choose_hero")
        .map(str::to_owned)
        .collect();
    assert!(!lines.is_empty(), "the install's line in the cache");
    let said_now = {
        let heard = heard.clone();
        move || {
            heard
                .lock()
                .expect("unpoisoned")
                .iter()
                .any(|path| lines.iter().any(|line| path.ends_with(line)))
        }
    };
    client.until("a seat taken and the selection waiting", |c| {
        c.core().my_slot.is_some() && c.core().lobby.pick == PickPhase::Waiting
    });
    wait_for_loading(&mut client);
    client.frames(120);
    assert!(!said_now(), "no line while the others have not come");
    quiet_seat(&server, HeroId::PUDGE);
    client.until("the selection clock running", |c| {
        matches!(c.core().lobby.pick, PickPhase::Selecting { .. })
    });
    client.within(PATIENCE, "the line", |_| said_now());
}

/// The hero the strategy screen stands on its stage leaves it with the screen: through the match
/// no stage camera, light or animated model is left (the stage belongs to the menu's pages and the
/// strategy screen only).
#[test]
fn the_strategy_screens_hero_stage_is_released_when_the_match_begins() {
    let server = Server::start(2, &["--pick-secs", "600", "--tactical-secs", "4"]);
    quiet_seat(&server, HeroId::PUDGE);
    let mut client = Client::run(&["--addr", &server.address]);
    client.until("the pick screen", |c| {
        c.core().lobby.pick != PickPhase::Waiting
    });
    wait_for_loading(&mut client);
    client.click_named("pick shadow fiend");
    client.click_named("pick ready");
    client.within(PATIENCE, "the model on the strategy stage", |c| {
        c.world()
            .resource::<crate::main_menu::hero_stage::HeroStage>()
            .actor
            .is_some()
    });
    client.until("the match begun", |c| c.core().ears().is_some());
    client.frames(5);
    let world = client.world_mut();
    assert!(
        world
            .resource::<crate::main_menu::hero_stage::HeroStage>()
            .actor
            .is_none(),
        "the stage's actor is gone in the match"
    );
    let cameras = world
        .query_filtered::<Entity, With<crate::main_menu::hero_stage::StageEntity>>()
        .iter(world)
        .count();
    assert_eq!(cameras, 0, "no stage camera or light in the match");
}
