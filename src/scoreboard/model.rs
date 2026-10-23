//! What each row of the scoreboard says, apart from any drawing: worked out
//! from the view alone, and only what the watching side may know.
//!
//! A player is told of the other side what Dota's scoreboard tells: the
//! score, the level, the last hits and denies the view gives every seat, and
//! the items as they were when its hero was last seen (Dota's "last seen
//! inventory state"); never its gold, its net worth (gold and items), or its
//! ultimate. A replay and a spectator are told both sides in full, as far as
//! the view goes.

use bota_client_core::App as Core;
use bota_proto::{
    AbilityView, HeroId, ItemId, ItemView, PlayerView, SlotId, Talents, Team, UnitKind, UnitView,
    WorldView,
};

/// Inventory and backpack slots a row shows, as the hero panel has them.
pub const BAG: usize = 9;

/// The ultimate of an allied hero, as Dota's diamond tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ult {
    /// No point in it yet.
    NotLearned,
    Ready,
    /// Off cooldown, but more mana than the hero has.
    NoMana,
    /// Seconds left on its cooldown.
    Cooldown(u32),
}

/// One seat's row.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub slot: SlotId,
    pub team: Team,
    pub hero: HeroId,
    pub level: u8,
    pub xp: i32,
    /// The name the player gave, empty when none.
    pub name: String,
    /// Whether this is the seat being played or watched as.
    pub local: bool,
    /// Whether it plays for the other side than the one playing: Dota hides
    /// its gold and ultimate, and draws its level plainly.
    pub foe: bool,
    pub kills: u16,
    pub deaths: u16,
    pub assists: u16,
    pub last_hits: u16,
    pub denies: u16,
    /// Seconds until the hero comes back, while it is down.
    pub respawn: Option<u32>,
    /// Unspent gold, where it may be known.
    pub gold: Option<i32>,
    /// Gold and the worth of every item the seat owns, where both may be
    /// known.
    pub net_worth: Option<i32>,
    /// The ultimate, where it may be known.
    pub ult: Option<Ult>,
    /// The talents learned, as last told.
    pub talents: Talents,
    /// The inventory and backpack, as last seen; empty until the hero has
    /// been seen.
    pub items: [Option<ItemId>; BAG],
    /// Whether the items are as last seen rather than as they are now.
    pub stale: bool,
}

/// The side whose secrets are kept from the other: the playing seat's; none
/// for a replay (watched as a seat or not) and a spectator.
pub fn side(core: &Core) -> Option<Team> {
    if core.my_slot.is_some() && core.pov.is_none() {
        core.my_team()
    } else {
        None
    }
}

/// Every seat's row, in seat order.
pub fn lines(core: &Core) -> Vec<Line> {
    let Some(view) = core.view() else {
        return Vec::new();
    };
    let side = side(core);
    let rate = u32::from(core.tick_rate().max(1));
    view.players
        .iter()
        .map(|p| line(core, view, p, side, rate))
        .collect()
}

fn line(core: &Core, view: &WorldView, p: &PlayerView, side: Option<Team>, rate: u32) -> Line {
    let foe = side.is_some_and(|team| team != p.team);
    let (bag, stale) = bag(core, p);
    let gold = p.gold.filter(|_| !foe);
    Line {
        slot: p.slot,
        team: p.team,
        hero: p.hero,
        level: p.level,
        xp: p.xp,
        name: core.player_name(p.slot).unwrap_or_default().to_owned(),
        local: core.my_slot == Some(p.slot),
        foe,
        kills: p.kills,
        deaths: p.deaths,
        assists: p.assists,
        last_hits: p.last_hits,
        denies: p.denies,
        respawn: (p.respawn_left > 0).then(|| p.respawn_left.div_ceil(rate)),
        gold,
        net_worth: gold.map(|gold| gold.saturating_add(owned_worth(view, p, bag.as_ref()))),
        ult: if foe { None } else { ult(view, p, rate) },
        talents: talents(core, p),
        items: bag.as_ref().map_or([None; BAG], |b| {
            std::array::from_fn(|at| b.items.get(at).copied().flatten().map(|i| i.id))
        }),
        stale,
    }
}

/// What a hero carries: its inventory and backpack, and its town portal
/// slot.
struct Bag {
    items: Vec<Option<ItemView>>,
    town_portal: Option<ItemView>,
}

/// The hero's bag as the view tells it now (a fallen ally's kit, a hero in
/// sight), else as it was when last seen; and whether it is the latter.
fn bag(core: &Core, p: &PlayerView) -> (Option<Bag>, bool) {
    if let Some(kit) = &p.kit {
        let bag = Bag {
            items: kit.items.clone(),
            town_portal: kit.town_portal,
        };
        return (Some(bag), false);
    }
    let Some((unit, age)) = core.body_of(p.slot).and_then(|id| core.known(id)) else {
        return (None, false);
    };
    let bag = Bag {
        items: unit.items.clone(),
        town_portal: unit.town_portal,
    };
    (Some(bag), age > 0)
}

/// The worth of every item the seat owns that the view carries: in its
/// hero's bag, in its stash and on its side's couriers.
fn owned_worth(view: &WorldView, p: &PlayerView, bag: Option<&Bag>) -> i32 {
    let carried = bag
        .into_iter()
        .flat_map(|b| b.items.iter().flatten().chain(&b.town_portal));
    let stashed = p.stash.iter().flatten().flatten();
    let couriered = view
        .units
        .iter()
        .filter(|u| u.kind == UnitKind::Courier && u.team == p.team)
        .flat_map(|u| u.items.iter().flatten());
    carried
        .chain(stashed)
        .chain(couriered)
        .filter(|item| item.owner == p.slot)
        .map(worth)
        .fold(0i32, i32::saturating_add)
}

/// What an item counts for in net worth: its price, or the price of the
/// charges left of one bought with several.
fn worth(item: &ItemView) -> i32 {
    let Some(def) = bota_core::item_def(item.id) else {
        return 0;
    };
    match item.charges {
        Some(left) if def.charges > 0 => {
            let value = i64::from(def.cost) * i64::from(left) / i64::from(def.charges);
            i32::try_from(value).unwrap_or(i32::MAX)
        }
        _ => def.cost,
    }
}

/// The ultimate of a hero the view tells of: standing, its slots and mana;
/// fallen, its kit.
fn ult(view: &WorldView, p: &PlayerView, rate: u32) -> Option<Ult> {
    let unit: Option<&UnitView> = p.unit.and_then(|id| view.unit(id));
    let (abilities, mana): (&[AbilityView], Option<i32>) = match (unit, &p.kit) {
        (Some(u), _) => (&u.abilities, Some(u.mana)),
        (None, Some(kit)) => (&kit.abilities, None),
        (None, None) => return None,
    };
    let held = abilities
        .iter()
        .find(|a| bota_core::ability_def(a.id).is_some_and(|def| def.ultimate))?;
    Some(if held.level == 0 {
        Ult::NotLearned
    } else if held.cooldown_left > 0 {
        Ult::Cooldown(held.cooldown_left.div_ceil(rate))
    } else if mana.is_some_and(|mana| mana < held.mana_cost) {
        Ult::NoMana
    } else {
        Ult::Ready
    })
}

/// The talents the view tells of, else the ones last told of the hero.
fn talents(core: &Core, p: &PlayerView) -> Talents {
    if let Some(t) = core.talents_of(p.slot) {
        return t.learned;
    }
    match (core.session(), core.body_of(p.slot)) {
        (Some(session), Some(id)) => session.talents_of(id),
        _ => Talents::default(),
    }
}
