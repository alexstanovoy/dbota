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
use bota_core::rules::{SCAN_MAX_CHARGES, SCAN_RADIUS, SCAN_RECHARGE_TICKS};

use crate::camera::CursorGround;
use crate::hud::Docks;
use crate::manifest::Manifest;
use crate::map::Ground;
use crate::minimap::MinimapFrame;
use crate::session::{MatchKnown, Session};
use crate::ui::{self, Fonts, dota};

/// The frame the button sits in.
#[derive(Component)]
struct ScanFrame;

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

/// The badge's diameter, in logical pixels.
const BADGE: f32 = 18.0;
/// The ring colours of the running scan particle: green while nothing is found, red when found.
pub(crate) const SEARCHING_COLOUR: Color = Color::srgb(0.0, 0.75, 0.0);
pub(crate) const FOUND_COLOUR: Color = Color::srgb(0.82, 0.34, 0.34);
/// The preview under the cursor while aiming: Dota's preview green.
pub(crate) const PREVIEW_COLOUR: Color = Color::srgb(0.3, 0.76, 0.32);

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn)
        .add_observer(click)
        .add_systems(
            Update,
            (show, minimap_rings).run_if(|k: Res<MatchKnown>| k.0),
        )
        .add_systems(PostUpdate, draw.after(crate::camera::CursorPicked));
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
            ScanFrame,
            Node {
                position_type: PositionType::Absolute,
                left: dota(crate::glyph::PLATE_LEFT),
                bottom: dota(crate::glyph::SCAN_BOTTOM),
                ..default()
            },
            Visibility::Hidden,
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
    let badge = commands
        .spawn((
            ScanBadge,
            Node {
                position_type: PositionType::Absolute,
                right: px(-2),
                bottom: px(-2),
                width: px(BADGE),
                height: px(BADGE),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.11, 0.13)),
            Pickable::IGNORE,
            Visibility::Hidden,
            ChildOf(button),
        ))
        .id();
    commands.spawn((
        ScanCount,
        ui::text("", &fonts.bold, 12.0, ui::INK),
        Pickable::IGNORE,
        ChildOf(badge),
    ));
}

/// The side whose Scan the HUD shows, and the game-clock tick the wait is counted from.
fn shown(core: &Core) -> Option<(ScanSide, u32)> {
    let tick = core.view()?.game_tick();
    Some((core.scan()?, tick))
}

/// The button: its icon, its sweep, its badge.
#[allow(clippy::type_complexity)]
fn show(
    session: NonSend<Session>,
    mut frame: Single<&mut Visibility, (With<ScanFrame>, Without<ScanBadge>)>,
    mut icon: Single<(&ScanIcon, &mut ImageNode), Without<ScanSweep>>,
    mut sweep: Single<&mut BackgroundGradient, With<ScanSweep>>,
    mut badge: Single<&mut Visibility, (With<ScanBadge>, Without<ScanFrame>)>,
    mut count: Single<&mut Text, With<ScanCount>>,
    mut button: Single<&mut BackgroundColor, (With<ScanButton>, Without<ScanSweep>)>,
) {
    let core = &session.0;
    let Some((side, now)) = shown(core) else {
        frame.set_if_neq(Visibility::Hidden);
        return;
    };
    frame.set_if_neq(Visibility::Inherited);
    let charges = side.ledger.charges;
    let (art, art_color) = match (&icon.0.on, &icon.0.off, charges) {
        (_, Some(off), 0) => (off.clone(), Color::WHITE),
        (Some(on), _, _) => (on.clone(), Color::WHITE),
        _ => (Handle::default(), Color::NONE),
    };
    let (_, image) = &mut *icon;
    if image.image != art {
        image.image = art;
    }
    if image.color != art_color {
        image.color = art_color;
    }
    // Dark over what is still to wait for the next charge, clear over what has passed; no cover
    // at all while every charge is held.
    let waiting = charges < SCAN_MAX_CHARGES;
    let left = side.ledger.next_in(now).map_or(0.0, |ticks| {
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
    count.set_if_neq(Text(charges.to_string()));
    // The aim taken up lights the plate behind the icon.
    let plate = if core.scan_aim {
        Color::srgb(0.18, 0.4, 0.2)
    } else {
        ui::PANEL_DEEP
    };
    button.set_if_neq(BackgroundColor(plate));
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
) {
    let core = &session.0;
    let radius = SCAN_RADIUS as f32;
    if core.scan_aim
        && let Some(at) = cursor.0.map(|p| (p.x, -p.z))
    {
        ring(&mut gizmos, &ground, at, radius, PREVIEW_COLOUR);
    }
    let Some((side, _)) = shown(core) else {
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
    mut commands: Commands,
    session: NonSend<Session>,
    frame: Option<Single<Entity, With<MinimapFrame>>>,
    mut rings: Query<(&mut Node, &mut BorderColor, &mut Visibility), With<MinimapRing>>,
    made: Query<(), With<MinimapRing>>,
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
    let shown: Vec<bota_proto::RadarView> = shown(&session.0)
        .map(|(side, _)| side.radars)
        .unwrap_or_default();
    let mut radars = shown.iter();
    for (mut node, mut border, mut visibility) in &mut rings {
        let Some(radar) = radars.next() else {
            visibility.set_if_neq(Visibility::Hidden);
            continue;
        };
        let centre = crate::minimap::on_map(radar.at.x.to_f32(), radar.at.y.to_f32());
        let across = 2.0 * crate::minimap::map_to_pixels(SCAN_RADIUS as f32);
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
    let title = texts
        .label("DOTA_Tooltip_ability_radar", &[("scan_hotkey", key)])
        .unwrap_or_else(|| "Scan".to_string());
    let about = texts
        .label("DOTA_Tooltip_ability_radar_Description", &[])
        .unwrap_or_else(|| {
            format!(
                "Scans a targeted {SCAN_RADIUS} AoE for {} seconds. Indicates whether there are enemy heroes in that area during the {} seconds.",
                bota_core::rules::SCAN_DURATION_TICKS / bota_core::rules::TICKS_PER_SECOND,
                bota_core::rules::SCAN_DURATION_TICKS / bota_core::rules::TICKS_PER_SECOND,
            )
        });
    let rate = u32::from(core.tick_rate().max(1));
    let mut lines = format!(
        "{title}\n{about}\nCooldown: {}\nCharges Available: {}",
        SCAN_RECHARGE_TICKS / rate,
        side.ledger.charges
    );
    if let Some(ticks) = side.ledger.next_in(now) {
        lines.push_str(&format!("\nNext charge in: {}", ticks.div_ceil(rate)));
    }
    Some(lines)
}
