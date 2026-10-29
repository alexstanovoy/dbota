//! Health bars: each side's colour whoever watches, at Dota's
//! sizes, and a ward's and a courier's health counted in hits.

use bevy::prelude::*;
use bota_proto::{Cheat, EntityId, HeroId, Team, UnitKind, Vec2 as At};

use super::harness::{Client, MID, Server, quiet_seat};

/// The bar drawn over a unit: its node's size in interface pixels and its health's colour.
fn bar(client: &Client, id: EntityId) -> Option<(f32, f32, Srgba)> {
    let bars = client.world().resource::<crate::bars::Bars>();
    let body = client.body(id)?;
    let (root, fill) = (bars.root(body)?, bars.fill(body)?);
    if client.world().get::<Visibility>(root)? == Visibility::Hidden {
        return None;
    }
    let node = client.world().get::<Node>(root)?;
    let (Val::Px(w), Val::Px(h)) = (node.width, node.height) else {
        return None;
    };
    let colour = client.world().get::<BackgroundColor>(fill)?.0.to_srgba();
    Some((w, h, colour))
}

fn green(c: Srgba) -> bool {
    c.green > 2.0 * c.red
}

fn red(c: Srgba) -> bool {
    c.red > 2.0 * c.green
}

/// Every unit of the view with a bar drawn: its id, kind and side.
fn barred(client: &Client) -> Vec<(EntityId, UnitKind, Team)> {
    client
        .view()
        .units
        .iter()
        .filter(|u| bar(client, u.id).is_some())
        .map(|u| (u.id, u.kind, u.team))
        .collect()
}

/// Seen from the Dire seat, a Radiant hero and Radiant creeps have green bars and the Dire
/// creeps red ones, the seat's own hero a red of its own: the colour is the side's, not the
/// watcher's (it was the allies' green and the enemies' red before). A hero's bar is Dota's 110
/// by 10 pixels at 1080 lines and a creep's 55 by 5 (a hero's was 124 by 12, too big).
#[test]
fn health_bars_are_their_sides_colour_whoever_watches_at_dotas_sizes() {
    let server = Server::start(2, &[]);
    quiet_seat(&server, HeroId::SHADOW_FIEND);
    let mut dire = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    assert_eq!(dire.me().team, Team::Dire, "the second seat is Dire's");
    dire.stand_at(MID);
    let near = |dx| At::from_ints(MID.0 + dx, MID.1 - 300);
    for (enemy, dx) in [(false, -400), (true, 400)] {
        dire.cheat(Cheat::CreateCreeps {
            enemy,
            at: near(dx),
        });
    }
    dire.cheat(Cheat::CreateHero {
        hero: HeroId::PUDGE,
        enemy: true,
        at: near(0),
    });
    let wanted = |c: &mut Client| {
        let shown = barred(c);
        let has = |kind: UnitKind, team| shown.iter().any(|u| u.1 == kind && u.2 == team);
        has(UnitKind::Hero, Team::Radiant)
            && has(UnitKind::Hero, Team::Dire)
            && has(UnitKind::CreepMelee, Team::Radiant)
            && has(UnitKind::CreepMelee, Team::Dire)
    };
    dire.until("both sides' heroes and creeps barred", wanted);
    let me = dire.me().id;
    let mut own = None;
    for (id, kind, team) in barred(&dire) {
        let (w, h, colour) = bar(&dire, id).expect("barred");
        let side_colour = match team {
            Team::Radiant => green(colour),
            _ => red(colour),
        };
        assert!(side_colour, "{kind:?} of {team:?}: {colour:?}");
        let size = match kind {
            UnitKind::Hero => Some((110.0, 10.0)),
            UnitKind::CreepMelee | UnitKind::CreepRanged => Some((55.0, 5.0)),
            _ => None,
        };
        if let Some((dw, dh)) = size {
            let dota = (dw * crate::ui::DOTA, dh * crate::ui::DOTA);
            assert_eq!((w, h), dota, "{kind:?}'s bar in Dota's size");
        }
        if id == me {
            own = Some(colour);
        }
    }
    let own = own.expect("one's own hero barred");
    let ally_creep = barred(&dire)
        .into_iter()
        .find(|u| u.1 == UnitKind::CreepMelee && u.2 == Team::Dire)
        .and_then(|u| bar(&dire, u.0))
        .expect("an allied creep barred")
        .2;
    assert_ne!(own, ally_creep, "one's own hero's red is its own");
}

/// The blocks of a unit's bar: how many, and how many stand.
fn blocks(client: &Client, id: EntityId) -> Option<(usize, usize)> {
    let bars = client.world().resource::<crate::bars::Bars>();
    let segments = bars.segments(client.body(id)?);
    let standing = segments
        .iter()
        .filter(|s| client.world().get::<Visibility>(**s) != Some(&Visibility::Hidden))
        .count();
    Some((segments.len(), standing))
}

/// The blocks a health counted in hits shows: one a hit of the blow that takes the least, and
/// as many standing as hits are left.
fn hits(count: bota_core::HitCount, hp: i32, max_hp: i32) -> (usize, usize) {
    let least = count
        .melee_hero
        .min(count.ranged_hero)
        .min(count.other)
        .max(1);
    let of = |health: i32| (health.max(0) as u32).div_ceil(least as u32) as usize;
    (of(max_hp), of(hp))
}

/// A courier's and a planted ward's health is counted in hits (reported: hit-count
/// bars and ward bars): their bars are blocks, one a hit, as many standing as hits are left,
/// at Dota's sizes (a courier's 50 by 6, a ward's 40 by 5).
#[test]
fn a_courier_and_a_ward_show_their_health_in_hits() {
    let server = Server::start(1, &[]);
    let mut client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
    client.stand_at(MID);
    client.cheat(Cheat::Item {
        item: bota_proto::ItemId::OBSERVER_WARD,
    });
    let slot = (0..bota_client_core::BAG_SLOTS)
        .find(|s| {
            client
                .core()
                .item_in(*s)
                .is_some_and(|i| i.id == bota_proto::ItemId::OBSERVER_WARD)
        })
        .expect("the ward in the bag");
    let spot = (MID.0 as f32 + 150.0, MID.1 as f32 + 150.0);
    client.core_mut().act(bota_client_core::Intent::Press {
        slot: bota_client_core::Slot::Item(slot),
        ctrl: false,
    });
    client.core_mut().act(bota_client_core::Intent::Aimed {
        at: spot,
        unit: None,
    });
    let ward = |c: &Client| {
        c.view()
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Ward)
            .map(|u| u.id)
    };
    client.until("the ward planted and barred", |c| {
        ward(c).is_some_and(|id| bar(c, id).is_some())
    });
    let ward = ward(&client).expect("planted");
    let live = client.view().unit(ward).expect("seen").clone();
    let (w, h, _) = bar(&client, ward).expect("barred");
    assert_eq!((w, h), (40.0 * crate::ui::DOTA, 5.0 * crate::ui::DOTA));
    let counted = bota_core::OBSERVER_WARD
        .hit_count
        .expect("a ward's health is in hits");
    assert_eq!(
        blocks(&client, ward),
        Some(hits(counted, live.hp, live.max_hp)),
        "a ward's blocks"
    );
    // The courier waits at the fountain, where the camera goes back to.
    let courier = client.core().my_courier().expect("a courier");
    client.core_mut().pinned = Some(bota_client_core::Pin::Unit(courier));
    client.until("the courier barred", |c| bar(c, courier).is_some());
    let live = client.view().unit(courier).expect("seen").clone();
    let (w, h, _) = bar(&client, courier).expect("barred");
    assert_eq!((w, h), (50.0 * crate::ui::DOTA, 6.0 * crate::ui::DOTA));
    let counted = bota_core::COURIER
        .hit_count
        .expect("a courier's health is in hits");
    let (all, standing) = hits(counted, live.hp, live.max_hp);
    assert!(all > 1, "a courier takes more than one hit");
    assert_eq!(
        blocks(&client, courier),
        Some((all, standing)),
        "a courier's blocks"
    );
}
