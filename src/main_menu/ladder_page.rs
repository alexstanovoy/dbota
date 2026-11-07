//! The flat leaderboard window: mode and audience selection, search, and a separate own rank.
use super::{
    core::Act,
    kit::Kit,
    leaderboard::{Board, Players, Row, Status},
    view::FieldView,
};
use crate::theme;
use bevy::prelude::*;

/// Loaded row positions and their clip viewport; the pinned own row is outside this list.
#[derive(Component)]
pub(super) struct ListRow {
    list: Entity,
    index: usize,
}
#[derive(Component)]
pub(super) struct RangeFooter {
    list: Entity,
    loaded: usize,
}

/// Read the drawn geometry after layout, including scrolling and window/UI scaling.
pub(super) fn visible_range(
    lists: Query<(&ComputedNode, &UiGlobalTransform)>,
    rows: Query<(&ListRow, &ComputedNode, &UiGlobalTransform)>,
    mut footers: Query<(&RangeFooter, &mut Text)>,
) {
    for (footer, mut text) in &mut footers {
        let Ok((viewport, placed)) = lists.get(footer.list) else {
            continue;
        };
        let top = placed.translation.y - viewport.size().y / 2.0;
        let bottom = placed.translation.y + viewport.size().y / 2.0;
        let mut first = usize::MAX;
        let mut last = 0;
        for (row, node, position) in &rows {
            if row.list != footer.list {
                continue;
            }
            // Count legible rows, excluding a clipped sliver at either edge.
            if node.size().y > 0.0
                && position.translation.y >= top
                && position.translation.y < bottom
            {
                first = first.min(row.index + 1);
                last = last.max(row.index + 1);
            }
        }
        let first = if last == 0 { 0 } else { first };
        let value = format!("{first}–{last} of {}", footer.loaded);
        if text.0 != value {
            text.0 = value;
        }
    }
}

pub fn draw(kit: &mut Kit, root: Entity, board: &Board, field: &FieldView) {
    let page = window(kit, root);
    filters(kit, page, board, field);
    table_heading(kit, page, board);
    let list = player_list(kit, page);
    list_status(kit, list, board);
    player_rows(kit, page, list, board);
}

fn window(kit: &mut Kit, root: Entity) -> Entity {
    let page = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            margin: UiRect::left(px(-450)),
            top: px(76),
            bottom: px(18),
            width: px(900),
            padding: UiRect::all(px(22)),
            flex_direction: FlexDirection::Column,
            row_gap: px(10),
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(page).insert((
        theme::downward(theme::PANEL),
        BorderColor::all(theme::PANEL_EDGE),
    ));
    let title = kit.node(
        page,
        Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.typed(title, "LEADERBOARD", theme::PAGE_TITLE, theme::INK_1);
    kit.text(
        title,
        format!("Top {}", super::leaderboard::TOP),
        13.0,
        theme::INK_3,
    );
    page
}

fn filters(kit: &mut Kit, page: Entity, board: &Board, field: &FieldView) {
    let controls = kit.node(
        page,
        Node {
            column_gap: px(15),
            align_items: AlignItems::FlexEnd,
            ..default()
        },
    );
    let modes = section(kit, controls, "Mode", 214.0);
    for mode in &board.modes {
        segment(
            kit,
            modes,
            &mode.name.to_uppercase(),
            Act::LeaderboardMode(mode.id.clone()),
            board.query.mode == mode.id,
            103.0,
        );
    }
    let kinds = section(kit, controls, "Players", 178.0);
    for (kind, name) in [(Players::Humans, "HUMANS"), (Players::Bots, "BOTS")] {
        segment(
            kit,
            kinds,
            name,
            Act::LeaderboardPlayers(kind),
            board.query.players == kind,
            if kind == Players::Humans { 96.0 } else { 74.0 },
        );
    }
    let search = kit.node(
        controls,
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    super::pages::search_field(kit, search, 0, field);
}

fn table_heading(kit: &mut Kit, page: Entity, board: &Board) {
    let heading = kit.node(
        page,
        Node {
            width: percent(100),
            height: px(30),
            column_gap: px(10),
            align_items: AlignItems::Center,
            padding: UiRect::axes(px(12), px(0)),
            ..default()
        },
    );
    kit.commands
        .entity(heading)
        .insert(BackgroundColor(theme::FIELD_SUNKEN));
    cells(
        kit,
        heading,
        board.query.players,
        ["RANK", "", "NICKNAME", "VERSION", "GAMES", "RATING"],
        false,
        theme::INK_4,
    );
}

fn player_list(kit: &mut Kit, page: Entity) -> Entity {
    let list = kit.node(
        page,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(3),
            flex_grow: 1.0,
            flex_basis: px(0),
            min_height: px(0),
            overflow: Overflow::scroll_y(),
            ..default()
        },
    );
    kit.commands.entity(list).insert((
        ScrollPosition::default(),
        super::pages::MenuBody,
        Name::new("leaderboard rows"),
    ));
    list
}

fn list_status(kit: &mut Kit, list: Entity, board: &Board) {
    match board.status {
        Status::Loading => {
            kit.text(list, "Loading leaderboard…", 15.0, theme::INK_3);
        }
        Status::Unavailable => {
            kit.text(
                list,
                "Leaderboard unavailable. Try again later.",
                15.0,
                theme::INK_3,
            );
        }
        Status::Ready if board.rows.is_empty() => {
            kit.text(list, "No matching players.", 15.0, theme::INK_3);
        }
        Status::Ready => {}
    }
    if let Some(notice) = &board.notice
        && board.status != Status::Unavailable
    {
        kit.text(list, notice, 13.0, theme::INK_3);
    }
}

fn player_rows(kit: &mut Kit, page: Entity, list: Entity, board: &Board) {
    for (index, row) in board.rows.iter().enumerate() {
        let entity = player(
            kit,
            list,
            row,
            board.query.players,
            board
                .own
                .as_ref()
                .is_some_and(|own| own.account == row.account),
        );
        kit.commands.entity(entity).insert(ListRow { list, index });
    }
    if let Some(own) = &board.own
        && !board.rows.iter().any(|row| row.account == own.account)
    {
        player(kit, page, own, board.query.players, true);
    }
    if board.status == Status::Ready {
        let footer = kit.text(page, "", 12.0, theme::INK_4);
        kit.commands.entity(footer).insert(RangeFooter {
            list,
            loaded: board.rows.len(),
        });
    }
}

fn section(kit: &mut Kit, parent: Entity, name: &str, width: f32) -> Entity {
    let column = kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    kit.text(column, name, 13.0, theme::INK_4);
    let row = kit.node(
        column,
        Node {
            width: px(width),
            height: px(44),
            border: UiRect::all(px(1)),
            padding: UiRect::all(px(3)),
            ..default()
        },
    );
    kit.commands.entity(row).insert((
        BackgroundColor(theme::FIELD_SUNKEN),
        BorderColor::all(theme::SECONDARY_EDGE),
    ));
    row
}

fn segment(kit: &mut Kit, parent: Entity, name: &str, act: Act, selected: bool, width: f32) {
    let button = kit.quiet(
        parent,
        name,
        act,
        theme::LABEL,
        (
            if selected { theme::INK_1 } else { theme::INK_4 },
            theme::INK_1,
        ),
    );
    kit.commands
        .entity(button)
        .entry::<Node>()
        .and_modify(move |mut node| {
            node.width = px(width);
            node.height = px(36);
            node.min_height = px(36);
            node.justify_content = JustifyContent::Center;
            node.border.bottom = px(2);
        });
    if selected {
        kit.commands.entity(button).insert((
            BackgroundColor(theme::SEGMENT_SELECTED),
            BorderColor {
                bottom: theme::SELECT,
                ..BorderColor::all(Color::NONE)
            },
        ));
    }
}

fn player(kit: &mut Kit, parent: Entity, row: &Row, kind: Players, own: bool) -> Entity {
    let (line, medal) = player_frame(kit, parent, row, own);
    let rank = cell(kit, line, 56.0, false, JustifyContent::FlexEnd);
    kit.typed(
        rank,
        row.rank.to_string(),
        if row.rank <= 3 {
            theme::PODIUM_RANK
        } else {
            theme::NUMBER
        },
        if row.rank <= 3 { medal } else { theme::INK_3 },
    );
    player_avatar(kit, line, row, kind);
    let nickname = cell(kit, line, 0.0, true, JustifyContent::FlexStart);
    kit.text(
        nickname,
        &row.nickname,
        14.0,
        if own { theme::SELECT } else { theme::INK_1 },
    );
    let version = cell(kit, line, 90.0, false, JustifyContent::FlexStart);
    if kind == Players::Bots {
        kit.text(
            version,
            row.version.as_deref().unwrap_or("—"),
            13.0,
            theme::INK_3,
        );
    }
    let games = cell(kit, line, 80.0, false, JustifyContent::FlexEnd);
    kit.text(games, row.games.to_string(), 13.0, theme::INK_3);
    let rating = cell(kit, line, 90.0, false, JustifyContent::FlexEnd);
    kit.bold(rating, &row.rating, 17.0, theme::INK_1);
    line
}

fn cell(kit: &mut Kit, parent: Entity, width: f32, grow: bool, justify: JustifyContent) -> Entity {
    kit.node(
        parent,
        Node {
            width: if grow { auto() } else { px(width) },
            flex_grow: if grow { 1.0 } else { 0.0 },
            min_width: px(0),
            flex_shrink: 0.0,
            overflow: Overflow::clip(),
            column_gap: px(0),
            justify_content: justify,
            ..default()
        },
    )
}
fn player_frame(kit: &mut Kit, parent: Entity, row: &Row, own: bool) -> (Entity, Color) {
    let medal = match row.rank {
        1 => theme::GOLD,
        2 => theme::INK_3,
        3 => theme::MEDAL_BRONZE,
        _ => Color::NONE,
    };
    let line = kit.node(
        parent,
        Node {
            width: percent(100),
            height: px(40),
            min_height: px(40),
            padding: UiRect::axes(px(12), px(0)),
            column_gap: px(10),
            align_items: AlignItems::Center,
            ..default()
        },
    );
    kit.commands.entity(line).insert(BackgroundColor(if own {
        theme::LADDER_OWN
    } else if row.rank <= 3 {
        theme::LADDER_TOP
    } else {
        theme::LADDER_ROW
    }));
    if own || row.rank <= 3 {
        let edge = kit.node(
            line,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                top: px(0),
                bottom: px(0),
                width: px(3),
                ..default()
            },
        );
        kit.commands
            .entity(edge)
            .insert(BackgroundColor(if own { theme::SELECT } else { medal }));
    }
    (line, medal)
}

fn player_avatar(kit: &mut Kit, line: Entity, row: &Row, kind: Players) {
    let avatar_cell = cell(kit, line, 36.0, false, JustifyContent::FlexStart);
    let avatar = kit.node(
        avatar_cell,
        Node {
            width: px(32),
            height: px(32),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(px(1)),
            ..default()
        },
    );
    kit.commands.entity(avatar).insert((
        BackgroundColor(if kind == Players::Bots {
            theme::BOT_AVATAR
        } else {
            theme::AVATAR.1
        }),
        BorderColor::all(if kind == Players::Bots {
            theme::BOT_AVATAR_EDGE
        } else {
            theme::SECONDARY_EDGE
        }),
    ));
    kit.text(
        avatar,
        row.nickname
            .chars()
            .next()
            .map(|c| c.to_uppercase().to_string())
            .unwrap_or_default(),
        13.0,
        theme::INK_3,
    );
}

fn cells(kit: &mut Kit, parent: Entity, kind: Players, values: [&str; 6], _bold: bool, ink: Color) {
    for (index, value) in values.into_iter().enumerate() {
        let column = cell(
            kit,
            parent,
            [56.0, 36.0, 0.0, 90.0, 80.0, 90.0][index],
            index == 2,
            if index == 0 || index >= 4 {
                JustifyContent::FlexEnd
            } else {
                JustifyContent::FlexStart
            },
        );
        if index != 3 || kind == Players::Bots {
            kit.typed(
                column,
                if index == 3 { "BOT VERSION" } else { value },
                theme::CAPTION,
                ink,
            );
        }
    }
}
