//! What clicks in the shop do, as Dota's: a left click on an item chooses it
//! (its build tree shows at the panel's foot), a second one on it soon after
//! buys it, and a right click buys it, as does Enter in the search box; a
//! shift-click replaces Quickbuy and Ctrl+Shift appends; an
//! item dropped on the panel's foot is sold. In the tactical phase what is
//! bought is chosen for the match's start.

use bevy::prelude::*;
use bota_client_core::{Intent, Source};

use super::cell::{Chosen, ShopCell};
use super::corner::{Grab, GrabAll, ShopButton};
use super::panel::{SearchBox, SellStrip, Shown, TabButton};
use crate::input::{Panels, Typing};
use crate::session::Session;

pub fn plugin(app: &mut App) {
    app.add_systems(
        Update,
        (
            buttons,
            cells.before(crate::item_menu::MenuSystems::Choose),
            sell,
            inventory
                .after(crate::input::boxes)
                .after(crate::item_menu::MenuSystems::Open)
                .before(crate::item_menu::MenuSystems::Choose),
        )
            .run_if(super::match_or_tactical),
    );
}

/// Only a release on the pressed inventory slot selects a passive item's tree.
/// Menu presses belong to the menu, not to the inventory behind it.
fn inventory(
    session: NonSend<Session>,
    mouse: Res<ButtonInput<MouseButton>>,
    panels: Res<Panels>,
    bag: Res<crate::bag::TacticalBag>,
    settling: Res<crate::drag::Settling>,
    menu: Res<crate::item_menu::Menu>,
    mut chosen: ResMut<Chosen>,
    mut pressed: Local<Option<u8>>,
    items: Query<(&Interaction, &crate::hud::ItemBox, &InheritedVisibility)>,
) {
    if menu.is_open() {
        *pressed = None;
        return;
    }
    let hovered = items
        .iter()
        .find(|(over, _, seen)| **over != Interaction::None && seen.get())
        .map(|(_, item, _)| item.0);
    if mouse.just_pressed(MouseButton::Left) {
        *pressed = hovered
            .filter(|slot| super::displayed_item(&session.0, &bag, &settling, *slot).is_some());
    }
    if mouse.just_released(MouseButton::Left)
        && let Some(slot) = pressed.take().filter(|slot| Some(*slot) == hovered)
        && panels.shop
        && let Some(item) = super::displayed_item(&session.0, &bag, &settling, slot)
        && item.aim.is_none()
    {
        chosen.0 = Some(item.id.0);
    }
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

/// Seconds within which a second left click on an item buys it (Dota's
/// double click).
const DOUBLE_CLICK: f32 = 0.4;

/// Clicks on items, wherever the shop shows them.
pub(super) fn cells(
    mut session: NonSendMut<Session>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    panels: Res<Panels>,
    mut chosen: ResMut<Chosen>,
    mut last_left: Local<Option<(u16, f32)>>,
    cells: Query<(&Interaction, &ShopCell, &InheritedVisibility)>,
    drag: Res<super::drag::ShopDrag>,
) {
    let left = drag.clicked.is_some();
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    let Some(id) = drag.clicked.or_else(|| {
        cells
            .iter()
            .find(|(i, _, seen)| **i != Interaction::None && seen.get())
            .map(|(_, cell, _)| cell.0)
    }) else {
        return;
    };
    let core = &mut session.0;
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let now = time.elapsed_secs();
    let again = last_left.is_some_and(|(was, at)| was == id && now - at <= DOUBLE_CLICK);
    if left && shift {
        let append = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
        core.set_quick_buy(id, append);
        return;
    }
    if left {
        *last_left = (!again).then_some((id, now));
        if panels.shop {
            chosen.0 = Some(id);
        }
    }
    if (right || again) && matches!(core.source, Source::Live(_)) {
        core.act(Intent::Buy(id));
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
