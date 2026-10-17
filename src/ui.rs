//! What every part of the interface shares: the fonts, the palette, the
//! interface scale, and small builders for text and boxes.

use bevy::{
    prelude::*,
    window::{PrimaryWindow, WindowResized},
};

use crate::config::Settings;

/// Logical window height at which the interface scale is one.
const REFERENCE_HEIGHT: f32 = 900.0;

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

/// Scales the whole interface with the window's logical height and the
/// player's own factor.
fn scale_ui(
    mut resized: MessageReader<WindowResized>,
    window: Single<&Window, With<PrimaryWindow>>,
    settings: Res<Settings>,
    mut ui_scale: ResMut<UiScale>,
    mut first: Local<bool>,
) {
    if resized.read().last().is_none() && !settings.is_changed() && *first {
        return;
    }
    *first = true;
    let scale = (window.height() / REFERENCE_HEIGHT).clamp(0.6, 2.5) * settings.ui_scale;
    if ui_scale.0 != scale {
        ui_scale.0 = scale;
    }
}
