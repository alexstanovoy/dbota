//! The inventory of the strategy phase: what the hero will carry as the
//! choices fill it, laid out in the slots the player puts them in. The
//! bottom panel's own item boxes show it (`hud::subject_item`), drag and
//! context menu work on it as on the real inventory, and when the match
//! begins, the items bought one by one are moved into these slots by the
//! orders any inventory move is.

use bevy::prelude::*;
use bota_client_core::{BAG_SLOTS, Intent};
use bota_core::item_def;
use bota_proto::{ItemId, ItemView, SlotId};

use crate::select::TacticalPhase;
use crate::session::Session;

/// Slots of the bag: the inventory's six and the backpack's three.
const SLOTS: usize = BAG_SLOTS as usize;
/// Seconds after the match begins that the items still have to arrive for
/// them to be put in their places.
const ARRANGE_PATIENCE: f32 = 20.0;

/// The slots of the strategy phase: an item and its charges, or none.
#[derive(Resource, Default, Clone, Debug, PartialEq)]
pub struct TacticalBag {
    slots: [Option<(u16, u16)>; SLOTS],
}

impl TacticalBag {
    /// Follows what is chosen to buy: what is no longer chosen leaves its
    /// slot, what is newly chosen takes the slot of what it replaces (a stack
    /// that grew) or else the first free one. Items beyond the slots are
    /// not shown: the match puts them in the stash.
    pub fn sync(&mut self, queue: &[u16]) {
        let mut wanted = crate::strategy::slots_of(queue);
        let mut freed: Vec<(usize, u16)> = Vec::new();
        for (at, slot) in self.slots.iter_mut().enumerate() {
            let Some(entry) = *slot else { continue };
            match wanted.iter().position(|w| *w == entry) {
                Some(i) => {
                    wanted.remove(i);
                }
                None => {
                    freed.push((at, entry.0));
                    *slot = None;
                }
            }
        }
        for entry in wanted {
            let at = freed
                .iter()
                .position(|(_, id)| *id == entry.0)
                .map(|i| freed.remove(i).0)
                .or_else(|| self.slots.iter().position(Option::is_none));
            if let Some(at) = at {
                self.slots[at] = Some(entry);
            }
        }
    }

    /// The item in a slot, as the real inventory's view of it.
    pub fn item(&self, slot: u8, owner: SlotId) -> Option<ItemView> {
        let (id, count) = (*self.slots.get(usize::from(slot))?)?;
        let charged = item_def(ItemId(id)).is_some_and(|d| d.charges > 0);
        Some(ItemView {
            id: ItemId(id),
            charges: charged.then(|| u8::try_from(count).unwrap_or(u8::MAX)),
            cooldown_left: 0,
            mute_left: 0,
            mode: None,
            mana_cost: 0,
            range: 0,
            aim: None,
            for_sale: false,
            owner,
            selling: None,
        })
    }

    /// The id of the item in a slot.
    pub fn id_at(&self, slot: u8) -> Option<u16> {
        self.slots.get(usize::from(slot))?.map(|(id, _)| id)
    }

    /// Moves what is in one slot into another, what is there coming back.
    pub fn swap(&mut self, from: u8, to: u8) {
        let (from, to) = (usize::from(from), usize::from(to));
        if from < SLOTS && to < SLOTS {
            self.slots.swap(from, to);
        }
    }

    /// The item ids by slot.
    fn ids(&self) -> [Option<u16>; SLOTS] {
        self.slots.map(|s| s.map(|(id, _)| id))
    }
}

/// Where the match is to put the items once it has bought them.
#[derive(Resource, Default)]
struct Arrange {
    wanted: Option<[Option<u16>; SLOTS]>,
    /// When it began, and the tick of the last move ordered.
    since: f32,
    last_tick: u32,
}

pub fn plugin(app: &mut App) {
    app.init_resource::<TacticalBag>()
        .init_resource::<Arrange>()
        .add_systems(Update, (follow_queue, arrange).chain());
}

/// Keeps the bag in step with the choices while the strategy phase is on.
fn follow_queue(
    session: NonSend<Session>,
    phase: Res<TacticalPhase>,
    mut bag: ResMut<TacticalBag>,
) {
    if phase.0 {
        let mut next = bag.clone();
        next.sync(&session.0.queued_buys);
        if *bag != next {
            *bag = next;
        }
    }
}

/// Once the match has bought the choices, puts each in the slot it was
/// given: a swap a tick, the next when the view shows the last.
fn arrange(
    session: NonSendMut<Session>,
    phase: Res<TacticalPhase>,
    time: Res<Time>,
    bag: Res<TacticalBag>,
    mut plan: ResMut<Arrange>,
    mut was_tactical: Local<bool>,
) {
    let now = time.elapsed_secs();
    let core = &mut session.into_inner().0;
    if *was_tactical && !phase.0 {
        let ids = bag.ids();
        let in_order =
            ids.iter().flatten().count() == ids.iter().take_while(|s| s.is_some()).count();
        // A bag filled from the first slot on, as the match fills it, needs no moves.
        plan.wanted = (!in_order).then_some(ids);
        plan.since = now;
    }
    *was_tactical = phase.0;
    let Some(wanted) = plan.wanted else { return };
    let Some(tick) = core.view().map(|v| v.tick) else {
        return;
    };
    if now - plan.since > ARRANGE_PATIENCE || phase.0 {
        plan.wanted = None;
        return;
    }
    let held: Vec<Option<u16>> = (0..BAG_SLOTS)
        .map(|s| core.item_in(s).map(|i| i.id.0))
        .collect();
    let mut have: Vec<u16> = held.iter().flatten().copied().collect();
    let mut want: Vec<u16> = wanted.iter().flatten().copied().collect();
    have.sort_unstable();
    want.sort_unstable();
    // Not all bought yet (or some refused): wait for them, then give up.
    if have != want {
        return;
    }
    if tick == plan.last_tick {
        return;
    }
    // The first item out of its place goes to a slot that wants it.
    let Some(from) = (0..SLOTS).find(|s| held[*s].is_some() && held[*s] != wanted[*s]) else {
        plan.wanted = None;
        return;
    };
    let to = (0..SLOTS).find(|t| *t != from && wanted[*t] == held[from] && held[*t] != wanted[*t]);
    if let Some(to) = to {
        plan.last_tick = tick;
        core.act(Intent::Swap {
            from: from as u8,
            to: to as u8,
        });
    }
}
