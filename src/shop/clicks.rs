//! What clicks in the shop do, as dbota has always had them: a left click on
//! an item, or Enter in the search box, buys it (and shows its build tree);
//! a right click makes it the quick-buy item, as a shift-click does in Dota;
//! an item dropped on the panel's foot is sold. In the tactical phase a right
//! click takes a choice off the queue.

use bevy::prelude::*;
use bota_client_core::{Intent, Source};

use super::cell::{Chosen, Place, ShopCell};
use super::corner::{Grab, GrabAll, ShopButton};
use super::panel::{SearchBox, SellStrip, Shown, TabButton};
use crate::input::{Panels, Typing};
use crate::session::Session;

pub fn plugin(app: &mut App) {
    app.add_systems(Update, (buttons, cells, sell).run_if(super::shop_runs));
}

/// The shop's buttons: the gold opens and closes the shop, the search box
/// takes the keyboard, a tab shows its page, GRAB ALL empties the stash, the
/// courier fetches what waits in it.
fn buttons(
    mut session: NonSendMut<Session>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut panels: ResMut<Panels>,
    mut typing: ResMut<Typing>,
    mut shown: ResMut<Shown>,
    mut grab: ResMut<Grab>,
    shop: Single<&Interaction, With<ShopButton>>,
    search: Single<&Interaction, With<SearchBox>>,
    tabs: Query<(&Interaction, &TabButton)>,
    grab_all: Query<&Interaction, With<GrabAll>>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let core = &mut session.0;
    if **shop != Interaction::None {
        panels.shop = !panels.shop;
    }
    if **search != Interaction::None && panels.shop {
        *typing = Typing::Search;
    } else if *typing == Typing::Search {
        *typing = Typing::No;
    }
    if let Some((_, TabButton(tab))) = tabs.iter().find(|(i, _)| **i != Interaction::None)
        && panels.shop
    {
        shown.0 = *tab;
        panels.search.clear();
    }
    let live = matches!(core.source, Source::Live(_));
    if live && grab_all.iter().any(|i| *i != Interaction::None) {
        grab.start(core);
    }
}

/// Clicks on items, wherever the shop shows them.
fn cells(
    mut session: NonSendMut<Session>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    panels: Res<Panels>,
    mut chosen: ResMut<Chosen>,
    cells: Query<(&Interaction, &ShopCell, &Place, &InheritedVisibility)>,
) {
    let left = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    let Some((_, cell, place, _)) = cells
        .iter()
        .find(|(i, _, _, seen)| **i != Interaction::None && seen.get())
    else {
        return;
    };
    let core = &mut session.0;
    let id = cell.0;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    if left && shift || right && *place != Place::Quick && !core.in_tactical_phase() {
        core.quick_buy = if core.quick_buy == Some(id) {
            None
        } else {
            Some(id)
        };
    } else if right && *place == Place::Quick {
        core.quick_buy = None;
    } else if right {
        core.act(Intent::Unqueue(id));
    } else {
        if matches!(core.source, Source::Live(_)) {
            core.act(Intent::Buy(id));
        }
        if panels.shop {
            chosen.0 = Some(id);
        }
    }
}

/// An item dropped on the panel's foot is sold.
fn sell(
    mut session: NonSendMut<Session>,
    mouse: Res<ButtonInput<MouseButton>>,
    panels: Res<Panels>,
    strip: Single<&Interaction, With<SellStrip>>,
) {
    let core = &mut session.0;
    if panels.shop
        && mouse.just_released(MouseButton::Left)
        && **strip != Interaction::None
        && matches!(core.source, Source::Live(_))
        && let Some(slot) = core.held_item.take()
    {
        core.act(Intent::Sell(slot));
    }
}
