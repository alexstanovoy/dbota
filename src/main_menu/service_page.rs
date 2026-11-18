//! The way in's services: the list to choose from and add to (the Service page), and the line
//! that tells the sign-in which service it is for and whether that one works.

use bevy::prelude::*;

use super::core::{Act, Page};
use super::kit::{Kit, Tone};
use super::pages::{field_box, way_in_column, without_account};
use super::service_words::{Tone as Finding, Words};
use super::services::ServiceAct;
use super::view::{ServiceBanner, ServiceRow, ServicesView, TypedService};
use crate::theme;

/// The Service page's card.
const CARD_WIDTH: f32 = 620.0;
/// Height of the buttons of a service's row.
const ROW_BUTTON: f32 = 32.0;

/// The colour of a finding.
fn ink(finding: Finding) -> Color {
    match finding {
        Finding::Idle => theme::INK_4,
        Finding::Waiting => theme::GOLD,
        Finding::Good => theme::GO.0,
        Finding::Bad => theme::ERROR_TEXT,
    }
}

/// The finding, and what to do about it, as lines of `parent`.
fn finding(kit: &mut Kit, parent: Entity, words: &Words) {
    kit.text(parent, words.headline.clone(), 16.0, ink(words.tone));
    if !words.advice.is_empty() {
        kit.text(parent, words.advice.clone(), 14.0, theme::INK_3);
    }
}

/// What the sign-in and registration cards say of their service: which one, and what its check
/// found; a service that failed offers its check again.
pub fn banner(kit: &mut Kit, card: Entity, banner: &ServiceBanner) {
    let block = kit.node(
        card,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            ..default()
        },
    );
    kit.text(block, "Service", 13.0, theme::INK_5);
    kit.text(block, banner.name.clone(), 18.0, theme::INK_1);
    kit.text(block, banner.address.clone(), 14.0, theme::INK_4);
    finding(kit, block, &banner.words);
    if !banner.open && !banner.checking {
        kit.button(
            card,
            "CHECK AGAIN",
            Act::Service(ServiceAct::Recheck(banner.address.clone())),
            Tone::Primary,
            44.0,
        );
    }
}

/// The Service page: Dota's logo and DBOTA over a card of the services, each with the finding of
/// its check and its buttons, the field to add one, and the way back to sign-in.
pub fn draw(kit: &mut Kit, root: Entity, view: &ServicesView) {
    let column = way_in_column(kit, root, true);
    let card = kit.panel(
        column,
        Node {
            width: px(CARD_WIDTH),
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            padding: UiRect::all(px(28)),
            row_gap: px(14),
            ..default()
        },
    );
    kit.typed(card, "MATCHMAKING SERVICE", theme::CARD_TITLE, theme::INK_1);
    kit.text(
        card,
        "Choose the service to sign in to. Each one is checked before you can sign in to it.",
        14.0,
        theme::INK_4,
    );
    for row in &view.rows {
        service_row(kit, card, row);
    }
    kit.typed(card, "ADD A SERVICE", theme::HEADING, theme::INK_1);
    field_box(kit, card, 0, &view.field);
    match &view.typed {
        TypedService::Nothing => {
            kit.text(
                card,
                "Type its address, like bota.lubush.space, or paste the address of its page.",
                13.0,
                theme::INK_5,
            );
        }
        TypedService::Address(address) => {
            kit.text(
                card,
                format!("Will be added as {address}"),
                13.0,
                theme::GO.0,
            );
        }
        TypedService::Refused(why) => {
            kit.text(card, why.clone(), 13.0, theme::ERROR_TEXT);
        }
    }
    let actions = kit.node(
        card,
        Node {
            column_gap: px(10),
            margin: UiRect::top(px(6)),
            ..default()
        },
    );
    kit.button(actions, "ADD", Act::Submit, Tone::Primary, 44.0);
    kit.button(actions, "BACK", Act::Go(Page::Login), Tone::Secondary, 44.0);
    without_account(kit, column);
}

fn service_row(kit: &mut Kit, card: Entity, row: &ServiceRow) {
    let panel = kit.panel(
        card,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(12)),
            row_gap: px(4),
            ..default()
        },
    );
    let title = kit.node(
        panel,
        Node {
            column_gap: px(10),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.text(title, row.name.clone(), 17.0, theme::INK_1);
    if row.selected {
        kit.text(title, "SELECTED", 12.0, theme::GOLD);
    }
    kit.text(panel, row.address.clone(), 14.0, theme::INK_4);
    finding(kit, panel, &row.words);
    let buttons = kit.node(
        panel,
        Node {
            column_gap: px(8),
            row_gap: px(8),
            margin: UiRect::top(px(6)),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        },
    );
    if !row.selected {
        kit.button(
            buttons,
            "USE",
            Act::Service(ServiceAct::Use(row.address.clone())),
            Tone::Primary,
            ROW_BUTTON,
        );
    }
    if !row.checking {
        kit.button(
            buttons,
            "CHECK AGAIN",
            Act::Service(ServiceAct::Recheck(row.address.clone())),
            Tone::Secondary,
            ROW_BUTTON,
        );
    }
    if row.removable {
        kit.button(
            buttons,
            "REMOVE",
            Act::Service(ServiceAct::Remove(row.address.clone())),
            Tone::Danger,
            ROW_BUTTON,
        );
    }
}
