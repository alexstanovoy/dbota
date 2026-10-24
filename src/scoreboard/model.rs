//! What each row of the scoreboard says, apart from any drawing: worked out
//! from the view alone, and only what the watching side may know.
//!
//! A player is told of the other side what Dota's scoreboard tells: the
//! score, the level as it stood when its hero was last seen, and the items
//! as they were then (Dota's "last seen inventory state"); never its gold or
//! its ultimate. A replay and a spectator are told both sides in full, as far
//! as the view goes.

use bota_client_core::App as Core;
use bota_proto::{
    AbilityView, HeroId, ItemId, ItemView, PlayerView, SlotId, Talents, Team, UnitView, WorldView,
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
    /// Seconds until the hero comes back, while it is down.
    pub respawn: Option<u32>,
    /// Unspent gold, where it may be known.
    pub gold: Option<i32>,
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

/// The side whose view this is, whose secrets the other side's keep: the
/// playing seat's, or the one a spectator watches through; none for a replay
/// (the whole match, watched as a seat or not) and a spectator watching it
/// all.
pub fn side(core: &Core) -> Option<Team> {
    if core.pov.is_some() {
        return None;
    }
    let seat = core.my_slot.or(core.eyes)?;
    Some(core.view()?.player(seat)?.team)
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
    let gold = p.gold;
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
        respawn: (p.respawn_left > 0).then(|| p.respawn_left.div_ceil(rate)),
        gold,
        ult: if foe { None } else { ult(view, p, rate) },
        talents: talents(core, p),
        items: bag.unwrap_or([None; BAG]),
        stale,
    }
}

/// The hero's inventory and backpack as the view tells them now (a fallen
/// ally's kit, a hero in sight), else as they were when last seen; and
/// whether it is the latter.
fn bag(core: &Core, p: &PlayerView) -> (Option<[Option<ItemId>; BAG]>, bool) {
    let ids = |items: &[Option<ItemView>]| {
        std::array::from_fn(|at| items.get(at).copied().flatten().map(|i| i.id))
    };
    if let Some(kit) = &p.kit {
        return (Some(ids(&kit.items)), false);
    }
    let Some((unit, age)) = core.body_of(p.slot).and_then(|id| core.known(id)) else {
        return (None, false);
    };
    (Some(ids(&unit.items)), age > 0)
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
