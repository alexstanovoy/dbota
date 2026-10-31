//! The loading screen, laid out as Dota's (`loading_screen.vcss` and
//! `loading_screen_default.vcss` of the install): black, Dota's logo in the
//! middle, a sidebar on the right with each side's players (Radiant's green
//! and Dire's red gradients, the hero's portrait as the avatar once picked),
//! and at the bottom right the turning throbber with how far the loading is.

use bevy::prelude::*;
use bota_client_core::App as Core;
use bota_proto::{SlotId, Team};

use super::Loading;
use crate::manifest::Manifest;
use crate::session::Session;
use crate::ui::{self, Fonts};

/// Over the game and its pick and strategy screens, under the menu.
const Z: i32 = 50;
/// Dota's sidebar width and the side of its logo, in interface pixels.
const SIDEBAR: f32 = 400.0;
const LOGO: f32 = 253.0;
const AVATAR: f32 = 36.0;
const THROBBER: f32 = 29.0;
/// The bar's width at the bottom right.
const BAR: f32 = 300.0;

#[derive(Component)]
struct Root;

/// The sidebar's list of a side, rebuilt when its players change.
#[derive(Component)]
struct TeamList(Team);

#[derive(Component)]
struct Throbber;

#[derive(Component)]
struct BarFill;

#[derive(Component)]
struct Percent;

/// One player of the loading screen: side, name, hero once picked.
#[derive(Clone, PartialEq, Debug)]
pub struct Row {
    pub team: Team,
    pub name: String,
    pub hero: Option<u16>,
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn).add_systems(
        PostUpdate,
        (show, rosters, progress, turn).after(super::track),
    );
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
) {
    let root = commands
        .spawn((
            Root,
            ui::fill(),
            BackgroundColor(Color::BLACK),
            GlobalZIndex(Z),
            Visibility::Hidden,
        ))
        .id();
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: percent(50),
            width: px(LOGO),
            height: px(LOGO),
            margin: UiRect::new(px(-LOGO / 2.0), px(0), px(-LOGO / 2.0), px(0)),
            ..default()
        },
        ui::art(&manifest, &server, "loading.logo"),
        ChildOf(root),
    ));
    sidebar(&mut commands, root, &fonts);
    loading_panel(&mut commands, root, &fonts, &manifest, &server);
}

/// The players of both sides, Dota's sidebar.
fn sidebar(commands: &mut Commands, root: Entity, fonts: &Fonts) {
    let bar = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                width: px(SIDEBAR),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(15)),
                ..default()
            },
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                Color::srgb_u8(0x26, 0x2a, 0x2f).into(),
                Color::srgb_u8(0x13, 0x15, 0x18).into(),
            ])),
            ChildOf(root),
        ))
        .id();
    for (team, name, tint) in [
        (Team::Radiant, "Radiant", Color::srgb_u8(0x0e, 0x3b, 0x1a)),
        (Team::Dire, "Dire", Color::srgb_u8(0x3c, 0x0f, 0x16)),
    ] {
        let side = commands
            .spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::new(px(15), px(10), px(10), px(10)),
                    min_height: px(260),
                    border: UiRect::top(px(1)),
                    ..default()
                },
                BorderColor::all(Color::srgb_u8(0x36, 0x3d, 0x3b)),
                BackgroundGradient::from(LinearGradient::to_bottom_right(vec![
                    tint.into(),
                    Color::srgb_u8(0x15, 0x17, 0x19).into(),
                ])),
                ChildOf(bar),
            ))
            .id();
        commands.spawn((
            ui::text(name.to_uppercase(), &fonts.bold, 18.0, Color::WHITE),
            ChildOf(side),
        ));
        commands.spawn((
            TeamList(team),
            Node {
                flex_direction: FlexDirection::Column,
                ..default()
            },
            ChildOf(side),
        ));
    }
}

/// The throbber, the bar and the share, at the bottom right left of the
/// sidebar.
fn loading_panel(
    commands: &mut Commands,
    root: Entity,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
) {
    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(SIDEBAR + 40.0),
                bottom: px(40),
                align_items: AlignItems::Center,
                column_gap: px(10),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Throbber,
        Node {
            width: px(THROBBER),
            height: px(THROBBER),
            ..default()
        },
        ui::art(manifest, server, "loading.throbber"),
        ChildOf(panel),
    ));
    let well = commands
        .spawn((
            Node {
                width: px(BAR),
                height: px(6),
                ..default()
            },
            BackgroundColor(Color::srgb_u8(0x15, 0x17, 0x19)),
            ChildOf(panel),
        ))
        .id();
    commands.spawn((
        BarFill,
        Node {
            width: percent(0),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(0x32, 0x80, 0x22)),
        ChildOf(well),
    ));
    commands.spawn((
        Percent,
        ui::text("0%", &fonts.regular, 18.0, Color::srgb_u8(0xb4, 0xb4, 0xb4)),
        ChildOf(panel),
    ));
}

fn show(loading: Res<Loading>, mut root: Single<&mut Visibility, With<Root>>) {
    root.set_if_neq(ui::visible(loading.shown));
}

/// The players of the match: the lobby's seats while it lasts, else the
/// match's roster (a replay's).
pub fn rows(core: &Core) -> Vec<Row> {
    let name = |slot: SlotId, given: &str| {
        if given.is_empty() {
            crate::seat_name::seat_name(core, slot)
        } else {
            given.to_owned()
        }
    };
    if let Some(statics) = core.statics() {
        return statics
            .roster()
            .iter()
            .map(|pick| Row {
                team: pick.team,
                name: name(pick.slot, ""),
                hero: Some(pick.hero.0),
            })
            .collect();
    }
    core.lobby
        .slots
        .iter()
        .filter(|seat| seat.role.is_some())
        .map(|seat| Row {
            team: seat.team,
            name: name(seat.slot, &seat.name),
            hero: seat.hero.map(|h| h.0),
        })
        .collect()
}

/// Lays each side's players out again when they change.
fn rosters(
    mut commands: Commands,
    session: NonSend<Session>,
    loading: Res<Loading>,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    lists: Query<(Entity, &TeamList)>,
    mut laid: Local<Vec<Row>>,
) {
    if !loading.shown {
        return;
    }
    let now = rows(&session.0);
    if *laid == now {
        return;
    }
    for (list, team) in &lists {
        commands.entity(list).despawn_related::<Children>();
        for row in now.iter().filter(|r| r.team == team.0) {
            player(&mut commands, list, row, &fonts, &manifest, &server);
        }
    }
    *laid = now;
}

/// One player's row: the hero's portrait as the avatar, the name.
fn player(
    commands: &mut Commands,
    list: Entity,
    row: &Row,
    fonts: &Fonts,
    manifest: &Manifest,
    server: &AssetServer,
) {
    let line = commands
        .spawn((
            Node {
                margin: UiRect::all(px(5)),
                align_items: AlignItems::Center,
                column_gap: px(5),
                ..default()
            },
            ChildOf(list),
        ))
        .id();
    let avatar = row.hero.and_then(|hero| manifest.portrait(hero));
    commands.spawn((
        Node {
            width: px(AVATAR),
            height: px(AVATAR),
            ..default()
        },
        match avatar {
            Some(path) => (
                ImageNode::new(server.load(path)),
                BackgroundColor(Color::NONE),
            ),
            None => (
                ImageNode::default().with_color(Color::NONE),
                BackgroundColor(Color::srgb_u8(0x26, 0x2a, 0x2f)),
            ),
        },
        ui::Cover,
        ChildOf(line),
    ));
    commands.spawn((
        ui::text(
            &row.name,
            &fonts.regular,
            18.0,
            Color::srgb_u8(0xb4, 0xb4, 0xb4),
        ),
        ChildOf(line),
    ));
}

fn progress(
    loading: Res<Loading>,
    mut fill: Single<&mut Node, With<BarFill>>,
    mut share: Single<&mut Text, With<Percent>>,
) {
    if !loading.is_changed() {
        return;
    }
    let p = (loading.progress * 100.0).round();
    fill.width = percent(p);
    share.set_if_neq(Text(format!("{p}%")));
}

/// Dota's throbber turns once a second.
fn turn(
    loading: Res<Loading>,
    time: Res<Time>,
    mut throbber: Single<&mut UiTransform, With<Throbber>>,
) {
    if loading.shown {
        throbber.rotation = Rot2::radians(-time.elapsed_secs() * std::f32::consts::TAU);
    }
}
