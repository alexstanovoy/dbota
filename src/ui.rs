//! What every part of the interface shares: the fonts, the palette, the
//! interface scale, and small builders for text and boxes.

use bevy::{prelude::*, text::FontCx, window::PrimaryWindow};
use parley::fontique::{FallbackKey, Script};

use crate::config::Settings;
use crate::manifest::Manifest;

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

/// The family of the shipped Han, kana and Hangul font, which the Latin fonts
/// lack. It stands behind them for those scripts; native builds find the
/// system's fonts for the rest, a browser has none.
const CJK_FAMILY: &str = "Bota Sans CJK";
const CJK_SCRIPTS: [&str; 5] = ["Hani", "Hira", "Kana", "Hang", "Bopo"];

/// Keeps the fallback font loaded.
#[derive(Resource)]
struct FallbackFont(#[expect(dead_code, reason = "only keeps the font loaded")] Handle<Font>);

pub fn plugin(app: &mut App) {
    app.add_systems(PreStartup, load_fonts)
        .add_systems(Update, (scale_ui, register_fallback))
        .add_systems(PostUpdate, cover.after(bevy::ui::UiSystems::Layout));
}

fn load_fonts(mut commands: Commands, server: Res<AssetServer>) {
    commands.insert_resource(Fonts {
        bold: server.load("fonts/bota-sans-bold.ttf"),
        regular: server.load("fonts/bota-sans-regular.ttf"),
    });
    commands.insert_resource(FallbackFont(server.load("fonts/bota-sans-cjk.otf")));
}

/// The font collection, with the system's fonts where it has them. Fontique
/// panics when fontconfig knows no font at all; the collection is then built
/// again with fontconfig's own config made unreadable, which it takes to mean no
/// system fonts, and only the shipped fonts are used. Call it before any
/// thread starts: it may set an environment variable.
#[cfg(not(target_arch = "wasm32"))]
pub fn font_cx() -> FontCx {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let found = std::panic::catch_unwind(FontCx::default);
    std::panic::set_hook(hook);
    found.unwrap_or_else(|_| {
        eprintln!(
            "dbota: the system has no fonts fontconfig can list; using the shipped fonts only"
        );
        // SAFETY: no other thread exists yet, so nothing reads the environment meanwhile.
        unsafe { std::env::set_var("FONTCONFIG_FILE", "/nonexistent/fonts.conf") };
        FontCx::default()
    })
}

/// Makes the CJK font the fallback of its scripts once Bevy has taken it into
/// the font collection.
fn register_fallback(
    mut cx: ResMut<FontCx>,
    mut texts: Query<&mut TextFont>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    let Some(family) = cx.collection.family_id(CJK_FAMILY) else {
        return;
    };
    for script in CJK_SCRIPTS {
        if let Ok(script) = script.parse::<Script>() {
            cx.collection
                .set_fallbacks(FallbackKey::new(script, None), std::iter::once(family));
        }
    }
    // Text laid out before the font arrived (a replay's chat lines, read ahead) is shaped again.
    for mut font in &mut texts {
        font.set_changed();
    }
    info!("interface fonts: {CJK_FAMILY} stands behind the Latin ones for CJK scripts");
    *done = true;
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

/// Dota's art at `logical` in the cache, or a plain disc or box of
/// `stand_in` where the cache has none.
pub fn art(
    manifest: &Manifest,
    server: &AssetServer,
    logical: &str,
    stand_in: Color,
) -> impl Bundle {
    match manifest.file(logical) {
        Some(path) => (
            ImageNode::new(server.load(path)),
            BackgroundColor(Color::NONE),
        ),
        None => (
            ImageNode::default().with_color(Color::NONE),
            BackgroundColor(stand_in),
        ),
    }
}

/// A picture that fills its node as Dota fills a portrait's frame: scaled
/// until it covers the frame and cut to the frame's shape about its middle,
/// never shrunk to fit with bars beside it. Its image node stretches.
#[derive(Component, Clone, Copy, Default)]
#[require(ImageNode)]
pub struct Cover;

/// The part of a `source` sized picture that covers a `frame`: the middle of
/// it, of the frame's shape. None while either has no size.
pub fn cover_rect(source: Vec2, frame: Vec2) -> Option<Rect> {
    if source.cmple(Vec2::ZERO).any() || frame.cmple(Vec2::ZERO).any() {
        return None;
    }
    let seen = frame / (frame / source).max_element();
    let corner = (source - seen) / 2.0;
    Some(Rect::from_corners(corner, corner + seen))
}

/// Cuts each [`Cover`] picture to its frame, once both have a size.
fn cover(
    images: Res<Assets<Image>>,
    mut pictures: Query<(&mut ImageNode, &ComputedNode), With<Cover>>,
) {
    for (mut picture, node) in &mut pictures {
        let rect = images
            .get(&picture.image)
            .and_then(|image| cover_rect(image.size_f32(), node.content_box().size()));
        if picture.image_mode != NodeImageMode::Stretch {
            picture.image_mode = NodeImageMode::Stretch;
        }
        if picture.rect != rect {
            picture.rect = rect;
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A tall picture in a wide frame keeps its whole width and the middle
    /// of its height; a wide one in a tall frame the middle of its width:
    /// either way the frame's shape, so nothing is letterboxed.
    #[test]
    fn a_cover_is_the_middle_of_the_picture_in_the_frames_shape() {
        let tall = cover_rect(Vec2::new(150.0, 200.0), Vec2::new(300.0, 150.0)).unwrap();
        assert_eq!(tall, Rect::new(0.0, 62.5, 150.0, 137.5));
        let wide = cover_rect(Vec2::new(256.0, 144.0), Vec2::new(176.0, 156.0)).unwrap();
        assert!((wide.width() / wide.height() - 176.0 / 156.0).abs() < 1e-4);
        assert!((wide.min.x + wide.max.x - 256.0).abs() < 1e-3, "centred");
        assert_eq!(wide.height(), 144.0);
        assert!(cover_rect(Vec2::ZERO, Vec2::ONE).is_none());
    }
}
