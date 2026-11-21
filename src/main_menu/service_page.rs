//! The way in's services: the list to choose from and add to (the Service page), and the line
//! that tells the sign-in which service it is for and whether that one works.

use bevy::prelude::*;
use bevy::ui_widgets::{ControlOrientation, ScrollArea, ScrollIntoView, Scrollbar, ScrollbarThumb};

use super::core::{Act, MenuCore, Page};
use super::kit::{Kit, MenuButton, Tone};
use super::pages::{field_box, way_in_column, without_account};
use super::player_matches::MatchScrollbar;
use super::service_words::{Tone as Finding, Words};
use super::services::{BUILT_IN_HOST, ServiceAct};
use super::view::{ServiceBanner, ServiceRow, ServicesView, TypedService};
use crate::theme;

/// The Service page's card.
const CARD_WIDTH: f32 = 620.0;
/// What the card leaves free under it in a window too short for all of it.
const WINDOW_MARGIN: f32 = 24.0;
/// The list's room for its scrollbar.
const SCROLL_GUTTER: f32 = 14.0;
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

/// What the sign-in and registration cards say of a service that passed its check: one quiet
/// line, whose service it is and that it works, beside the way to another service. The address
/// and what the service calls itself are on the Service page; of a service the player typed it
/// is the host that is shown, which a name it gave itself could not stand in for.
pub fn works_line(kit: &mut Kit, card: Entity, banner: &ServiceBanner) {
    let row = kit.node(
        card,
        Node {
            column_gap: px(12),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
    );
    let said = kit.text(
        row,
        format!("{} — works", banner.subject),
        15.0,
        ink(Finding::Good),
    );
    kit.commands.entity(said).insert((
        Name::new("service check summary"),
        Node {
            width: percent(60),
            min_width: px(0),
            ..default()
        },
        TextLayout {
            linebreak: bevy::text::LineBreak::WordOrCharacter,
            ..default()
        },
    ));
    kit.button(
        row,
        "CHANGE SERVICE",
        Act::Go(Page::Service),
        Tone::Secondary,
        28.0,
    );
}

/// The list of services on the Service page: what scrolls.
#[derive(Component)]
pub(super) struct ServiceList;

/// The row of the service signed in to.
#[derive(Component)]
pub(super) struct SelectedService;

/// The Service page: Dota's logo and DBOTA over a card of the services, each with the finding of
/// its check and its buttons, the field to add one, and the way back to sign-in.
pub fn draw(kit: &mut Kit, root: Entity, view: &ServicesView) {
    let column = way_in_column(kit, root, false);
    // The card is as tall as the window leaves it: when the services are more than that, the list
    // scrolls and the field to add one and the way back stay in reach.
    kit.commands
        .entity(column)
        .entry::<Node>()
        .and_modify(|mut node| node.bottom = px(WINDOW_MARGIN));
    let card = kit.panel(
        column,
        Node {
            width: px(CARD_WIDTH),
            flex_direction: FlexDirection::Column,
            flex_shrink: 1.0,
            min_height: px(0),
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
    let (body, list) = list_viewport(kit, card);
    for row in &view.rows {
        service_row(kit, list, row);
    }
    scrollbar(kit, body, list);
    kit.typed(card, "ADD A SERVICE", theme::HEADING, theme::INK_1);
    field_box(kit, card, 0, &view.field);
    match &view.typed {
        TypedService::Nothing => {
            kit.text(
                card,
                format!(
                    "Type its address, like {BUILT_IN_HOST}, or paste the address of its page."
                ),
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

/// The room of the list: as tall as its services, or as the card leaves it, then it scrolls.
fn list_viewport(kit: &mut Kit, card: Entity) -> (Entity, Entity) {
    let body = kit.node(
        card,
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            flex_shrink: 1.0,
            min_height: px(0),
            ..default()
        },
    );
    let list = kit.node(
        body,
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            flex_shrink: 1.0,
            min_height: px(0),
            row_gap: px(10),
            padding: UiRect::right(px(SCROLL_GUTTER)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    );
    kit.commands.entity(list).insert((
        ServiceList,
        ScrollArea,
        Pickable::default(),
        Name::new("service list"),
    ));
    (body, list)
}

fn scrollbar(kit: &mut Kit, body: Entity, list: Entity) {
    let rail = kit.node(
        body,
        Node {
            position_type: PositionType::Absolute,
            right: px(0),
            top: px(0),
            bottom: px(0),
            width: px(8),
            ..default()
        },
    );
    kit.commands.entity(rail).insert((
        MatchScrollbar,
        Visibility::Inherited,
        BackgroundColor(theme::FIELD_SUNKEN),
        Scrollbar::new(list, ControlOrientation::Vertical, 16.0),
    ));
    kit.commands.spawn((
        ScrollbarThumb::default(),
        Name::new("service list scroll thumb"),
        BackgroundColor(theme::FIELD_EDGE),
        ChildOf(rail),
    ));
}

fn service_row(kit: &mut Kit, card: Entity, row: &ServiceRow) {
    let panel = kit.panel(
        card,
        Node {
            flex_direction: FlexDirection::Column,
            // Never squeezed to fit: the list scrolls.
            flex_shrink: 0.0,
            padding: UiRect::all(px(12)),
            row_gap: px(4),
            ..default()
        },
    );
    if row.selected {
        kit.commands.entity(panel).insert(SelectedService);
    }
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

/// Where the list was left, so that it stays there when the page is drawn anew (every finding of
/// a check draws it again) and the first time is at the service signed in to.
#[derive(Resource, Default)]
pub(super) struct Place {
    kept: Kept,
    focus: Option<Act>,
}

#[derive(Default, PartialEq)]
enum Kept {
    /// The page was not open yet: the chosen service is the first thing seen.
    #[default]
    Unseen,
    /// Brought to the chosen service; its place is kept from the next frame.
    Revealed,
    At(f32),
}

/// A list drawn anew starts where the one it replaces was left, in the frame that shows it.
pub(super) fn restore_place(
    place: Res<Place>,
    mut lists: Query<&mut ScrollPosition, Added<ServiceList>>,
) {
    if let Kept::At(y) = place.kept {
        for mut position in &mut lists {
            position.y = y;
        }
    }
}

/// Brings the service signed in to into view when the page opens, keeps the place of the list,
/// and shows the button the keyboard moves to.
pub(super) fn keep_place(
    mut place: ResMut<Place>,
    mut commands: Commands,
    core: Option<NonSend<MenuCore>>,
    lists: Query<(Entity, &ScrollPosition, &ComputedNode), With<ServiceList>>,
    chosen: Query<Entity, With<SelectedService>>,
    buttons: Query<(Entity, &MenuButton)>,
    parents: Query<&ChildOf>,
) {
    let Ok((list, position, node)) = lists.single() else {
        *place = Place::default();
        return;
    };
    match place.kept {
        Kept::Unseen => {
            // Laid out first: where a row is is known only then.
            if node.size().y > 0.0
                && let Ok(row) = chosen.single()
            {
                commands.trigger(ScrollIntoView { entity: row });
                place.kept = Kept::Revealed;
            }
        }
        Kept::Revealed | Kept::At(_) => place.kept = Kept::At(position.y),
    }
    let focus = core.and_then(|core| core.button_focus().cloned());
    if focus != place.focus {
        // Not before the list has settled: what the keyboard starts on does not outrank the
        // service signed in to.
        if matches!(place.kept, Kept::At(_))
            && let Some(act) = &focus
            && let Some((button, _)) = buttons.iter().find(|(button, held)| {
                held.0 == *act && parents.iter_ancestors(*button).any(|above| above == list)
            })
        {
            commands.trigger(ScrollIntoView { entity: button });
        }
        place.focus = focus;
    }
}
