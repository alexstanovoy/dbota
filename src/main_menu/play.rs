//! The Play drawer and its local choices. Queue transport stays in MenuCore.
use bevy::prelude::*;
use bota_radovid::proto::{AccountGameLimit, RulesetId};

use super::core::{Act, Page};
use super::kit::{Kit, MenuButton, Tone};
use super::view::{MenuView, RulesetRow};
use crate::theme;
use bevy::ui_widgets::Button;

/// Local choices, independent of the service's queue request shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayState {
    pub open: bool,
    pub section: Section,
    pub bots: bool,
    pub selected: Option<Vec<RulesetId>>,
}

/// The drawer's stacked sections; exactly one is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    CustomLobbies,
    Ranked,
    Unranked,
}

impl Section {
    /// Whether the queues the section lists are rated; `None` for the lobbies, which queue nowhere.
    pub fn rated(self) -> Option<bool> {
        match self {
            Self::CustomLobbies => None,
            Self::Ranked => Some(true),
            Self::Unranked => Some(false),
        }
    }
}

impl Default for PlayState {
    fn default() -> Self {
        Self {
            open: false,
            section: Section::Ranked,
            bots: false,
            selected: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlayAction {
    Open(bool),
    Section(Section),
    Bots,
    Select(RulesetId),
    Find,
    LobbyLink(bool),
}

impl PlayState {
    /// Whether a queue that is `rated` belongs to the open section.
    pub fn lists(&self, rated: bool) -> bool {
        self.section.rated() == Some(rated)
    }

    pub fn chosen(&self, row: &RulesetRow, first: Option<&RulesetRow>) -> bool {
        self.selected.as_ref().map_or_else(
            || first.is_some_and(|first| first.id == row.id),
            |ids| ids.contains(&row.id),
        )
    }
}

/// The only route from the menu's PLAY corner into matchmaking; local play uses this drawer too.
pub fn corner(kit: &mut Kit, root: Entity, home: bool) {
    let button = kit.button(
        root,
        "PLAY",
        Act::Play(PlayAction::Open(true)),
        Tone::Primary,
        52.0,
    );
    if !home {
        kit.commands.entity(button).insert(Name::new("open play"));
    }
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut n| {
            n.position_type = PositionType::Absolute;
            n.right = px(theme::CORNER_RIGHT);
            n.bottom = px(theme::CORNER_BOTTOM);
            n.width = px(theme::CORNER_WIDTH);
        });
}

pub fn draw(
    kit: &mut Kit,
    root: Entity,
    view: &MenuView,
    rulesets: &[RulesetRow],
    in_flight: (usize, Option<AccountGameLimit>),
    cooldown: Option<&str>,
) {
    if !view.play.open || view.offer.is_some() {
        return;
    }
    let online = view.account.is_some();
    let searching = !view.finding().0.is_empty();
    let drawer = drawer(kit, root);
    // Without an account only the local lobbies exist.
    let section = if online {
        view.play.section
    } else {
        Section::CustomLobbies
    };
    let lobbies = section == Section::CustomLobbies;
    if online {
        for (heading_section, title) in [
            (Section::CustomLobbies, "CUSTOM LOBBIES"),
            (Section::Ranked, "RANKED"),
            (Section::Unranked, "UNRANKED"),
        ] {
            heading(
                kit,
                drawer,
                title,
                heading_section,
                section == heading_section,
            );
            if section == heading_section {
                match heading_section {
                    Section::CustomLobbies => lobby_links(kit, drawer, online, view.can_host),
                    _ => matchmaking(kit, drawer, view, rulesets),
                }
            }
        }
    } else {
        heading(kit, drawer, "LOCAL LOBBIES", section, true);
        lobby_links(kit, drawer, online, view.can_host);
    }

    // Keep the active sections above the search band and the common PLAY position.
    kit.node(
        drawer,
        Node {
            height: px(theme::ACTION_HEIGHT
                + 24.0
                + if searching || super::availability::reason(view).is_some() {
                    theme::FINDING_HEIGHT + theme::CORNER_GAP
                } else {
                    0.0
                }),
            flex_shrink: 0.0,
            ..default()
        },
    );
    footer(
        kit, root, view, rulesets, in_flight, cooldown, searching, online, lobbies,
    );
}

fn heading(kit: &mut Kit, parent: Entity, title: &str, section: Section, open: bool) {
    let button = kit.quiet(
        parent,
        title,
        Act::Play(PlayAction::Section(section)),
        theme::PLAY_SECTION,
        (if open { theme::INK_1 } else { theme::INK_3 }, theme::INK_1),
    );
    kit.commands.entity(button).insert((
        Node {
            width: percent(100),
            height: px(48),
            padding: UiRect::left(px(12)),
            border: UiRect::left(px(6)),
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(if open {
            theme::INK_1
        } else {
            theme::AVATAR_EDGE
        }),
    ));
    if open {
        kit.commands
            .entity(button)
            .insert(theme::downward(theme::PLAY_HEADING));
    } else {
        kit.commands
            .entity(button)
            .insert(BackgroundColor(theme::FIELD));
    }
}

fn matchmaking(kit: &mut Kit, parent: Entity, view: &MenuView, rulesets: &[RulesetRow]) {
    let body = kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(6), px(6)),
            row_gap: px(4),
            ..default()
        },
    );
    kit.commands
        .entity(body)
        .insert(BackgroundColor(theme::PLAY_BODY));
    kit.typed(
        body,
        match view.play.section {
            Section::Unranked => "Matches by rating, more loosely, and never changes it.",
            _ => "Changes your rating for the mode.",
        },
        theme::BODY,
        theme::INK_3,
    );
    let available: Vec<_> = rulesets
        .iter()
        .filter(|r| view.play.lists(r.rated))
        .collect();
    if available.is_empty() {
        kit.typed(
            body,
            "No queues available for this mode.",
            theme::BODY,
            theme::INK_4,
        );
    }
    for row in &available {
        queue_row(kit, body, view, row, available.first().copied());
    }
    bots(kit, body, view.play.bots);
}

fn queue_row(
    kit: &mut Kit,
    parent: Entity,
    view: &MenuView,
    row: &RulesetRow,
    first: Option<&RulesetRow>,
) {
    let chosen = view.play.chosen(row, first);
    let line = queue_frame(kit, parent, row, chosen);
    kit.typed(
        line,
        row.name.to_uppercase(),
        theme::QUEUE_NAME,
        theme::INK_1,
    );
    info(
        kit,
        line,
        &format!("About {}", row.name),
        &row.description,
        false,
    );
    kit.node(
        line,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    kit.typed(
        line,
        row.teams.replace('v', " v "),
        theme::SMALL_NUMBER,
        theme::INK_3,
    );
}

fn bots(kit: &mut Kit, parent: Entity, on: bool) {
    let button = kit.node(
        parent,
        Node {
            min_height: px(44),
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::left(px(8)),
            margin: UiRect::top(px(2)),
            border: UiRect::top(px(1)),
            ..default()
        },
    );
    kit.commands.entity(button).insert((
        Button,
        Pickable::default(),
        MenuButton(Act::Play(PlayAction::Bots)),
        Name::new("allow matchmaking with bots"),
        BorderColor::all(theme::DIVIDER),
    ));
    let track = kit.node(
        button,
        Node {
            width: px(40),
            height: px(22),
            padding: UiRect::all(px(3)),
            align_items: AlignItems::Center,
            justify_content: if on {
                JustifyContent::FlexEnd
            } else {
                JustifyContent::FlexStart
            },
            border_radius: BorderRadius::all(px(12)),
            border: UiRect::all(px(1)),
            flex_shrink: 0.0,
            ..default()
        },
    );
    kit.commands.entity(track).insert((
        BackgroundColor(if on { theme::GO.1 } else { theme::FIELD }),
        BorderColor::all(theme::FIELD_EDGE),
    ));
    let knob = kit.node(
        track,
        Node {
            width: px(16),
            height: px(16),
            border_radius: BorderRadius::all(px(8)),
            ..default()
        },
    );
    kit.commands
        .entity(knob)
        .insert(BackgroundColor(theme::INK_1));
    kit.typed(
        button,
        "Allow matchmaking with bots",
        theme::BODY,
        theme::INK_1,
    );
}

fn lobby_links(kit: &mut Kit, parent: Entity, online: bool, can_host: bool) {
    let body = kit.node(
        parent,
        Node {
            padding: UiRect::all(px(10)),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        },
    );
    kit.commands
        .entity(body)
        .insert(BackgroundColor(theme::PLAY_BODY));
    for label in if online {
        ["BROWSE LOBBIES", "CREATE LOBBY"]
    } else {
        ["HOST A LOCAL LOBBY", "JOIN BY ADDRESS"]
    } {
        if !online && !can_host && label == "HOST A LOCAL LOBBY" {
            continue;
        }
        let create = label == "CREATE LOBBY" || label == "HOST A LOCAL LOBBY";
        let action = if online && !create {
            Act::Go(Page::Lobbies)
        } else {
            Act::Play(PlayAction::LobbyLink(create))
        };
        let button = kit.button(body, label, action, Tone::Secondary, 44.0);
        kit.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(|mut n| n.width = percent(100));
    }
}

pub fn info(kit: &mut Kit, parent: Entity, label: &str, text: &str, right: bool) {
    crate::ui::help::draw(
        &mut kit.visual(),
        parent,
        label,
        text,
        crate::ui::help::Style {
            side: if right {
                crate::ui::help::Side::Right
            } else {
                crate::ui::help::Side::Left
            },
            ..crate::ui::help::Style::COMPACT
        },
    );
}

pub fn tooltip(kit: &mut Kit, parent: Entity, text: &str, right: bool) -> Entity {
    crate::ui::help::tooltip(
        &mut kit.visual(),
        parent,
        text,
        if right {
            crate::ui::help::Side::Right
        } else {
            crate::ui::help::Side::Left
        },
    )
}

/// The service's info bytes are game metadata, not display text. Descriptions are keyed by id.
pub fn description(id: &RulesetId) -> String {
    match id.0.as_str() {
        "solo_mid" | "solo_mid_bots" => format!(
            "The middle lane. {} kills or the first tower wins; {} minutes at most.",
            bota_core::MAP2_DEATH_LIMIT,
            bota_core::MAP2_GAME_TICKS / bota_core::rules::TICKS_PER_SECOND / 60
        ),
        "all_pick" => "The whole map. Destroy the enemy Ancient.".into(),
        _ => "Play this mode using the rules configured by the service.".into(),
    }
}

/// Account capacity is charged per search, so a multi-selection must fit in full.
pub fn can_search(
    in_flight: (usize, Option<AccountGameLimit>),
    selected: usize,
    cooldown: bool,
) -> bool {
    selected > 0
        && !cooldown
        && match in_flight.1 {
            Some(AccountGameLimit::Unlimited) => true,
            Some(AccountGameLimit::Limited(maximum)) => {
                selected <= usize::from(maximum).saturating_sub(in_flight.0)
            }
            None => false,
        }
}

fn drawer(kit: &mut Kit, root: Entity) -> Entity {
    let shade = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(theme::PLAY_DRAWER_WIDTH),
            top: px(theme::BAR_HEIGHT),
            bottom: px(0),
            ..default()
        },
    );
    kit.commands.entity(shade).insert((
        Name::new("play backdrop"),
        BackgroundColor(Color::BLACK.with_alpha(theme::dim(0.35))),
        crate::hud::Blocker,
        bevy::picking::hover::Hovered::default(),
        Pickable::default(),
    ));
    let drawer = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(0),
            top: px(theme::BAR_HEIGHT),
            bottom: px(0),
            width: px(theme::PLAY_DRAWER_WIDTH),
            padding: UiRect::new(
                px(32),
                px(theme::CORNER_RIGHT),
                px(12),
                px(theme::CORNER_BOTTOM),
            ),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            border: UiRect::left(px(1)),
            ..default()
        },
    );
    kit.commands.entity(drawer).insert((
        Name::new("play drawer"),
        crate::hud::Blocker,
        bevy::picking::hover::Hovered::default(),
        Pickable::default(),
        theme::downward(theme::PLAY_PANEL),
        BorderColor::all(theme::PANEL_GLINT),
    ));
    kit.quiet(
        drawer,
        "‹ BACK",
        Act::Play(PlayAction::Open(false)),
        theme::button(44.0),
        (theme::INK_3, theme::INK_1),
    );
    kit.node(
        drawer,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    drawer
}

fn footer(
    kit: &mut Kit,
    root: Entity,
    view: &MenuView,
    rulesets: &[RulesetRow],
    in_flight: (usize, Option<AccountGameLimit>),
    cooldown: Option<&str>,
    searching: bool,
    online: bool,
    lobbies: bool,
) {
    let corner = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(theme::CORNER_RIGHT),
            bottom: px(theme::CORNER_BOTTOM),
            width: px(theme::CORNER_WIDTH),
            ..default()
        },
    );
    let selected = rulesets.iter().filter(|r| view.play.lists(r.rated));
    let first = selected.clone().next();
    let count = selected
        .clone()
        .filter(|r| view.play.chosen(r, first))
        .count();
    let blocked = super::availability::reason(view);
    let can = can_search(in_flight, count, cooldown.is_some()) && blocked.is_none();
    let (label, action) = if searching {
        ("PLAY", Act::Play(PlayAction::Open(false)))
    } else if lobbies && !online && !view.can_host {
        ("JOIN BY ADDRESS", Act::Play(PlayAction::LobbyLink(false)))
    } else if lobbies {
        (
            "OPEN LOBBIES",
            Act::Go(if online {
                Page::Lobbies
            } else {
                Page::LocalLobby
            }),
        )
    } else {
        ("FIND MATCH", Act::Play(PlayAction::Find))
    };
    let button = if can || lobbies || searching {
        kit.button(corner, label, action, Tone::Primary, 52.0)
    } else {
        kit.disabled_button(corner, label, 52.0)
    };
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut n| n.width = percent(100));
    if !lobbies
        && !searching
        && let Some(reason) = blocked
    {
        super::availability::strip(kit, root, view, reason);
        super::availability::explain(kit, button, reason);
    }
}

fn queue_frame(kit: &mut Kit, parent: Entity, row: &RulesetRow, chosen: bool) -> Entity {
    let line = kit.node(
        parent,
        Node {
            min_height: px(52),
            width: percent(100),
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(12), px(6)),
            border: UiRect::left(px(4)),
            ..default()
        },
    );
    kit.commands.entity(line).insert((
        Button,
        Pickable::default(),
        MenuButton(Act::Play(PlayAction::Select(row.id.clone()))),
        Name::new(format!("queue {}", row.id.0)),
        BorderColor::all(if chosen { theme::SELECT } else { Color::NONE }),
    ));
    kit.row_effect(
        line,
        if chosen {
            theme::PLAY_SELECTION
        } else {
            (theme::PLAY_ROW, theme::PLAY_ROW)
        },
        BorderColor::all(if chosen { theme::SELECT } else { Color::NONE }),
    );
    if chosen {
        let outline = kit.node(
            line,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                border: UiRect::all(px(1)),
                ..default()
            },
        );
        kit.commands.entity(outline).insert((
            Name::new("queue inset outline"),
            BorderColor::all(theme::SELECT),
        ));
    } else {
        kit.commands
            .entity(line)
            .insert(BackgroundColor(theme::PLAY_ROW));
    }
    line
}
