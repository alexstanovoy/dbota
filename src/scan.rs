//! Scan, as Dota's HUD shows it: the round button above the Glyph's in the frame against the
//! minimap's right side, lit with the charges it holds in a badge and a dark sweep over it
//! while a charge is still to come; its click or key takes it up for aiming, the next click on
//! the world or on the minimap aims it, and the scans that look for one's side show as rings
//! of the rule's radius in the world and on the minimap, green while they find nothing and red
//! while they find an enemy hero.

use std::f32::consts::TAU;

use bevy::{
    picking::{events::PointerPress, hover::Hovered, pointer::PointerButton},
    prelude::*,
    ui_widgets::Button,
};
use bota_client_core::{App as Core, ScanSide};
use bota_core::ScanLedger;
use bota_core::rules::{SCAN_MAX_CHARGES, SCAN_RADIUS, SCAN_RECHARGE_TICKS};
use bota_proto::Team;

use crate::camera::CursorGround;
use crate::hud::Docks;
use crate::manifest::Manifest;
use crate::map::Ground;
use crate::minimap::MinimapFrame;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts, dota, team_buttons};

#[derive(Resource)]
struct SpectatorScan(team_buttons::TeamFaces);

/// The button; the hover popup reads it.
#[derive(Component)]
pub struct ScanButton;

/// The button's icon: Dota's lit dial, and the dimmed one at no charge.
#[derive(Component)]
struct ScanIcon {
    on: Option<Handle<Image>>,
    off: Option<Handle<Image>>,
}

/// The dark sweep over the icon while a charge is still to come.
#[derive(Component)]
struct ScanSweep;

/// The badge with the charges held, and the number in it.
#[derive(Component)]
struct ScanBadge;

#[derive(Component)]
pub(crate) struct ScanCount;

/// One ring of a running scan on the minimap.
#[derive(Component)]
struct MinimapRing;

/// The ring colours of the running scan particle: green while nothing is found, red when found.
pub(crate) const SEARCHING_COLOUR: Color = Color::srgb(0.0, 0.75, 0.0);
pub(crate) const FOUND_COLOUR: Color = Color::srgb(0.82, 0.34, 0.34);
/// The preview under the cursor while aiming: Dota's preview green.
pub(crate) const PREVIEW_COLOUR: Color = Color::srgb(0.3, 0.76, 0.32);

pub fn plugin(app: &mut App) {
    app.init_resource::<DisplayedScan>()
        .add_systems(Startup, spawn)
        .add_observer(click)
        .add_systems(Update, updates())
        .add_systems(PostUpdate, draw.after(crate::camera::CursorPicked));
}

pub(crate) fn updates() -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (
        sample,
        (show, show_teams, minimap_rings).run_if(|k: Res<MatchKnown>| k.0),
    )
        .chain()
}

fn spawn(
    mut commands: Commands,
    fonts: Res<Fonts>,
    manifest: Res<Manifest>,
    server: Res<AssetServer>,
    docks: Res<Docks>,
) {
    // On the plate, in the upper of its two wells (`#RadarButton`).
    let frame = commands
        .spawn((
            Name::new("hud scan well"),
            Node {
                position_type: PositionType::Absolute,
                left: dota(crate::glyph::PLATE_LEFT),
                bottom: dota(crate::glyph::SCAN_BOTTOM),
                ..default()
            },
        ))
        .id();
    commands
        .entity(docks.minimap_buttons)
        .insert_children(0, &[frame]);
    let button = commands
        .spawn((
            ScanButton,
            Name::new("hud scan"),
            Button,
            Hovered::default(),
            bevy::picking::hover::DirectlyHovered::default(),
            Node {
                width: dota(crate::glyph::BUTTON),
                height: dota(crate::glyph::BUTTON),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
            Visibility::Hidden,
            ChildOf(frame),
        ))
        .id();
    let load = |name: &str| manifest.file(name).map(|path| server.load(path));
    let round = Node {
        border_radius: BorderRadius::MAX,
        ..ui::fill()
    };
    commands.spawn((
        ScanIcon {
            on: load("hud.scan_on"),
            off: load("hud.scan_off"),
        },
        round.clone(),
        ImageNode::default().with_color(Color::NONE),
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    commands.spawn((
        ScanSweep,
        round,
        ui::split_disc(TAU, Color::NONE, Color::NONE),
        Pickable::IGNORE,
        ChildOf(button),
    ));
    let (badge, count) = ui::spawn_charge_badge(&mut commands, &fonts, button);
    commands
        .entity(badge)
        .insert((ScanBadge, Name::new("hud scan charges"), Visibility::Hidden));
    commands.entity(count).insert(ScanCount);
    let teams = team_buttons::TeamFaces::spawn(
        &mut commands,
        frame,
        team_buttons::Ability::Scan,
        [load("hud.scan_radiant"), load("hud.scan_dire")],
        load("hud.scan_off"),
    );
    commands.insert_resource(SpectatorScan(teams));
}

/// The side whose Scan the HUD shows, and the game-clock tick the wait is counted from.
fn shown(core: &Core) -> Option<(ScanSide, u32)> {
    let tick = core.view()?.game_tick();
    let scan = if core.my_slot.is_none()
        && let Some(team) = core.view()?.viewer
    {
        core.scan_of(team)
    } else {
        core.scan()
    };
    Some((scan?, tick))
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ScanFrameKey {
    hud: crate::hud::HudFrame,
    event_tick: Option<u32>,
}

#[derive(Default, Resource)]
struct DisplayedScan {
    selected: Option<(ScanSide, u32)>,
    teams: Option<([ScanLedger; 2], u32)>,
}

/// Scan facts can arrive after their snapshot. Read once when either part changes so the
/// button, minimap and world draw all use the same sample.
fn sample(
    session: NonSend<Session>,
    game: Res<crate::main_menu::GameEpoch>,
    mut read: Local<Option<ScanFrameKey>>,
    mut displayed: ResMut<DisplayedScan>,
) {
    let core = &session.0;
    let key = ScanFrameKey {
        hud: crate::hud::HudFrame::new(core, game.0),
        event_tick: core.session().and_then(|session| session.last_event_tick()),
    };
    if read.replace(key) != Some(key) {
        let scan = shown(core);
        let teams = core
            .view()
            .filter(|_| team_buttons::all_teams(core))
            .and_then(|view| {
                let ledger = |team| core.scan_of(team).map(|side| side.ledger);
                Some((
                    [ledger(Team::Radiant)?, ledger(Team::Dire)?],
                    view.game_tick(),
                ))
            });
        if displayed.selected != scan || displayed.teams != teams {
            *displayed = DisplayedScan {
                selected: scan,
                teams,
            };
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct ScanButtonState {
    charges: u8,
    remaining: Option<u32>,
    aiming: bool,
}

/// The button: its icon, its sweep, its badge.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    displayed: Res<DisplayedScan>,
    mut drawn: Local<Option<ScanButtonState>>,
    mut frame: Single<&mut Visibility, (With<ScanButton>, Without<ScanBadge>)>,
    mut icon: Single<(&ScanIcon, &mut ImageNode), Without<ScanSweep>>,
    mut sweep: Single<&mut BackgroundGradient, With<ScanSweep>>,
    mut badge: Single<&mut Visibility, (With<ScanBadge>, Without<ScanButton>)>,
    mut count: Single<&mut Text, With<ScanCount>>,
    mut button: Single<&mut BackgroundColor, (With<ScanButton>, Without<ScanSweep>)>,
) {
    let core = &session.0;
    let Some((side, now)) = displayed
        .selected
        .as_ref()
        .filter(|_| !team_buttons::all_teams(core))
    else {
        *drawn = None;
        frame.set_if_neq(Visibility::Hidden);
        return;
    };
    let charges = side.ledger.charges;
    let remaining = side.ledger.next_in(*now);
    let state = ScanButtonState {
        charges,
        remaining,
        aiming: core.scan_aim,
    };
    let previous = drawn.replace(state);
    if previous == Some(state) {
        return;
    }
    frame.set_if_neq(Visibility::Inherited);
    if previous.is_none_or(|old| (old.charges == 0) != (charges == 0)) {
        let (art, colour) = match (&icon.0.on, &icon.0.off, charges) {
            (_, Some(off), 0) => (off.clone(), Color::WHITE),
            (Some(on), _, _) => (on.clone(), Color::WHITE),
            _ => (Handle::default(), Color::NONE),
        };
        let (_, image) = &mut *icon;
        image.image = art;
        image.color = colour;
    }
    // Dark over what is still to wait for the next charge, clear over what has passed; no cover
    // at all while every charge is held.
    let waiting = charges < SCAN_MAX_CHARGES;
    let left = remaining.map_or(0.0, |ticks| {
        (ticks as f32 / SCAN_RECHARGE_TICKS as f32).clamp(0.0, 1.0)
    });
    let dark = Color::srgba(0.0, 0.0, 0.0, if waiting { 0.87 } else { 0.0 });
    let wanted = ui::split_disc((1.0 - left) * TAU, Color::NONE, dark);
    if **sweep != wanted {
        **sweep = wanted;
    }
    badge.set_if_neq(if charges > 0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    });
    if previous.is_none_or(|old| old.charges != charges) {
        count.0 = charges.to_string();
    }
    // The aim taken up lights the plate behind the icon.
    let plate = if core.scan_aim {
        Color::srgb(0.18, 0.4, 0.2)
    } else {
        ui::PANEL_DEEP
    };
    button.set_if_neq(BackgroundColor(plate));
}

fn show_teams(
    displayed: Res<DisplayedScan>,
    mut faces: ResMut<SpectatorScan>,
    mut paint: team_buttons::Paint,
) {
    let states = displayed.teams.map(|(ledgers, now)| {
        ledgers.map(|ledger| {
            let left = ledger
                .next_in(now)
                .map_or(0.0, |wait| wait as f32 / SCAN_RECHARGE_TICKS as f32);
            team_buttons::FaceState::new(ledger.charges == 0, left)
        })
    });
    faces.0.paint(states, &mut paint);
}

/// A click on the button takes the Scan up for aiming, or lays the aim down again.
fn click(
    event: On<PointerPress>,
    mut session: NonSendMut<Session>,
    button: Query<(), With<ScanButton>>,
) {
    if event.button == PointerButton::Primary && button.contains(event.entity) {
        session.0.press_scan();
    }
}

/// A ring on the ground around a world point, `radius` world units out.
fn ring(gizmos: &mut Gizmos, ground: &Ground, (x, y): (f32, f32), radius: f32, color: Color) {
    let steps = ((radius / 14.0) as usize).clamp(24, 160);
    let point = |i: usize| {
        let a = i as f32 / steps as f32 * TAU;
        let (px, py) = (x + radius * a.cos(), y + radius * a.sin());
        Vec3::new(px, ground.at(px, py) + crate::overlay::LIFT, -py)
    };
    gizmos.linestrip((0..=steps).map(point), color);
}

/// The rule's radius round the cursor while the Scan is up for aiming, and the rings of the
/// scans that look: both from the rule's radius, never from the art of the preview.
fn draw(
    mut gizmos: Gizmos,
    session: NonSend<Session>,
    ground: Res<Ground>,
    cursor: Res<CursorGround>,
    displayed: Res<DisplayedScan>,
) {
    let core = &session.0;
    let radius = SCAN_RADIUS as f32;
    if core.scan_aim
        && let Some(at) = cursor.0.map(|p| (p.x, -p.z))
    {
        ring(&mut gizmos, &ground, at, radius, PREVIEW_COLOUR);
    }
    let Some((side, _)) = &displayed.selected else {
        return;
    };
    for radar in &side.radars {
        let at = (radar.at.x.to_f32(), radar.at.y.to_f32());
        let color = if radar.found {
            FOUND_COLOUR
        } else {
            SEARCHING_COLOUR
        };
        ring(&mut gizmos, &ground, at, radius, color);
        ring(
            &mut gizmos,
            &ground,
            at,
            radius * 0.97,
            color.with_alpha(0.45),
        );
    }
}

/// The rings of the running scans on the minimap, one node each, made as the minimap's frame
/// appears: its diameter is the rule's radius twice over, to the map's scale.
fn minimap_rings(
    projection: Res<crate::minimap::MapProjection>,
    mut commands: Commands,
    displayed: Res<DisplayedScan>,
    frame: Option<Single<Entity, With<MinimapFrame>>>,
    mut rings: Query<(&mut Node, &mut BorderColor, &mut Visibility), With<MinimapRing>>,
    made: Query<(), With<MinimapRing>>,
    added: Query<(), Added<MinimapRing>>,
) {
    let Some(frame) = frame else {
        return;
    };
    if made.is_empty() {
        for _ in 0..bota_proto::MAX_SCAN_RADARS {
            commands.spawn((
                MinimapRing,
                Node {
                    position_type: PositionType::Absolute,
                    border: UiRect::all(px(2)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BorderColor::all(SEARCHING_COLOUR),
                Visibility::Hidden,
                Pickable::IGNORE,
                ChildOf(*frame),
            ));
        }
        return;
    }
    if !displayed.is_changed() && !projection.is_changed() && added.is_empty() {
        return;
    }
    let mut radars = displayed
        .selected
        .as_ref()
        .into_iter()
        .flat_map(|(side, _)| &side.radars);
    for (mut node, mut border, mut visibility) in &mut rings {
        let Some(radar) = radars.next() else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let centre = projection.on_map(radar.at.x.to_f32(), radar.at.y.to_f32());
        let across = 2.0 * projection.pixels(SCAN_RADIUS as f32);
        node.set_if_neq(Node {
            left: px(centre.x - across / 2.0),
            top: px(centre.y - across / 2.0),
            width: px(across),
            height: px(across),
            ..node.clone()
        });
        border.set_if_neq(BorderColor::all(if radar.found {
            FOUND_COLOUR
        } else {
            SEARCHING_COLOUR
        }));
        visibility.set_if_neq(Visibility::Inherited);
    }
}

/// The hover popup of the button: Dota's title and description, the charges held and the wait.
pub fn tip(
    texts: &crate::dota_text::Texts,
    core: &Core,
    settings: &crate::config::Settings,
) -> Option<String> {
    let (side, now) = shown(core)?;
    let key = settings
        .keys
        .get(crate::keys::index_of(crate::keys::Action::Scan)?)
        .copied()
        .flatten()
        .map_or_else(String::new, |chord| chord.name());
    tip_of(texts, side.ledger, now, core.tick_rate(), key, None)
}

pub(crate) fn team_tip(texts: &crate::dota_text::Texts, core: &Core, team: Team) -> Option<String> {
    let now = core.view()?.game_tick();
    tip_of(
        texts,
        core.scan_of(team)?.ledger,
        now,
        core.tick_rate(),
        String::new(),
        Some(team),
    )
}

fn tip_of(
    texts: &crate::dota_text::Texts,
    ledger: ScanLedger,
    now: u32,
    rate: u16,
    key: String,
    team: Option<Team>,
) -> Option<String> {
    let mut title = texts.label("DOTA_Tooltip_ability_radar", &[("scan_hotkey", key)])?;
    let lines = [
        team.and_then(team_buttons::team_label)
            .and_then(|key| texts.label(key, &[])),
        texts.label("DOTA_Tooltip_ability_radar_Description", &[]),
        texts.label(
            "DOTA_HUD_RadarCharges",
            &[("charges_available", ledger.charges.to_string())],
        ),
        ledger.next_in(now).and_then(|wait| {
            texts.label(
                "DOTA_HUD_RadarCooldown",
                &[(
                    "cooldown_time",
                    wait.div_ceil(u32::from(rate.max(1))).to_string(),
                )],
            )
        }),
    ];
    for line in lines.into_iter().flatten() {
        title.push('\n');
        title.push_str(&line);
    }
    Some(title)
}
