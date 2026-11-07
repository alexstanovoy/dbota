//! A hero's channel at a Watcher as Dota shows it: the Watcher plays its channel animation and
//! the lower middle of the screen shows the channel bar, which fills as the channel runs and goes
//! when it ends.

use bevy::prelude::*;
use bota_client_core::Intent;
use bota_client_core::{IssuedOrder, Slot};
use bota_proto::{
    AbilityId, Cheat, EntityId, HeroId, Order, StatusFlags, Target, UnitKind, Vec2 as Spot,
};

use super::harness::{Client, Server, scripted_seat};
use crate::channel_bar::{ChannelBar, ChannelFill};

/// Ticks beyond the lamp's duration a channel may be seen for.
const CHANNEL_MARGIN_TICKS: u32 = 15;

/// Whether one's hero is channelling.
fn channelling(client: &Client) -> bool {
    let hero = client.core().my_hero().expect("one's hero");
    client
        .view()
        .unit(hero)
        .is_some_and(|u| u.statuses.bits & StatusFlags::CHANNELLING != 0)
}

fn watcher_clip(client: &Client, watcher: EntityId) -> Option<&'static str> {
    let body = client.body(watcher)?;
    Some(client.world().get::<crate::anim::Pose>(body)?.clip())
}

/// The bar's visibility and how full it is, in percent.
fn bar(client: &mut Client) -> (bool, f32) {
    let world = client.world_mut();
    let shown = world
        .query_filtered::<&InheritedVisibility, With<ChannelBar>>()
        .single(world)
        .expect("the channel bar")
        .get();
    let fill = world
        .query_filtered::<&Node, With<ChannelFill>>()
        .single(world)
        .expect("the channel bar's fill");
    let Val::Percent(share) = fill.width else {
        panic!("the fill is sized in percent, not {:?}", fill.width);
    };
    (shown, share)
}

#[test]
fn a_watcher_channels_in_its_animation_and_the_bar_shows_the_channel_near_the_bottom() {
    let server = Server::start(1, &["--map", "0", "--objectives", "watchers"]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    let at = bota_core::DOTA_WATCHERS[4];
    client.stand_at((at.x.to_int() + 100, at.y.to_int()));
    let watcher = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher")
        .id;
    assert!(!bar(&mut client).0, "no bar while nothing is channelled");
    client.core_mut().act(Intent::Smart {
        at: (at.x.to_f32(), at.y.to_f32()),
        unit: Some(watcher),
    });
    client.within(
        std::time::Duration::from_secs(30),
        "the channel under way, drawn",
        |c| channelling(c) && bar(c).0 && watcher_clip(c, watcher) == Some("channel"),
    );
    let (root, window) = {
        let world = client.world_mut();
        let root = world
            .query_filtered::<Entity, With<ChannelBar>>()
            .single(world)
            .expect("the channel bar");
        let window = world
            .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
            .single(world)
            .expect("the window");
        (
            root,
            Vec2::new(
                window.physical_width() as f32,
                window.physical_height() as f32,
            ),
        )
    };
    let middle = client.centre_of(root).expect("the bar is laid out");
    let size = client
        .world()
        .get::<bevy::ui::ComputedNode>(root)
        .expect("the bar is laid out")
        .size();
    let dota_pixel = window.y / 1080.0;
    assert!(
        (middle.x - window.x / 2.0).abs() < 1.5,
        "the bar is centred across: its middle {middle} against the window's {}",
        window / 2.0
    );
    assert!(
        middle.y > window.y * 0.7,
        "the bar is near the bottom: its middle {middle} in a window of {window}"
    );
    let bottom = middle.y + size.y / 2.0;
    assert!(
        (window.y - bottom - 250.0 * dota_pixel).abs() < 1.5,
        "the bar's bottom edge stands 250 of Dota's pixels above the window's: it is at {bottom} \
         in a window of {window}"
    );
    assert!(
        size.x <= 380.0 * dota_pixel + 1.0 && size.y <= 64.0 * dota_pixel + 1.0,
        "the bar is no bigger than 380x70 of Dota's pixels, it is {size} in a window of {window}"
    );
    let mut fills = vec![bar(&mut client).1];
    // The channel is bounded by the lamp's own duration; a margin of ticks is for the view's lag.
    let began = client.tick();
    let lamp = bota_core::ability_def(AbilityId::LAMP_USE)
        .expect("the lamp")
        .duration;
    while channelling(&client) {
        assert!(
            client.tick() <= began + lamp + CHANNEL_MARGIN_TICKS,
            "the channel outlasts the lamp's {lamp} ticks"
        );
        client.frames(1);
        if channelling(&client) {
            fills.push(bar(&mut client).1);
        }
    }
    assert!(
        fills.iter().all(|f| (0.0..=100.0).contains(f)),
        "the fill stays inside its well: {fills:?}"
    );
    assert!(
        fills.windows(2).all(|pair| pair[0] <= pair[1]) && fills.last() > fills.first(),
        "the fill grows with the channel: {fills:?}"
    );
    client.until("the channel over", |c| !channelling(c));
    client.until("the bar gone", |c| !bar(c).0);
    assert_ne!(watcher_clip(&client, watcher), Some("channel"));
}

/// An enemy hero's channel at a Watcher, seen through shared vision, animates the Watcher but is
/// no bar on one's own screen: the bar is of one's own hero's channel.
#[test]
fn an_enemys_channel_at_a_watcher_shows_no_bar() {
    let server = Server::start(2, &["--map", "0", "--objectives", "watchers"]);
    let at = bota_core::DOTA_WATCHERS[4];
    let mut stage = 0;
    // The enemy starts its channel only when the test is watching: it lasts a second.
    let go = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let start = go.clone();
    scripted_seat(&server, HeroId::PUDGE, move |session| {
        let Some(view) = session.current() else {
            return Vec::new();
        };
        let Some(watcher) = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        else {
            return Vec::new();
        };
        let order = match stage {
            0 => Order::Cheat {
                cheat: Cheat::Teleport {
                    at: at + Spot::from_ints(-100, 0),
                },
            },
            1 if start.load(std::sync::atomic::Ordering::SeqCst) => Order::Attack {
                target: Target::Unit(watcher.id),
            },
            _ => return Vec::new(),
        };
        stage += 1;
        vec![IssuedOrder::hero(order)]
    });
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.cheat(Cheat::StartGame);
    client.cheat(Cheat::AllVision { on: true });
    let watcher = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Watcher && u.pos == at)
        .expect("the watcher")
        .id;
    let mine = client.core().my_hero().expect("one's hero");
    // Asset loading can outlast the whole short channel. Keep the first complete channel
    // tick on screen while the model loads, rather than racing its next snapshots. A held
    // link also hides a real disconnect, so this fixture checks presentation, not liveness.
    let mut channel_tick = None;
    client.hold_incoming_after(move |message| match message {
        bota_proto::ServerMsg::Snapshot { view } => {
            if view.units.iter().any(|unit| {
                unit.kind == UnitKind::Hero
                    && unit.id != mine
                    && unit.statuses.bits & StatusFlags::CHANNELLING != 0
            }) {
                channel_tick.get_or_insert(view.tick);
            }
            false
        }
        bota_proto::ServerMsg::Events { tick, .. } => channel_tick == Some(*tick),
        _ => false,
    });
    go.store(true, std::sync::atomic::Ordering::SeqCst);
    let enemy_channels = |c: &Client| {
        c.view().units.iter().any(|u| {
            u.kind == UnitKind::Hero
                && u.id != mine
                && u.statuses.bits & StatusFlags::CHANNELLING != 0
        })
    };
    client.within(
        std::time::Duration::from_secs(60),
        "the enemy channelling at the Watcher, drawn",
        |c| enemy_channels(c) && watcher_clip(c, watcher) == Some("channel"),
    );
    assert!(!bar(&mut client).0, "no bar for an enemy's channel");
}

/// A teleport has its own bar over the hero (see `bars`), not the one in the middle of the screen.
#[test]
fn a_teleport_keeps_its_own_bar_and_shows_none_in_the_middle_of_the_screen() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.follow_me();
    client.cheat(Cheat::Refresh);
    let mine = client.me().team;
    let tower = client
        .view()
        .units
        .iter()
        .find(|u| u.kind == UnitKind::Tower && u.team == mine)
        .map(|u| (u.id, u.pos))
        .expect("a tower of one's side");
    let core = client.core_mut();
    core.act(Intent::Press {
        slot: Slot::Item(bota_client_core::TOWN_PORTAL_SLOT),
        ctrl: false,
    });
    core.act(Intent::Aimed {
        at: (tower.1.x.to_f32(), tower.1.y.to_f32()),
        unit: Some(tower.0),
    });
    let hero = client.core().my_hero().expect("one's hero");
    client.within(
        std::time::Duration::from_secs(30),
        "the teleport channelled",
        |c| crate::teleport::channel_share(c.view(), hero).is_some(),
    );
    client.frames(3);
    assert!(!bar(&mut client).0, "the teleport has no bar in the middle");
}
