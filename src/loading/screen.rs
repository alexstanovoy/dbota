//! Loading: the install's default art and a quiet sidebar of the game's connection facts.
use super::{Details, Loading, Row};
use crate::main_menu::kit::Kit;
use crate::{
    manifest::Manifest,
    session::Session,
    theme,
    ui::{self, Fonts},
};
use bevy::prelude::*;
use bota_proto::Team;

const SIDEBAR: f32 = 336.0;
#[derive(Component)]
struct Root;
#[derive(Component)]
struct Throbber;
#[derive(Component)]
struct BarFill;
#[derive(Component)]
struct Elapsed;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, |mut commands: Commands| {
        commands.spawn((
            Root,
            ui::fill(),
            BackgroundColor(Color::BLACK),
            GlobalZIndex(50),
            Visibility::Hidden,
        ));
    })
    .add_systems(
        PostUpdate,
        (show, progress, turn).chain().after(super::track),
    );
}

#[allow(clippy::too_many_arguments)]
fn show(
    commands: Commands,
    loading: Res<Loading>,
    session: NonSend<Session>,
    core: Option<NonSend<crate::main_menu::MenuCore>>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    texts: Res<crate::dota_text::Texts>,
    server: Res<AssetServer>,
    mut root: Single<(Entity, &mut Visibility), With<Root>>,
    mut drawn: Local<Option<Details>>,
) {
    root.1.set_if_neq(ui::visible(loading.shown));
    if !loading.shown {
        return;
    }
    let details = core
        .map_or_else(Details::default, |c| c.loading_details())
        .with_connections(&session.0, &texts);
    if drawn.as_ref() == Some(&details) {
        return;
    }
    let mut kit = Kit {
        commands,
        fonts: &fonts,
        manifest: &manifest,
        texts: &texts,
        server: &server,
    };
    kit.commands.entity(root.0).despawn_children();
    draw(&mut kit, root.0, &details, loading.progress, 0);
    *drawn = Some(details);
}

/// Also used by the menu while the connection is opening, and by its loading preview.
pub fn draw(kit: &mut Kit, root: Entity, details: &Details, progress: f32, seconds: u32) {
    art(kit, root);
    let sidebar = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            right: px(0),
            top: px(0),
            bottom: px(0),
            width: px(SIDEBAR),
            padding: UiRect::axes(px(16), px(20)),
            flex_direction: FlexDirection::Column,
            row_gap: px(14),
            border: UiRect::left(px(1)),
            ..default()
        },
    );
    kit.commands.entity(sidebar).insert((
        Name::new("loading sidebar"),
        BackgroundColor(theme::LOADING_PANEL),
        BorderColor::all(theme::DIVIDER),
    ));
    identity(kit, sidebar, details, seconds);
    for team in [Team::Radiant, Team::Dire] {
        team_list(kit, sidebar, details, team);
    }
    kit.node(
        sidebar,
        Node {
            flex_grow: 1.0,
            ..default()
        },
    );
    loading_panel(kit, sidebar, progress);
}

fn team_list(kit: &mut Kit, parent: Entity, details: &Details, team: Team) {
    let side = kit.node(
        parent,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    let ink = if team == Team::Radiant {
        theme::RADIANT
    } else {
        theme::DIRE
    };
    let head = kit.node(
        side,
        Node {
            padding: UiRect::axes(px(8), px(4)),
            margin: UiRect::bottom(px(4)),
            ..default()
        },
    );
    kit.commands.entity(head).insert(theme::rightward((
        theme::LOADING_PANEL
            .to_srgba()
            .mix(&ink.to_srgba(), 0.28)
            .into(),
        theme::LOADING_PANEL,
    )));
    kit.typed(
        head,
        if team == Team::Radiant {
            "RADIANT"
        } else {
            "DIRE"
        },
        theme::LOADING_TEAM,
        ink,
    );
    for row in details.seats.iter().filter(|r| r.team == team).take(10) {
        player(kit, side, row);
    }
}

fn player(kit: &mut Kit, parent: Entity, row: &Row) {
    let line = kit.node(
        parent,
        Node {
            height: px(46),
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::axes(px(8), px(0)),
            border: UiRect::left(px(if row.me { 3 } else { 0 })),
            ..default()
        },
    );
    kit.commands.entity(line).insert((
        Name::new(format!("loading player {}", row.slot)),
        BackgroundColor(if row.me {
            theme::LOADING_SELF
        } else {
            theme::LOADING_ROW
        }),
        BorderColor::all(theme::SELECT),
    ));
    avatar(kit, line, row);
    let words = kit.node(
        line,
        Node {
            min_width: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            ..default()
        },
    );
    player_words(kit, words, row);
}

fn loading_panel(kit: &mut Kit, parent: Entity, progress: f32) {
    let panel = kit.node(
        parent,
        Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::FlexEnd,
            column_gap: px(10),
            ..default()
        },
    );
    let track = kit.node(
        panel,
        Node {
            width: px(220),
            height: px(4),
            ..default()
        },
    );
    kit.commands
        .entity(track)
        .insert(BackgroundColor(theme::LOADING_TRACK));
    let fill = kit.node(
        track,
        Node {
            width: percent(progress.clamp(0.0, 1.0) * 100.0),
            height: percent(100),
            ..default()
        },
    );
    kit.commands.entity(fill).insert((
        BarFill,
        Name::new("loading progress"),
        BackgroundColor(theme::INK_1),
    ));
    let spinner = kit.art(
        panel,
        "loading.throbber",
        Node {
            width: px(28),
            height: px(28),
            ..default()
        },
    );
    kit.commands.entity(spinner).insert(Throbber);
}

fn progress(
    loading: Res<Loading>,
    mut fills: Query<&mut Node, With<BarFill>>,
    mut elapsed: Query<&mut Text, With<Elapsed>>,
) {
    if !loading.shown {
        return;
    }
    for mut fill in &mut fills {
        fill.width = percent(loading.progress.clamp(0.0, 1.0) * 100.0);
    }
    let seconds = (crate::since_start() - loading.began).max(0.0) as u32;
    for mut text in &mut elapsed {
        text.set_if_neq(Text(format!("{}:{:02}", seconds / 60, seconds % 60)));
    }
}

fn turn(time: Res<Time>, mut throbbers: Query<&mut UiTransform, With<Throbber>>) {
    for mut transform in &mut throbbers {
        transform.rotation = Rot2::radians(-time.elapsed_secs() * std::f32::consts::TAU);
    }
}

fn art(kit: &mut Kit, root: Entity) {
    let art = kit.node(
        root,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(SIDEBAR),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    kit.commands
        .entity(art)
        .insert(BackgroundColor(Color::BLACK));
    // The install's default loading screen is black with this logo, not a replacement art pack.
    kit.art(
        art,
        "loading.logo",
        Node {
            width: px(253),
            height: px(253),
            ..default()
        },
    );
}

fn identity(kit: &mut Kit, sidebar: Entity, details: &Details, seconds: u32) {
    let head = kit.node(
        sidebar,
        Node {
            justify_content: JustifyContent::SpaceBetween,
            ..default()
        },
    );
    let label = details
        .match_id
        .map_or_else(|| "Local game".to_owned(), |id| format!("Match {id}"));
    kit.typed(head, label, theme::BODY, theme::INK_3);
    kit.typed(head, &details.mode, theme::BODY, theme::INK_3);
    let players = kit.node(
        sidebar,
        Node {
            align_items: AlignItems::Baseline,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::bottom(px(8)),
            border: UiRect::bottom(px(1)),
            ..default()
        },
    );
    kit.commands
        .entity(players)
        .insert(BorderColor::all(theme::DIVIDER));
    let connected = details.seats.iter().filter(|r| r.connected).count();
    kit.typed(
        players,
        format!("PLAYERS · {connected} OF {} CONNECTED", details.seats.len()),
        theme::LOADING_SUMMARY,
        theme::INK_1,
    );
    let elapsed = kit.typed(
        players,
        format!("{}:{:02}", seconds / 60, seconds % 60),
        theme::NUMBER,
        theme::INK_5,
    );
    kit.commands.entity(elapsed).insert(Elapsed);
}

fn avatar(kit: &mut Kit, line: Entity, row: &Row) {
    let avatar = kit.node(
        line,
        Node {
            width: px(38),
            height: px(38),
            flex_shrink: 0.0,
            border: UiRect::all(px(1)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
    );
    kit.commands.entity(avatar).insert((
        BackgroundColor(theme::FIELD),
        BorderColor::all(theme::FIELD_EDGE),
    ));
    let initial: String = row
        .name
        .chars()
        .take(1)
        .flat_map(char::to_uppercase)
        .collect();
    kit.typed(avatar, initial, theme::LABEL, theme::INK_3);
}

fn player_words(kit: &mut Kit, words: Entity, row: &Row) {
    let name_row = kit.node(
        words,
        Node {
            align_items: AlignItems::Center,
            column_gap: px(5),
            ..default()
        },
    );
    let name = kit.typed(
        name_row,
        &row.name,
        theme::LABEL,
        if row.connected {
            theme::INK_1
        } else {
            theme::INK_3
        },
    );
    kit.commands.entity(name).insert((
        Node {
            max_width: px(180),
            overflow: Overflow::clip(),
            ..default()
        },
        TextLayout {
            linebreak: LineBreak::NoWrap,
            ..default()
        },
    ));
    if row.bot {
        kit.typed(name_row, "BOT", theme::CAPTION, theme::INK_4);
    }
    kit.typed(
        words,
        if row.connected {
            "Connected"
        } else {
            "Connecting…"
        },
        theme::STATUS_TEXT,
        if row.connected {
            theme::RADIANT_TEXT
        } else {
            theme::INK_3
        },
    );
}
