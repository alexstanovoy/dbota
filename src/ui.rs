//! What every part of the interface shares: the fonts, the palette, the
//! interface scale, and small builders for text and boxes.

use bevy::{prelude::*, window::PrimaryWindow};

use crate::config::Settings;

/// Logical window height at which the interface scale is one.
const REFERENCE_HEIGHT: f32 = 900.0;
/// The smallest scale a window may force.
const MIN_SCALE: f32 = 0.25;

/// The interface fonts.
#[derive(Resource, Clone)]
pub struct Fonts {
    /// Titles, numbers and keys.
    pub bold: Handle<Font>,
    /// Body text.
    pub regular: Handle<Font>,
}

/// Dota's dark interface panels.
pub const PANEL: Color = Color::srgba(0.05, 0.06, 0.08, 0.93);
/// A panel drawn over a panel.
pub const PANEL_DEEP: Color = Color::srgba(0.02, 0.025, 0.035, 0.96);
/// Panel edges.
pub const EDGE: Color = Color::srgb(0.24, 0.27, 0.32);
/// Plain text.
pub const INK: Color = Color::srgb(0.92, 0.93, 0.95);
/// Secondary text.
pub const INK_DIM: Color = Color::srgb(0.6, 0.64, 0.7);
/// Gold, prices and levels.
pub const GOLDEN: Color = Color::srgb(1.0, 0.8, 0.32);
/// Mana.
pub const MANA: Color = Color::srgb(0.25, 0.5, 0.98);
/// Health of a friend.
pub const HP_FRIEND: Color = Color::srgb(0.3, 0.78, 0.32);
/// Health of one's own hero.
pub const HP_MINE: Color = Color::srgb(0.38, 0.95, 0.4);
/// Health of a foe.
pub const HP_FOE: Color = Color::srgb(0.86, 0.22, 0.18);

pub fn plugin(app: &mut App) {
    app.add_systems(PreStartup, load_fonts)
        .add_systems(Update, scale_ui);
}

fn load_fonts(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Fonts {
        bold: server.load("fonts/bota-sans-bold.ttf"),
        regular: server.load("fonts/bota-sans-regular.ttf"),
    });
}

/// A text bundle in a font at a size.
pub fn text(s: impl Into<String>, font: &Handle<Font>, size: f32, color: Color) -> impl Bundle {
    (
        Text::new(s),
        TextFont {
            font: font.clone().into(),
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(color),
    )
}

/// The thin dark outline every HUD text carries, so it reads on any ground.
pub fn shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::splat(1.0),
        color: Color::srgba(0.0, 0.0, 0.0, 0.9),
    }
}

/// Shown or hidden, as a part of the interface is.
pub fn visible(on: bool) -> Visibility {
    if on {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    }
}

/// A disc in two colours, `before` clockwise from twelve o'clock to the
/// angle `a` (radians) and `after` the rest of the way round.
pub fn split_disc(a: f32, before: Color, after: Color) -> BackgroundGradient {
    BackgroundGradient::from(ConicGradient::new(
        UiPosition::CENTER,
        vec![
            AngularColorStop::new(before, 0.0),
            AngularColorStop::new(before, a),
            AngularColorStop::new(after, a),
            AngularColorStop::new(after, std::f32::consts::TAU),
        ],
    ))
}

/// A node over the whole of its parent.
pub fn fill() -> Node {
    Node {
        position_type: PositionType::Absolute,
        width: percent(100),
        height: percent(100),
        ..default()
    }
}

/// A node placed at a corner of its parent.
pub fn absolute(left: f32, top: f32, width: f32, height: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: px(left),
        top: px(top),
        width: px(width),
        height: px(height),
        ..default()
    }
}

/// A part of the interface the scale must leave room for. Nodes carrying it
/// are measured in interface pixels; the scale never grows past the point
/// where they would meet or leave the window, as Dota's HUD keeps every panel
/// on the screen.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub enum Extent {
    /// What is docked at the bottom left (the minimap and its buttons).
    Left,
    /// The hero panel, centred at the foot.
    Bar,
    /// What is docked at the bottom right (the shop button).
    Right,
    /// The clock and the seats at the top centre.
    TopCentre,
    /// One's own numbers, in the top-left corner.
    Corner,
}

/// Interface pixels between a corner's panel and the window's edge.
pub const CORNER_MARGIN: f32 = 8.0;

/// The largest scale at which every [`Extent`] fits the window, given the
/// size of each in interface pixels.
fn fitting_scale(window: Vec2, size: impl Fn(Extent) -> Vec2) -> f32 {
    let (left, bar, right) = (size(Extent::Left), size(Extent::Bar), size(Extent::Right));
    let (top, corner) = (size(Extent::TopCentre), size(Extent::Corner));
    // The top centre stays clear of the corner on both sides.
    let width = (left.x + bar.x + right.x).max(top.x + 2.0 * (corner.x + CORNER_MARGIN));
    // The top centre sits above the panel; the corner above the minimap.
    let height = (bar.y + top.y).max(left.y + corner.y + CORNER_MARGIN);
    let fit = |have: f32, need: f32| {
        if need > 0.0 {
            have / need
        } else {
            f32::INFINITY
        }
    };
    fit(window.x, width).min(fit(window.y, height))
}

/// Scales the whole interface with the window's logical height and the
/// player's own factor, down to what fits the window.
fn scale_ui(
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<Settings>,
    parts: Query<(&Extent, &ComputedNode)>,
    mut ui_scale: ResMut<UiScale>,
) {
    let wanted = (window.height() / REFERENCE_HEIGHT).clamp(0.6, 2.5) * settings.ui_scale;
    let size = |which: Extent| {
        parts
            .iter()
            .filter(|(e, _)| **e == which)
            .map(|(_, node)| node.size() * node.inverse_scale_factor())
            .fold(Vec2::ZERO, Vec2::max)
    };
    let scale = wanted
        .min(fitting_scale(window.size(), size))
        .max(MIN_SCALE);
    if (ui_scale.0 - scale).abs() > 1e-3 {
        ui_scale.0 = scale;
    }
}
