//! One profile presentation, with the player's retained or publicly served matches.
use super::{
    core::{Act, Page},
    history::Match,
    kit::{Kit, Tone},
};
use crate::{dota_text::Texts, theme};
use bevy::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rating {
    pub mode: String,
    pub rating: Option<String>,
    pub games: u32,
    pub rank: Option<u32>,
}

pub struct Profile<'a> {
    pub name: &'a str,
    pub connected: bool,
    pub read_only: bool,
    pub ratings: &'a [Rating],
    pub matches: &'a [Match],
    pub mode_names: &'a [(String, String)],
    pub texts: &'a Texts,
    pub avatar: &'a super::avatar::View,
    pub notice: Option<&'a str>,
}

pub fn profile(kit: &mut Kit, root: Entity, data: Profile<'_>, clock: super::page_time::Clock) {
    let page = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: percent(12.5),
            top: vh(12),
            bottom: px(28),
            width: percent(75),
            flex_direction: FlexDirection::Column,
            ..default()
        },
    );
    kit.commands
        .entity(page)
        .insert(Name::new("player profile"));
    profile_header(kit, page, &data);
    if let Some(notice) = data.notice {
        let text = kit.typed(page, notice, theme::BODY, theme::INK_3);
        kit.commands
            .entity(text)
            .entry::<Node>()
            .and_modify(|mut node| {
                node.margin = UiRect::new(
                    px(super::player_matches::CONTENT_INSET),
                    px(0),
                    px(12),
                    px(0),
                )
            });
    }
    super::player_matches::draw(kit, page, &data, clock);
}

fn profile_header(kit: &mut Kit, parent: Entity, data: &Profile<'_>) {
    let header = kit.node(
        parent,
        Node {
            height: px(80),
            flex_shrink: 0.0,
            padding: UiRect::axes(px(super::player_matches::CONTENT_INSET), px(12)),
            align_items: AlignItems::Center,
            column_gap: px(14),
            ..default()
        },
    );
    kit.commands.entity(header).insert((
        Name::new("profile player header"),
        BackgroundColor(Color::BLACK),
    ));
    super::avatar_draw::header(kit, header, data.name, data.avatar);
    let name = kit.node(
        header,
        Node {
            flex_grow: 1.0,
            min_width: px(0),
            height: percent(100),
            overflow: Overflow::clip(),
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    let title = kit.bold(name, data.name, 28.0, theme::INK_1);
    kit.commands.entity(title).insert(TextLayout {
        linebreak: LineBreak::NoWrap,
        ..default()
    });
    kit.typed(name, "MATCHES", theme::CAPTION, theme::INK_4);
    if data.connected {
        header_ratings(kit, header, data.ratings);
    }
    header_actions(
        kit,
        header,
        data.connected && !data.read_only,
        data.read_only,
    );
}

fn header_ratings(kit: &mut Kit, parent: Entity, ratings: &[Rating]) {
    for rating in ratings.iter().filter(|rating| rating.games > 0) {
        let column = kit.node(
            parent,
            Node {
                width: px(150),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                overflow: Overflow::clip(),
                ..default()
            },
        );
        kit.typed(column, &rating.mode, theme::CAPTION, theme::INK_3);
        kit.bold(
            column,
            rating.rating.as_deref().unwrap_or("—"),
            22.0,
            theme::INK_1,
        );
        kit.typed(
            column,
            format!("{} rated games", rating.games),
            theme::CAPTION,
            theme::INK_4,
        );
    }
}

fn header_actions(kit: &mut Kit, parent: Entity, connected: bool, read_only: bool) {
    let actions = if read_only {
        vec![("BACK", Act::Back, 80.0)]
    } else if connected {
        vec![
            ("SIGN OUT", Act::Logout, 100.0),
            ("CHANGE SERVICE", Act::Go(Page::Service), 156.0),
        ]
    } else {
        Vec::new()
    };
    for (label, action, width) in actions {
        let button = kit.button_role(
            parent,
            label,
            action,
            Tone::Secondary,
            40.0,
            theme::TABLE_ACTION,
        );
        kit.commands
            .entity(button)
            .entry::<Node>()
            .and_modify(move |mut node| node.width = px(width));
    }
}
