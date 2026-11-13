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

/// One's own courier keeps the original colours of the icon, even for the blue player.
#[test]
fn ones_courier_keeps_the_original_icon_colours_for_the_blue_player() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let (courier, owner) = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Courier && u.owner == client.core().my_slot)
        .map(|u| (u.pos, u.owner.expect("owned")))
        .expect("one's courier");
    let own = client
        .world()
        .resource::<crate::player_colour::PlayerColours>()
        .of(client.view(), owner);
    assert_ne!(
        own,
        crate::overlay::team_color(Team::Radiant),
        "the seat's colour is not its side's"
    );
    let middle = crate::minimap::on_map(courier.x.to_f32(), courier.y.to_f32());
    client.within(Duration::from_secs(20), "the courier marked", |c| {
        crate::minimap::shown_unit_tints(c.world_mut())
            .iter()
            .any(|(_, found)| found.distance(middle) < 1.0)
    });
    let tint = crate::minimap::shown_unit_tints(client.world_mut())
        .into_iter()
        .find(|(_, found)| found.distance(middle) < 1.0)
        .expect("marked")
        .0;
    assert_eq!(
        tint,
        Color::WHITE,
        "the courier artwork keeps its own colours"
    );
}

/// A unit of `kind` on the side asked for (`enemy`), if the view has one.
fn unit_of(
    client: &Client,
    mine: Team,
    kind: UnitKind,
    enemy: Option<bool>,
) -> Option<bota_proto::EntityId> {
    client
        .view()
        .units
        .iter()
        .filter(|u| u.kind == kind && u.hp > 0)
        .find(|u| enemy.is_none_or(|enemy| (u.team != mine) == enemy))
        .map(|u| u.id)
}

/// What a test selects and clicks the minimap with: the name, the unit kind and whose it is.
const TARGETS: [(&str, UnitKind, Option<bool>); 7] = [
    ("an enemy hero", UnitKind::Hero, Some(true)),
    ("an own creep", UnitKind::CreepMelee, Some(false)),
    ("an enemy creep", UnitKind::CreepMelee, Some(true)),
    ("an own tower", UnitKind::Tower, Some(false)),
    ("an enemy tower", UnitKind::Tower, Some(true)),
    ("the courier", UnitKind::Courier, Some(false)),
    ("a ward", UnitKind::Ward, Some(false)),
];

/// Selects each of [`TARGETS`] in turn and clicks the minimap, left and right, over its middle
/// and two corners with the pointer coming from the world; the unit stays selected.
fn selection_survives_the_minimap(client: &mut Client, mine: Team) {
    let frame = crate::minimap::frame(client.world());
    let world = client.world_mut();
    let scale = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .expect("the window")
        .scale_factor();
    let centre = client.centre_of(frame).expect("laid out") / scale;
    let half = client
        .world()
        .get::<bevy::ui::ComputedNode>(frame)
        .expect("laid out")
        .size()
        / (2.0 * scale);
    let away = centre + Vec2::new(0.0, -400.0);
    for (what, kind, enemy) in TARGETS {
        let unit = unit_of(client, mine, kind, enemy).unwrap_or_else(|| panic!("{what} in view"));
        for (dx, dy) in [(0.0, 0.0), (-0.95, -0.95), (0.95, 0.95)] {
            let at = centre + Vec2::new(dx, dy) * half;
            for button in [MouseButton::Left, MouseButton::Right] {
                client.core_mut().act(Intent::SelectUnit(Some(unit)));
                client.mouse(away);
                client.point(at);
                client.click(button);
                assert!(
                    client
                        .world()
                        .get::<bevy::picking::hover::DirectlyHovered>(frame)
                        .is_some_and(|hover| hover.get()),
                    "the click must reach the minimap itself"
                );
                assert_eq!(
                    client.core().selected,
                    Some(unit),
                    "{what}: {button:?} on the minimap at {at}: still selected"
                );
            }
        }
    }
}

/// Every kind of unit in a scene, around the seat's hero at the middle of the lane, as a
/// match shows them: both sides' heroes and creeps, their towers, the courier, a ward.
fn stage_every_kind(client: &mut Client) {
    use super::harness::MID;
    client.stand_at(MID);
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::AllVision { on: true });
    for (enemy, dx) in [(false, 250), (true, -250)] {
        client.cheat(Cheat::CreateCreeps {
            enemy,
            at: bota_proto::Vec2::from_ints(MID.0 + dx, MID.1 + 200),
        });
    }
    client.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at: bota_proto::Vec2::from_ints(MID.0 + 450, MID.1),
    });
    client.cheat(Cheat::Item {
        item: bota_proto::ItemId::OBSERVER_WARD,
    });
    client.until("the ward in the bag", |c| {
        c.slot_of(bota_proto::ItemId::OBSERVER_WARD).is_some()
    });
    let slot = client
        .slot_of(bota_proto::ItemId::OBSERVER_WARD)
        .expect("there");
    let core = client.core_mut();
    core.act(Intent::Press {
        slot: Slot::Item(slot),
        ctrl: false,
    });
    core.act(Intent::Aimed {
        at: (MID.0 as f32 + 100.0, MID.1 as f32 + 100.0),
        unit: None,
    });
    let mine = client.me().team;
    client.until("every kind of unit in view", |c| {
        TARGETS
            .iter()
            .all(|(_, kind, enemy)| unit_of(c, mine, *kind, *enemy).is_some())
    });
}

/// Dota's minimap takes its clicks itself: a left click moves the camera and a right click
/// orders the selection, and the unit selected stays selected, whatever it is (a hero or a
/// creep of either side, a building, the courier, a ward). Neutrals are left out: the map
/// holds no camps in a match of this size.
#[test]
fn a_click_on_the_minimap_keeps_the_selection() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    stage_every_kind(&mut client);
    let mine = client.me().team;
    server.frozen(|| {
        for scale in [1.0, 1.5] {
            display_scale(&mut client, scale);
            selection_survives_the_minimap(&mut client, mine);
        }
    });
}

/// The same in a replay, where any unit may be selected and none is commanded.
#[test]
fn a_click_on_the_minimap_keeps_the_selection_in_a_replay() {
    let directory = ReplayDirectory::new();
    let replay = directory.0.join("match.brp");
    let (mine, tick) = {
        let mut server = Server::start(
            1,
            &[
                "--replay",
                replay.to_str().expect("UTF-8"),
                "--empty-after-secs",
                "1",
            ],
        );
        let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        stage_every_kind(&mut client);
        client.frames(30);
        let (mine, tick) = (client.me().team, client.tick());
        drop(client);
        server.wait();
        (mine, tick)
    };
    let mut client = Client::run(&["--replay", replay.to_str().expect("UTF-8")]);
    replay_to(&mut client, tick);
    client.until("every kind of unit in view", |c| {
        TARGETS
            .iter()
            .all(|(_, kind, enemy)| unit_of(c, mine, *kind, *enemy).is_some())
    });
    selection_survives_the_minimap(&mut client, mine);
}

/// Plays the replay to exactly `tick` and holds it there. The clock jump is additive: queued again
/// on every frame decoding took, and with the replay let go when its scenery is loaded, the
/// replay ran on past the staged tick, where the creeps fighting at the middle had died. The jump
/// is queued once, from the first snapshot, with no presentation time running and the replay
/// set aside while its scenery loads (as `loot_names` does).
fn replay_to(client: &mut Client, tick: u32) {
    let zero = || bevy::time::TimeUpdateStrategy::ManualDuration(Duration::ZERO);
    client.world_mut().insert_resource(zero());
    let bota_client_core::Source::Replay(player) = &mut client.core_mut().source else {
        panic!("a replay");
    };
    player.paused = true;
    client.until("the replay's first snapshot", |c| c.core().view().is_some());
    let source = std::mem::replace(
        &mut client.core_mut().source,
        crate::main_menu::idle().0.source,
    );
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_millis(100),
        ));
    client.until("the replay scenery loaded", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    client.world_mut().insert_resource(zero());
    client.core_mut().source = source;
    let advance = tick
        .checked_sub(client.tick())
        .expect("the staged tick follows the first snapshot");
    if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
        player.advance_ticks(f64::from(advance));
    }
    client.until("the replay at exactly the staged tick", |c| {
        c.tick() == tick
    });
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::Automatic);
}

fn display_scale(client: &mut Client, scale: f32) {
    let world = client.world_mut();
    let window = {
        let (id, mut window) = world
            .query_filtered::<(Entity, &mut Window), With<bevy::window::PrimaryWindow>>()
            .single_mut(world)
            .expect("the window");
        window.resolution.set_scale_factor_override(Some(scale));
        window
            .resolution
            .set_physical_resolution((1920.0 * scale) as u32, (1080.0 * scale) as u32);
        id
    };
    world.write_message(bevy::window::WindowScaleFactorChanged {
        window,
        scale_factor: f64::from(scale),
    });
    world.write_message(bevy::window::WindowResized {
        window,
        width: 1920.0,
        height: 1080.0,
    });
    client.frames(2);
}

struct ReplayDirectory(std::path::PathBuf);

impl ReplayDirectory {
    fn new() -> Self {
        for attempt in 0..32 {
            let path = crate::test_support::scratch_path(&format!(
                "minimap_{}_{attempt}",
                std::process::id()
            ));
            match std::fs::create_dir(&path) {
                Ok(()) => return Self(path),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => panic!("create a private replay folder: {error}"),
            }
        }
        panic!("no unused replay folder among 32 candidates");
    }
}

impl Drop for ReplayDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A dense friendly wave stays marked on every frame, including between network ticks.
#[test]
fn a_steady_wave_keeps_every_creep_dot_between_snapshots() {
    let replay =
        crate::test_support::scratch_path(&format!("minimap_dense_{}.brp", std::process::id()));
    let (mut server, mut client) = dense_wave_scene(&replay);
    // Remove the network as a source of changes: no marker may vanish while its unit remains.
    let tick = server.frozen(|| {
        client.frames(4);
        assert_steady_creep_dots(&mut client);
        client.tick()
    });
    drop(client);
    server.wait();
    std::fs::write(
        replay.with_extension("json"),
        serde_json::to_vec(&serde_json::json!({ "tick": tick })).expect("fixture metadata"),
    )
    .expect("dense-wave replay metadata");
    println!("dense minimap replay: {} at {tick}", replay.display());
}

fn dense_wave_scene(replay: &std::path::Path) -> (Server, Client) {
    let server = Server::start(
        1,
        &[
            "--replay",
            replay.to_str().expect("UTF-8"),
            "--empty-after-secs",
            "1",
        ],
    );
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.until("the gameplay minimap loaded", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: bota_proto::CreepSide::All,
    });
    client.cheat(Cheat::AllVision { on: true });
    client.cheat(Cheat::StartGame);
    for row in 0..50 {
        client.cheat(Cheat::CreateCreeps {
            enemy: false,
            at: bota_proto::Vec2::from_ints(4500 + row % 8 * 180, 4500 + row / 8 * 180),
        });
    }
    client.until("the dense wave arrives", |c| {
        c.view()
            .units
            .iter()
            .filter(|u| minimap_creep(u.kind))
            .count()
            > 170
    });
    (server, client)
}

fn minimap_creep(kind: UnitKind) -> bool {
    matches!(
        kind,
        UnitKind::CreepMelee
            | UnitKind::CreepFlagbearer
            | UnitKind::CreepRanged
            | UnitKind::CreepNeutral
    )
}

fn assert_steady_creep_dots(client: &mut Client) {
    let expected: Vec<_> = client
        .view()
        .units
        .iter()
        .filter(|u| minimap_creep(u.kind))
        .map(|u| crate::minimap::on_map(u.pos.x.to_f32(), u.pos.y.to_f32()))
        .collect();
    let creep = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file("icon.minimap.creep")
        .expect("the creep dot");
    for frame in 0..90 {
        client.frames(1);
        let marks: Vec<_> = crate::minimap::shown_unit_marks(client.world_mut())
            .into_iter()
            .filter(|(path, at)| {
                path.as_deref() == Some(creep.as_str())
                    && expected.iter().any(|unit| at.distance(*unit) < 0.1)
            })
            .collect();
        assert_eq!(
            marks.len(),
            expected.len(),
            "frame {frame}: one dot per creep"
        );
        // The same icon also marks fillers; these are the dots at the creeps' positions.
        for at in &expected {
            assert!(
                marks.iter().any(|(_, marked)| marked.distance(*at) < 0.1),
                "frame {frame}: a wave dot at {at} disappeared ({} dots)",
                marks.len()
            );
        }
    }
}

/// The minimap remembers a sighting, never invents Roshan's location from a public timer.
#[test]
fn roshans_red_minimap_icon_remembers_only_what_the_player_saw() {
    let server = Server::start(1, &["--objectives", "roshan"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::StartGame);
    let icon = client
        .world()
        .resource::<crate::manifest::Manifest>()
        .file("icon.minimap.roshan")
        .expect("Roshan's icon");
    let shown = |c: &mut Client| {
        crate::minimap::shown_unit_marks(c.world_mut())
            .into_iter()
            .filter(|(path, _)| path.as_deref() == Some(icon.as_str()))
            .count()
    };
    client.frames(3);
    assert_eq!(
        shown(&mut client),
        0,
        "an unseen spawn has no fabricated sighting"
    );
    let pit = bota_core::ROSHAN_DAY_PIT;
    client.stand_at((pit.x.to_int() + 200, pit.y.to_int()));
    client.until("Roshan visible", |c| {
        c.view().units.iter().any(|u| u.kind == UnitKind::Roshan)
    });
    client.frames(4);
    assert_eq!(
        shown(&mut client),
        1,
        "the visible Roshan wears his own red icon"
    );
    let boss = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Roshan)
        .expect("seen");
    let centre = crate::minimap::on_map(boss.pos.x.to_f32(), boss.pos.y.to_f32());
    let tint = crate::minimap::shown_unit_tints(client.world_mut())
        .into_iter()
        .find(|(_, at)| at.distance(centre) < 0.1)
        .expect("Roshan's mark")
        .0
        .to_srgba();
    assert!(
        tint.red > 0.9 && tint.green < 0.2 && tint.blue < 0.2,
        "Roshan's icon is red"
    );
    // Real minimap input moves the camera without revealing a different part of the map.
    let minimap = crate::minimap::frame(client.world());
    client.hover_ui(minimap, Duration::from_secs(20));
    client.click(MouseButton::Left);
    client.stand_at(MID);
    client.until("Roshan out of sight", |c| {
        !c.view().units.iter().any(|u| u.kind == UnitKind::Roshan)
    });
    client.frames(4);
    assert_eq!(
        shown(&mut client),
        1,
        "out of sight retains the last actual sighting"
    );
    assert!(
        !client
            .shown_texts()
            .iter()
            .any(|text| text.contains("Roshan is") || text.contains("Roshan may")),
        "no Roshan status message above the minimap"
    );
}
