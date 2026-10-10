//! HUD: bottom panel with ability icons (absolute-path PNG), custom TTF text, conic
//! cooldown sweep, and UiScale tied to the window's logical height.
//!
//! UI units are logical pixels: Bevy multiplies by the window scale factor (2 on this
//! HiDPI display) and then by `UiScale`, so no manual HiDPI handling is needed.

use std::f32::consts::TAU;

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    window::{PrimaryWindow, WindowResized},
};

use crate::{camera::CursorGround, coords::world_to_sim, fallback::FoundAssets};

const FONT_PATH: &str = "fonts/bota-sans-bold.ttf";
const ICON_SIZE: f32 = 64.0;
const PANEL_HEIGHT: f32 = 96.0;
/// Logical window height at which UiScale is 1.
const REFERENCE_HEIGHT: f32 = 900.0;
const SLOT_KEYS: [&str; 4] = ["Q", "W", "E", "R"];
const SLOT_COOLDOWNS: [f32; 4] = [3.0, 5.0, 8.0, 20.0];

#[derive(Component)]
struct Cooldown {
    total: f32,
}

#[derive(Component)]
struct CooldownText;

#[derive(Component)]
struct DebugText;

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, spawn_hud)
        .add_systems(Update, (update_cooldowns, update_debug_text, scale_ui));
}

fn spawn_hud(mut commands: Commands, server: Res<AssetServer>, found: Res<FoundAssets>) {
    let font: Handle<Font> = server.load(FONT_PATH);
    // Raw absolute path: refused under UnapprovedPathMode::Deny unless overridden per load.
    let icon: Handle<Image> = server
        .load_builder()
        .override_unapproved()
        .load(found.icon.clone());

    commands.spawn((
        DebugText,
        Text::new(""),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(0.9, 0.9, 0.8)),
        TextShadow {
            offset: Vec2::splat(1.5),
            ..default()
        },
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(10),
            ..default()
        },
    ));

    let panel = commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(0),
                left: percent(25),
                width: percent(50),
                height: px(PANEL_HEIGHT),
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: px(10),
                border_radius: BorderRadius::top(px(10)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.06, 0.07, 0.09, 0.9)),
        ))
        .id();
    for (i, (key, total)) in SLOT_KEYS.iter().zip(SLOT_COOLDOWNS).enumerate() {
        let slot = spawn_slot(&mut commands, icon.clone(), font.clone(), key, total, i);
        commands.entity(panel).add_child(slot);
    }
}

fn spawn_slot(
    commands: &mut Commands,
    icon: Handle<Image>,
    font: Handle<Font>,
    key: &str,
    total: f32,
    index: usize,
) -> Entity {
    let text_font = |size: f32| TextFont {
        font: font.clone().into(),
        font_size: FontSize::Px(size),
        ..default()
    };
    commands
        .spawn((
            Node {
                width: px(ICON_SIZE),
                height: px(ICON_SIZE),
                border: UiRect::all(px(2)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BorderColor::all(Color::srgb(0.35, 0.35, 0.4)),
            // Tint differs per slot so the four copies of one icon are distinguishable.
            ImageNode::new(icon).with_color(Color::hsl(index as f32 * 80.0, 0.5, 0.85)),
        ))
        .with_children(|slot| {
            // Cooldown sweep: a full-size overlay whose conic gradient is transparent over
            // the elapsed angle and dark over the remaining one (clockwise from 12 o'clock).
            slot.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: percent(100),
                    height: percent(100),
                    ..default()
                },
                BackgroundGradient::from(sweep_gradient(0.0)),
                Cooldown { total },
            ));
            slot.spawn((
                CooldownText,
                Text::new(""),
                text_font(22.0),
                TextShadow {
                    offset: Vec2::splat(1.5),
                    ..default()
                },
            ));
            slot.spawn((
                Text::new(key),
                text_font(13.0),
                Node {
                    position_type: PositionType::Absolute,
                    top: px(1),
                    left: px(3),
                    ..default()
                },
            ));
        })
        .id()
}

fn sweep_gradient(elapsed_fraction: f32) -> ConicGradient {
    let a = elapsed_fraction.clamp(0.0, 1.0) * TAU;
    let clear = Color::NONE;
    let dark = Color::srgba(0.0, 0.0, 0.0, 0.7);
    // Two stops at the same angle make a hard edge.
    ConicGradient::new(
        UiPosition::CENTER,
        vec![
            AngularColorStop::new(clear, 0.0),
            AngularColorStop::new(clear, a),
            AngularColorStop::new(dark, a),
            AngularColorStop::new(dark, TAU),
        ],
    )
}

fn update_cooldowns(
    time: Res<Time>,
    mut sweeps: Query<(&Cooldown, &mut BackgroundGradient, &ChildOf)>,
    children: Query<&Children>,
    mut texts: Query<&mut Text, With<CooldownText>>,
) {
    for (cd, mut gradient, child_of) in &mut sweeps {
        // Restart the cooldown every `total + 1` seconds (1 s "ready" gap).
        let t = time.elapsed_secs() % (cd.total + 1.0);
        let remaining = (cd.total - t).max(0.0);
        let elapsed_angle = (t / cd.total).min(1.0) * TAU;
        // Mutate the two boundary stops in place: no per-frame Vec allocation.
        if let Some(Gradient::Conic(conic)) = gradient.0.first_mut() {
            conic.stops[1].angle = Some(elapsed_angle);
            conic.stops[2].angle = Some(elapsed_angle);
        }
        let label = if remaining > 0.0 {
            format!("{}", remaining.ceil() as u32)
        } else {
            String::new()
        };
        for sibling in children.iter_descendants(child_of.parent()) {
            if let Ok(mut text) = texts.get_mut(sibling)
                && text.0 != label
            {
                text.0.clone_from(&label);
            }
        }
    }
}

fn update_debug_text(
    diagnostics: Res<DiagnosticsStore>,
    cursor: Res<CursorGround>,
    mut text: Single<&mut Text, With<DebugText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|d| d.smoothed())
        .unwrap_or(0.0);
    let cursor = cursor
        .0
        .map(|p| {
            let s = world_to_sim(p);
            format!("({:.0}, {:.0})", s.x, s.y)
        })
        .unwrap_or_else(|| "-".into());
    text.0 = format!(
        "{fps:.0} fps | cursor sim {cursor}\narrows/edge: pan  wheel: zoom  space: anim mode  +/-: idle speed  [/]: volume"
    );
}

/// Scale the whole HUD with the window's logical height (on top of the DPI scale).
fn scale_ui(
    mut resized: MessageReader<WindowResized>,
    window: Single<&Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
) {
    if resized.read().last().is_none() {
        return;
    }
    let scale = (window.height() / REFERENCE_HEIGHT).clamp(0.75, 2.0);
    if ui_scale.0 != scale {
        ui_scale.0 = scale;
    }
}
