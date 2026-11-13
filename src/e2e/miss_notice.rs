//! The miss notice in a real match: one's own Shadow Fiend stands in the mid river (a cheat's
//! teleport) and is right-clicked onto an enemy hero up the Dire bank, so a quarter of its blows
//! miss uphill. Each miss floats Dota's notice over the struck hero: the word "miss", drawn, on
//! the screen above the hero's health bar, in the reference's red and at least as large as the
//! reference draws it beside a damage number, for the reference's 1.5 seconds; shown whether or
//! not damage numbers are, as Dota's overhead alerts are.
//!
//! The reference's look (colour, size beside a damage digit, lifetime) was established from the
//! installed game (build 6975).

use std::time::Duration;

use bevy::input::ButtonState;
use bevy::prelude::*;
use bota_client_core::FloaterKind;
use bota_proto::{
    Cheat, CreepSide, EntityId, HeroId, ItemId, ItemSlot, ModifierSpec, Order, Target, Team,
    UnitKind,
};

use super::harness::{Client, Server};
use crate::bars::DrawnFloater;

/// In the mid river (elevation tier 0) and up its Dire bank (tier 1), in a ranged hero's reach.
const RIVER: (i32, i32) = (8750, 8880);
const BANK: (i32, i32) = (9150, 9150);
/// Up the bank, in an Observer Ward's cast range from the river spot.
const WARD: (i32, i32) = (9000, 9080);
/// The miss notice's colour in the reference.
const REFERENCE_RED: [u8; 3] = [163, 38, 38];
/// The reference's miss notice at its largest against a damage digit.
const REFERENCE_MISS_OVER_DAMAGE: f32 = 25.0 / 16.0;
/// The reference's miss notice lifetime, in seconds.
const REFERENCE_MISS_LIFE: f32 = 1.5;
/// The game seconds each frame stands for: a fixed step, so ages are counted, not timed.
const FRAME: f32 = 1.0 / 30.0;

/// The enemy hero up the bank.
fn foe(client: &Client) -> Option<EntityId> {
    client
        .view()
        .units
        .iter()
        .find(|unit| unit.kind == UnitKind::Hero && unit.team == Team::Dire)
        .map(|unit| unit.id)
}

/// One's own hero in the river right-clicked onto the enemy hero on the bank, every hero's
/// blows harmless to one's own and a quarter as strong (so the enemy outlives the scene), no
/// creeps. The river does not see up the bank: one's own Observer Ward planted on it lends that
/// sight, as a ward or a creep up there does in a match.
fn swinging_uphill(extra: &[&str]) -> (Server, Client, EntityId) {
    let server = Server::start(1, &["--seed", "5"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, extra);
    client
        .world_mut()
        .insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
            Duration::from_secs_f32(FRAME),
        ));
    client.cheat(Cheat::CreepSpawning { on: false });
    client.cheat(Cheat::KillCreeps {
        which: CreepSide::All,
    });
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::ApplyModifier {
        target: Target::None,
        spec: ModifierSpec {
            physical_damage: ModifierSpec::MIN_SCALE,
            ..ModifierSpec::NOMINAL
        },
        ticks: bota_proto::MAX_MODIFIER_TICKS,
    });
    client.stand_at(RIVER);
    plant_a_ward(&mut client);
    client.cheat(Cheat::CreateHero {
        hero: HeroId::SHADOW_FIEND,
        enemy: true,
        at: bota_proto::Vec2::from_ints(BANK.0, BANK.1),
    });
    client.until("the enemy hero stands on the bank", |c| foe(c).is_some());
    let foe = foe(&client).expect("the enemy hero");
    right_click(&mut client, foe);
    (server, client, foe)
}

/// One's own Observer Ward planted on the bank, in reach from the river.
fn plant_a_ward(client: &mut Client) {
    client.cheat(Cheat::Item {
        item: ItemId::OBSERVER_WARD,
    });
    let slot = client
        .me()
        .items
        .iter()
        .position(|item| item.is_some_and(|item| item.id == ItemId::OBSERVER_WARD))
        .expect("the ward in the bag");
    client.core_mut().send_order_to(
        None,
        Order::Use {
            slot: ItemSlot(slot as u8),
            target: Target::Pos(bota_proto::Vec2::from_ints(WARD.0, WARD.1)),
        },
    );
    client.until("one's own ward planted on the bank", |c| {
        c.view()
            .units
            .iter()
            .any(|unit| unit.kind == UnitKind::Ward && unit.team == Team::Radiant)
    });
}

/// Right-clicks the unit where it is drawn, once the pointer is on it.
fn right_click(client: &mut Client, id: EntityId) {
    client.within(
        Duration::from_secs(20),
        "the pointer on the enemy hero",
        |c| {
            let Some(body) = c.body(id) else {
                return false;
            };
            let at = c
                .world()
                .get::<crate::units::Unit>(body)
                .expect("a unit")
                .at;
            let Some(pixel) = c.screen_of((at.x, at.y)) else {
                return false;
            };
            c.mouse(pixel);
            let hover = c.world().resource::<crate::pick::Hover>();
            hover.attack.is_some_and(|(on, _)| on == id)
        },
    );
    client.button(MouseButton::Right, ButtonState::Pressed);
    client.button(MouseButton::Right, ButtonState::Released);
}

/// Runs frames until a miss notice is drawn; it and every caption drawn with it.
fn first_miss_drawn(client: &mut Client) -> (DrawnFloater, Vec<DrawnFloater>) {
    let mut seen = None;
    client.within(Duration::from_secs(90), "a miss notice drawn", |c| {
        let drawn = crate::bars::drawn_floaters(c.world_mut());
        if let Some(index) = drawn.iter().position(|f| f.kind == FloaterKind::Miss) {
            let mut drawn = drawn;
            let miss = drawn.swap_remove(index);
            seen = Some((miss, drawn));
            return true;
        }
        false
    });
    seen.expect("seen")
}

/// The window's pixel a point `over` units above the ground at a bota position is drawn at.
fn screen_above(client: &mut Client, (x, y): (f32, f32), over: f32) -> Option<Vec2> {
    let ground = client.world().resource::<crate::map::Ground>().at(x, y);
    let world = client.world_mut();
    let (camera, place) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<crate::camera::RtsCamera>>()
        .single(world)
        .expect("the game's camera");
    camera
        .world_to_viewport(place, Vec3::new(x, ground + over, -y))
        .ok()
}

#[test]
fn ones_own_miss_uphill_is_drawn_on_screen_over_the_struck_hero_above_its_health_bar() {
    let (_server, mut client, foe) = swinging_uphill(&[]);
    let (miss, _) = first_miss_drawn(&mut client);
    assert_eq!(miss.words, "miss");
    let at = client
        .world()
        .get::<crate::units::Unit>(client.body(foe).expect("the enemy hero drawn"))
        .expect("a unit")
        .at;
    let centre = client.centre_of(miss.entity).expect("laid out");
    let window = {
        let world = client.world_mut();
        let window = world
            .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
            .single(world)
            .expect("the window");
        Vec2::new(
            window.physical_width() as f32,
            window.physical_height() as f32,
        )
    };
    assert!(
        centre.cmpge(Vec2::ZERO).all() && centre.cmplt(window).all(),
        "the notice at {centre} is inside the window {window}"
    );
    let feet = client.screen_of((at.x, at.y)).expect("the hero on screen");
    assert!(
        (centre.x - feet.x).abs() < 40.0,
        "the notice at {centre} stands over the struck hero at {feet}"
    );
    // A hero's health bar floats about 260 units over it (see the bars).
    let bar = screen_above(&mut client, (at.x, at.y), 260.0).expect("the bar's height on screen");
    assert!(
        centre.y + miss.size / 2.0 < bar.y,
        "the notice's foot ({}) is above the health bar ({})",
        centre.y + miss.size / 2.0,
        bar.y
    );
    assert!(
        miss.color.alpha() > 0.9,
        "a fresh notice is opaque: {:?}",
        miss.color
    );
}

#[test]
fn the_miss_notice_has_the_reference_red_and_its_size_beside_damage_numbers() {
    let (_server, mut client, _) = swinging_uphill(&[]);
    let (miss, _) = first_miss_drawn(&mut client);
    let red = miss.color.to_srgba();
    let drawn = [red.red, red.green, red.blue].map(|c| (c * 255.0).round() as u8);
    assert_eq!(drawn, REFERENCE_RED, "the reference's red");
    let mut damage = None;
    client.until("a damage number drawn", |c| {
        damage = crate::bars::drawn_floaters(c.world_mut())
            .into_iter()
            .find(|f| f.kind == FloaterKind::Damage)
            .map(|f| f.size);
        damage.is_some()
    });
    let damage = damage.expect("seen");
    assert!(
        miss.size >= damage * REFERENCE_MISS_OVER_DAMAGE - 0.5,
        "the notice ({} px) is drawn as large beside a damage number ({damage} px) as the \
         reference draws it",
        miss.size
    );
}

#[test]
fn the_miss_notice_lasts_the_reference_lifetime() {
    let (_server, mut client, _) = swinging_uphill(&[]);
    first_miss_drawn(&mut client);
    // The oldest a miss notice grows before it goes, over the next misses: one step short of its
    // life at most.
    let mut oldest = 0.0_f32;
    let mut frames = 0;
    client.until("three seconds of misses", |c| {
        frames += 1;
        oldest = c
            .core()
            .floaters
            .iter()
            .filter(|f| f.kind == FloaterKind::Miss)
            .map(|f| f.age)
            .fold(oldest, f32::max);
        frames as f32 * FRAME > 3.0
    });
    assert!(
        oldest >= REFERENCE_MISS_LIFE - 2.0 * FRAME,
        "a miss notice lasted {oldest} s of the reference's {REFERENCE_MISS_LIFE} s"
    );
}

#[test]
fn the_miss_notice_shows_with_damage_numbers_turned_off() {
    let (_server, mut client, _) = swinging_uphill(&["--set", "damage_numbers=false"]);
    assert!(
        !client
            .world()
            .resource::<crate::config::Settings>()
            .aids
            .numbers,
        "damage numbers are off"
    );
    let (miss, others) = first_miss_drawn(&mut client);
    assert_eq!(miss.words, "miss");
    assert!(
        others.iter().all(|f| f.kind != FloaterKind::Damage),
        "and no damage number is drawn with it"
    );
}
