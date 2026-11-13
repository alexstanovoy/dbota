//! A profile's match table, using the shared result flow and native scrolling widgets.
use super::{
    account_page::Profile,
    core::Act,
    history::{Match, ResultKind},
    kit::{Kit, Tone},
};
use crate::theme;
use bevy::{
    prelude::*,
    ui_widgets::{ControlOrientation, ScrollArea, ScrollIntoView, Scrollbar, ScrollbarThumb},
};

pub(super) const CONTENT_INSET: f32 = 64.0;
const SCROLL_GUTTER: f32 = 12.0;
const ROW_HEIGHT: f32 = 36.0;
const COLUMNS: [(&str, f32); 5] = [
    ("HERO PLAYED", 23.0),
    ("RESULT", 20.0),
    ("DATE / TIME", 23.0),
    ("DURATION", 15.5),
    ("TYPE", 18.5),
];

#[derive(Component)]
pub(super) struct MatchRow;

#[derive(Component)]
pub(super) struct MatchScrollbar;

pub fn draw(kit: &mut Kit, parent: Entity, data: &Profile<'_>, clock: super::page_time::Clock) {
    let table = kit.node(
        parent,
        Node {
            width: percent(100),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(0),
            margin: UiRect::top(px(12)),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    heading(kit, table);
    let (body, viewport) = scroll_viewport(kit, table);
    if data.matches.is_empty() {
        empty(kit, viewport, data.read_only);
    }
    for (index, game) in data.matches.iter().take(50).enumerate() {
        row(kit, viewport, data, game, index, clock);
    }
    scrollbar(kit, body, viewport);
}

fn heading(kit: &mut Kit, parent: Entity) {
    let heading = table_row(kit, parent, 32.0);
    // Headings reserve the same scrollbar space as the data viewport.
    kit.commands
        .entity(heading)
        .insert(BackgroundColor(theme::ACCOUNT_BAND))
        .entry::<Node>()
        .and_modify(|mut node| node.padding.right = px(CONTENT_INSET + SCROLL_GUTTER));
    for (index, (label, _)) in COLUMNS.iter().enumerate() {
        text_column(kit, heading, index, *label, theme::INK_4);
    }
}

fn scroll_viewport(kit: &mut Kit, parent: Entity) -> (Entity, Entity) {
    let body = kit.node(
        parent,
        Node {
            width: percent(100),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(0),
            ..default()
        },
    );
    let viewport = kit.node(
        body,
        Node {
            width: percent(100),
            height: percent(100),
            overflow: Overflow::scroll_y(),
            flex_direction: FlexDirection::Column,
            padding: UiRect::right(px(SCROLL_GUTTER)),
            ..default()
        },
    );
    kit.commands
        .entity(viewport)
        .insert((ScrollArea, Name::new("player matches scroll")));
    (body, viewport)
}

fn scrollbar(kit: &mut Kit, parent: Entity, viewport: Entity) {
    let rail = kit.node(
        parent,
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
        Scrollbar::new(viewport, ControlOrientation::Vertical, 16.0),
    ));
    kit.commands.spawn((
        ScrollbarThumb::default(),
        Name::new("player matches scroll thumb"),
        BackgroundColor(theme::FIELD_EDGE),
        ChildOf(rail),
    ));
}

fn empty(kit: &mut Kit, parent: Entity, public: bool) {
    let text = kit.typed(
        parent,
        if public {
            "No public matches yet"
        } else {
            "No matches on this machine yet."
        },
        theme::BODY,
        theme::INK_3,
    );
    kit.commands
        .entity(text)
        .entry::<Node>()
        .and_modify(|mut node| node.margin = UiRect::all(px(CONTENT_INSET)));
}

fn table_row(kit: &mut Kit, parent: Entity, height: f32) -> Entity {
    kit.node(
        parent,
        Node {
            width: percent(100),
            height: px(height),
            min_height: px(height),
            flex_shrink: 0.0,
            padding: UiRect::axes(px(CONTENT_INSET), px(0)),
            align_items: AlignItems::Center,
            ..default()
        },
    )
}

fn column(kit: &mut Kit, parent: Entity, index: usize) -> Entity {
    kit.node(
        parent,
        Node {
            width: percent(COLUMNS[index].1),
            height: percent(100),
            min_width: px(0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            column_gap: px(10),
            ..default()
        },
    )
}

fn text_column(
    kit: &mut Kit,
    parent: Entity,
    index: usize,
    text: impl Into<String>,
    color: Color,
) -> Entity {
    let column = column(kit, parent, index);
    kit.typed(column, text, theme::BODY, color);
    column
}

fn row(
    kit: &mut Kit,
    parent: Entity,
    data: &Profile<'_>,
    game: &Match,
    index: usize,
    clock: super::page_time::Clock,
) {
    let row = table_row(kit, parent, ROW_HEIGHT);
    let rest = if index.is_multiple_of(2) {
        theme::HISTORY_ROW
    } else {
        theme::FIELD_SUNKEN
    };
    let name = format!("profile match {}", game.file);
    kit.commands
        .entity(row)
        .insert((Name::new(name.clone()), BackgroundColor(rest)));
    if game.details_available {
        kit.row_action(row, &name, Act::MatchDetails(game.file.clone()), rest);
        kit.commands.entity(row).insert(MatchRow);
    }
    hero(kit, row, data, game);
    let (result, color) = match game.result {
        ResultKind::Won => ("Win", theme::RADIANT_TEXT),
        ResultKind::Lost => ("Loss", theme::LOSS),
        ResultKind::Draw => ("Draw", theme::INK_3),
        ResultKind::Undecided => ("—", theme::INK_4),
    };
    text_column(kit, row, 1, result, color);
    text_column(
        kit,
        row,
        2,
        clock.date(game.when).replace('\n', "  "),
        theme::INK_3,
    );
    text_column(kit, row, 3, game.length(), theme::INK_3);
    let mode_name = data
        .mode_names
        .iter()
        .find(|(id, _)| id == &game.mode)
        .map_or_else(
            || super::history::mode_name(&game.mode),
            |(_, name)| name.as_str(),
        );
    let mode = text_column(kit, row, 4, mode_name, theme::INK_3);
    if !data.read_only {
        watch(kit, mode, game);
    }
}

fn hero(kit: &mut Kit, parent: Entity, data: &Profile<'_>, game: &Match) {
    let column = column(kit, parent, 0);
    if let Some(id) = game.hero {
        kit.hero_icon(column, id.0, 44.0);
    }
    let name = game
        .hero
        .and_then(bota_core::hero_def)
        .map_or("—", |hero| data.texts.name(hero.key));
    kit.typed(column, name, theme::BODY, theme::INK_1);
}

fn watch(kit: &mut Kit, parent: Entity, game: &Match) {
    let button = if game.watchable {
        kit.button_role(
            parent,
            "WATCH",
            Act::WatchHistory(game.file.clone()),
            Tone::Secondary,
            28.0,
            theme::TABLE_ACTION,
        )
    } else {
        kit.disabled_button_role(parent, "WATCH", 28.0, theme::TABLE_ACTION)
    };
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(|mut node| {
            node.width = px(76);
            node.min_width = px(76);
            node.margin.left = Val::Auto;
        });
}

pub(super) fn reveal_focus(
    focus: Res<bevy::input_focus::InputFocus>,
    rows: Query<(), With<MatchRow>>,
    mut commands: Commands,
) {
    if focus.is_changed()
        && let Some(entity) = focus.get()
        && rows.get(entity).is_ok()
    {
        commands.trigger(ScrollIntoView { entity });
    }
}

pub(super) fn scrollbar_visibility(
    mut rails: Query<(&Scrollbar, &mut Visibility), With<MatchScrollbar>>,
    layouts: Query<&ComputedNode>,
) {
    for (bar, mut visible) in &mut rails {
        if let Ok(layout) = layouts.get(bar.target) {
            visible.set_if_neq(crate::ui::visible(
                layout.content_size().y > layout.size().y + 1.0,
            ));
        }
    }
}
