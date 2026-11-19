//! Scan end to end: a real server's match, the client driven by real input events. The button
//! shows the charges, its click and key take the Scan up for aiming, the next click on the world
//! or on the minimap aims it, and what the scan finds shows as rings of the rule's radius and in
//! the UI sounds of the scanning side.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_core::rules::{MAP_SIZE, SCAN_RADIUS};
use bota_proto::{Cheat, HeroId, Vec2 as Spot};

use super::harness::{Client, MID, Server, quiet_seat, ring_of};

/// The sound events of Scan that start, in order.
fn listen(client: &mut Client) -> Arc<Mutex<Vec<String>>> {
    let heard = Arc::new(Mutex::new(Vec::new()));
    let into = heard.clone();
    client.world_mut().add_observer(
        move |add: On<Add<AudioPlayer>>, mixer: Res<crate::mixer::Mixer>| {
            let Some((event, _)) = mixer.event_at(add.entity) else {
                return;
            };
            if matches!(
                event,
                "scan_minimap.activate" | "minimap_radar.cycle" | "minimap_radar.target"
            ) {
                into.lock().expect("heard").push(event.to_owned());
            }
        },
    );
    heard
}

/// A match of two seats, the enemy's quiet, with the client's hero standing in the middle of the
/// map and one charge of Scan from the cheat.
fn scene() -> (Server, Client) {
    let server = Server::start(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    (server, client)
}

fn charges(client: &Client) -> Option<u8> {
    client.core().scan().map(|side| side.ledger.charges)
}

/// The badge's number, as the button shows it.
fn badge(client: &mut Client) -> Option<String> {
    let world = client.world_mut();
    world
        .query_filtered::<(&Text, &InheritedVisibility), With<super::super::scan::ScanCount>>()
        .iter(world)
        .find(|(_, shown)| shown.get())
        .map(|(text, _)| text.0.clone())
}

/// The lines laid out in a colour, each as its middle and radius on the ground.
fn rings_in(client: &Client, colour: Color) -> Vec<(Vec2, f32)> {
    let wanted = colour.to_linear();
    client
        .line_strips()
        .into_iter()
        .filter(|(points, line)| *line == wanted && points.len() > 8)
        .map(|(points, _)| ring_of(&points))
        .collect()
}

fn take_up_by_key(client: &mut Client) {
    client.key(KeyCode::KeyP, ButtonState::Pressed);
    client.key(KeyCode::KeyP, ButtonState::Released);
}

/// The window's pixel of a world point on the minimap, as the window reports its pointer: the
/// layout is in physical pixels.
fn minimap_pixel(client: &mut Client, at: Spot) -> Vec2 {
    let frame = crate::minimap::frame(client.world());
    let world = client.world_mut();
    let scale = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .expect("the window")
        .scale_factor();
    let computed = world.get::<ComputedNode>(frame).expect("laid out");
    let placed = world
        .get::<bevy::ui::UiGlobalTransform>(frame)
        .expect("placed");
    let size = computed.size() / scale;
    let map = MAP_SIZE as f32;
    let k = Vec2::new(at.x.to_f32() / map, 1.0 - at.y.to_f32() / map);
    placed.translation / scale - size / 2.0 + k * size
}

/// The whole flow with real input: the Scan charge cheat gives one charge, the key takes the Scan up and the
/// preview rings the rule's radius round the cursor, a click on the world aims it over nothing and
/// the rings show green; a minimap click aims the next at the enemy's fountain, where its hero
/// stands, and the rings turn red.
#[test]
fn a_scan_is_aimed_on_the_world_and_on_the_minimap_and_shows_what_it_finds() {
    let (_server, mut client) = scene();
    let heard = listen(&mut client);
    assert_eq!(charges(&client), Some(0), "no charge before the first");
    client.cheat(Cheat::Refresh);
    client.within(Duration::from_secs(20), "one charge told", |c| {
        charges(c) == Some(1)
    });
    assert_eq!(badge(&mut client).as_deref(), Some("1"), "the badge");

    take_up_by_key(&mut client);
    assert!(client.core().scan_aim, "the key takes the Scan up");
    let aimed_at = (MID.0 as f32 + 600.0, MID.1 as f32);
    let pixel = client.screen_of(aimed_at).expect("on screen");
    client.mouse(pixel);
    let preview = rings_in(&client, super::super::scan::PREVIEW_COLOUR);
    assert_eq!(preview.len(), 1, "one preview ring under the cursor");
    let (middle, radius) = preview[0];
    assert!(
        (radius - SCAN_RADIUS as f32).abs() < 20.0,
        "the rule's radius, not the art's: {radius}"
    );
    assert!(
        middle.distance(Vec2::new(aimed_at.0, aimed_at.1)) < 60.0,
        "{middle:?}"
    );

    client.click(MouseButton::Left);
    assert!(!client.core().scan_aim, "the click aims it");
    client.within(Duration::from_secs(20), "the scan told", |c| {
        c.core().scan().is_some_and(|side| side.radars.len() == 1)
    });
    assert_eq!(charges(&client), Some(0));
    let radar = client.core().scan().expect("told").radars[0];
    assert!(!radar.found, "nothing of the enemy within the radius");
    client.frames(3);
    let green = rings_in(&client, super::super::scan::SEARCHING_COLOUR);
    assert!(
        green.iter().any(
            |(middle, radius)| (radius - SCAN_RADIUS as f32).abs() < 20.0
                && middle.distance(Vec2::new(radar.at.x.to_f32(), radar.at.y.to_f32())) < 5.0
        ),
        "the world ring of the scan: {green:?}"
    );
    assert_eq!(badge(&mut client), None, "no charge, no badge");

    // The enemy's hero stands in its fountain.
    client.cheat(Cheat::Refresh);
    client.within(Duration::from_secs(20), "a charge again", |c| {
        charges(c) == Some(1)
    });
    take_up_by_key(&mut client);
    // Where the enemy's hero stands is read once with the vision cheat, which is then taken
    // off: the scan has to find it in the fog.
    client.cheat(Cheat::AllVision { on: true });
    let mine = client.me().team;
    client.within(Duration::from_secs(20), "the enemy hero in view", |c| {
        c.view()
            .units
            .iter()
            .any(|u| u.kind == bota_proto::UnitKind::Hero && u.team != mine)
    });
    let enemy = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == bota_proto::UnitKind::Hero && u.team != mine)
        .map(|u| u.pos)
        .expect("seen");
    client.cheat(Cheat::AllVision { on: false });
    client.within(Duration::from_secs(20), "the enemy hero in the fog", |c| {
        !c.view()
            .units
            .iter()
            .any(|u| u.kind == bota_proto::UnitKind::Hero && u.team != mine)
    });
    let fountain = enemy;
    let pixel = minimap_pixel(&mut client, fountain);
    // The pointer is on the minimap once the interface's picking has found it there.
    let frame = crate::minimap::frame(client.world());
    client.within(Duration::from_secs(20), "the pointer on the minimap", |c| {
        c.mouse(pixel);
        c.world()
            .get::<bevy::picking::hover::DirectlyHovered>(frame)
            .is_some_and(|hover| hover.get())
    });
    client.click(MouseButton::Left);
    assert!(
        !client.core().scan_aim,
        "the minimap click aims it (pixel {pixel:?}, the enemy {fountain:?})"
    );
    client.within(Duration::from_secs(20), "the second scan finds", |c| {
        c.core()
            .scan()
            .is_some_and(|side| side.radars.iter().any(|radar| radar.found))
    });
    client.frames(3);
    assert!(
        !rings_in(&client, super::super::scan::FOUND_COLOUR).is_empty(),
        "the world ring turns red"
    );
    let sounds = heard.lock().expect("heard").clone();
    assert_eq!(
        sounds
            .iter()
            .filter(|e| *e == "scan_minimap.activate")
            .count(),
        2,
        "one at each placing: {sounds:?}"
    );
    assert!(sounds.iter().any(|e| e == "minimap_radar.cycle"));
    assert!(
        sounds.iter().any(|e| e == "minimap_radar.target"),
        "the find sounds: {sounds:?}"
    );
}

/// Without a charge the key takes up nothing and Dota's words are shown: "Recharging...".
#[test]
fn a_scan_with_no_charge_is_not_taken_up_and_says_it_recharges() {
    let (_server, mut client) = scene();
    take_up_by_key(&mut client);
    assert!(!client.core().scan_aim);
    client.within(Duration::from_secs(10), "the refusal told", |c| {
        c.shown_texts()
            .iter()
            .any(|text| text.contains("Recharging"))
    });
}

/// A right click or Escape lays the aim down, and the click that would have aimed it moves nothing.
#[test]
fn a_right_click_and_escape_lay_the_aim_of_scan_down() {
    let (_server, mut client) = scene();
    client.cheat(Cheat::Refresh);
    client.within(Duration::from_secs(20), "a charge", |c| {
        charges(c) == Some(1)
    });
    for by_escape in [false, true] {
        take_up_by_key(&mut client);
        assert!(client.core().scan_aim);
        if by_escape {
            client.key(KeyCode::Escape, ButtonState::Pressed);
            client.key(KeyCode::Escape, ButtonState::Released);
        } else {
            let pixel = client
                .screen_of((MID.0 as f32, MID.1 as f32))
                .expect("on screen");
            client.mouse(pixel);
            client.click(MouseButton::Right);
        }
        assert!(!client.core().scan_aim, "escape: {by_escape}");
    }
    assert_eq!(charges(&client), Some(1), "nothing was spent");
}

/// A recorded match in which Radiant's seat scans the Dire fountain once, saved while the scan
/// still looks.
pub(super) fn recorded_scan() -> super::fog_pov::ReplayFile {
    let mut game = super::fog_pov::Spectated::start("2");
    game.cheat(Cheat::Refresh);
    game.wait("a charge told", |driver, _| {
        driver.scan().is_some_and(|side| side.ledger.charges == 1)
    });
    let fountain = bota_core::map_of(bota_core::MAP2_ID).fountains[1];
    game.order(bota_proto::Order::Scan { at: fountain });
    game.wait("the scan looks", |driver, _| {
        driver.scan().is_some_and(|side| !side.radars.is_empty())
    });
    // The match goes on, a second of looks recorded, with the scan still running.
    for _ in 0..30 {
        game.cheat(Cheat::Wtf { on: false });
    }
    game.save()
}

/// What a replay shows of a scan: its own side's eyes the scan's rings and charges, the other
/// side's nothing, the eyes of nobody in particular the scan of the side shown.
#[test]
fn a_replay_shows_a_scan_to_the_side_that_used_it_and_to_nobody_else() {
    let file = recorded_scan();
    for (pov, sees) in [(0, true), (1, false)] {
        let mut client = super::fog_pov::replay_observed(&file, Some(pov), &[], |_| {});
        client.frames(4);
        let scan = client.core().scan();
        assert_eq!(
            scan.as_ref().is_some_and(|side| !side.radars.is_empty()),
            sees,
            "seat {pov}: {scan:?}"
        );
        let rings = rings_in(&client, super::super::scan::SEARCHING_COLOUR).len()
            + rings_in(&client, super::super::scan::FOUND_COLOUR).len();
        assert_eq!(rings > 0, sees, "seat {pov}: rings drawn {rings}");
    }
}
