//! The menu's design tokens: its colours, sizes and type roles, in one place. Sizes are interface
//! pixels, which are the mockups' pixels (a 1440 by 900 frame; see `ui::REFERENCE_HEIGHT`).
//! Dota's interface is square and flat: no glow, no rounded panels.

use bevy::prelude::*;
use bevy::text::{FontFeatureTag, FontFeatures, LetterSpacing};

use crate::ui::Fonts;

/// A colour of `#rrggbb`.
const fn hex(rgb: u32) -> Color {
    Color::srgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// What everything stands on.
pub const GROUND: Color = hex(0x07090C);
/// The dim behind an overlay window.
pub fn scrim() -> Color {
    Color::srgba(0.0, 0.0, 0.0, dim(0.62))
}

/// The alpha that darkens as much as a black of alpha `css` does in the mockups. A browser blends
/// in sRGB, where a black at 0.5 halves the stored value; Bevy blends in linear light, where it
/// halves the light, which looks much brighter. A stored value times `1 - css` is the light
/// times `(1 - css)^2.2`.
pub fn dim(css: f32) -> f32 {
    1.0 - (1.0 - css).powf(2.2)
}

/// The top bar, top to bottom, and its lower edge.
pub const BAR: (Color, Color) = (hex(0x000000), hex(0x0D1218));
pub const BAR_EDGE: Color = hex(0x1F2A33);
/// The faint line of light under the top bar's edge.
pub const BAR_GLINT: Color = Color::srgba(127.0 / 255.0, 212.0 / 255.0, 245.0 / 255.0, 0.14);
/// How tall the top bar is.
pub const BAR_HEIGHT: f32 = 54.0;

/// A panel over the menu's background, top to bottom, its edge and the light along its top.
pub const PANEL: (Color, Color) = (
    Color::srgba(27.0 / 255.0, 32.0 / 255.0, 38.0 / 255.0, 0.92),
    Color::srgba(14.0 / 255.0, 17.0 / 255.0, 20.0 / 255.0, 0.92),
);
pub const PANEL_EDGE: Color = hex(0x24303A);
pub const PANEL_GLINT: Color = Color::srgba(127.0 / 255.0, 212.0 / 255.0, 245.0 / 255.0, 0.35);

/// An overlay window: its ground, the ground of its side navigation (top to bottom), its edge.
pub const WINDOW: Color = hex(0x1B1E21);
pub const WINDOW_NAV: (Color, Color) = (hex(0x121618), hex(0x0C1011));
pub const WINDOW_EDGE: Color = hex(0x2E3B47);
/// The rule under a window's header.
pub const DIVIDER: Color = hex(0x222C35);

/// A quiet surface: secondary controls.
pub const SURFACE: Color = hex(0x161B20);
/// A field to pick from, one to type in, and their edge.
pub const FIELD: Color = hex(0x222629);
pub const FIELD_SUNKEN: Color = hex(0x0B0C0E);
pub const FIELD_EDGE: Color = hex(0x495255);

/// Values and titles.
pub const INK_1: Color = hex(0xFFFFFF);
/// Labels.
pub const INK_2: Color = hex(0xE2E6EA);
/// Secondary text and icons.
pub const INK_3: Color = hex(0xC5CBD0);
/// Captions.
pub const INK_4: Color = hex(0x9AA3AD);
/// Placeholders and faint labels.
pub const INK_5: Color = hex(0x8B939C);
/// Disabled text.
pub const INK_OFF: Color = hex(0x5C656E);

/// The mark of what is selected, and of the keyboard's focus.
pub const SELECT: Color = hex(0x7FD4F5);
pub const FOCUS: Color = hex(0x59B0D2);
/// The fill of a selected tab or option, left to right.
pub const SELECT_FILL: (Color, Color) = (
    Color::srgba(51.0 / 255.0, 130.0 / 255.0, 1.0, 0.32),
    Color::srgba(51.0 / 255.0, 130.0 / 255.0, 1.0, 0.04),
);
/// The local player's row, and the player's card at the bottom left.
pub const SELF_ROW: Color = Color::srgba(51.0 / 255.0, 130.0 / 255.0, 1.0, 0.18);
/// The square of the player's initial (their avatar later), top to bottom, and its edge.
pub const AVATAR: (Color, Color) = (hex(0x2D4A6B), hex(0x1A2633));
pub const AVATAR_EDGE: Color = hex(0x4A6A86);
/// A side navigation's tab that is not open.
pub const NAV_INK: Color = hex(0xB4BBC1);
/// The underline of the top bar's current tab.
pub const TAB_MARK: Color = hex(0x3382FF);

/// Buttons, top to bottom: the action a screen is for, a secondary one, one that leaves, and
/// one that cannot be pressed now.
pub const GO: (Color, Color) = (hex(0x5AA862), hex(0x2F6537));
pub const GO_GLINT: Color = Color::srgba(1.0, 1.0, 1.0, 0.35);
pub const SECONDARY: (Color, Color) = (hex(0x2A3A47), hex(0x1A242C));
pub const SECONDARY_EDGE: Color = hex(0x3E5566);
pub const STOP: (Color, Color) = (hex(0xA3372D), hex(0x6E1F19));
pub const STOP_EDGE: Color = Color::srgba(0.0, 0.0, 0.0, 0.5);
pub const DISABLED: (Color, Color) = (hex(0x3A4148), hex(0x2A2F33));
pub const DISABLED_EDGE: Color = hex(0x30363B);

/// Best values, gold.
pub const GOLD: Color = hex(0xF2C14E);
/// The sides' text and fill colours.
pub const RADIANT_TEXT: Color = hex(0x9BE36B);
pub const RADIANT: Color = hex(0x7BC74D);
pub const DIRE_TEXT: Color = hex(0xFF8A73);
pub const DIRE: Color = hex(0xE0583F);
/// A refusal or failure.
pub const ERROR_TEXT: Color = hex(0xFF8A73);

/// The top bar's connection pill: connected, without an account, and lost.
pub const PILL_ONLINE: (Color, Color) = (
    Color::srgba(123.0 / 255.0, 199.0 / 255.0, 77.0 / 255.0, 0.10),
    Color::srgba(123.0 / 255.0, 199.0 / 255.0, 77.0 / 255.0, 0.35),
);
pub const PILL_OFFLINE: (Color, Color) = (Color::srgba(1.0, 1.0, 1.0, 0.05), hex(0x3A4148));
pub const PILL_LOST: (Color, Color) = (
    Color::srgba(224.0 / 255.0, 88.0 / 255.0, 63.0 / 255.0, 0.22),
    hex(0xE0583F),
);

/// The smallest square a pointer is asked to hit.
pub const HIT: f32 = 44.0;
/// The gap between the window's edge and what is docked at it.
pub const EDGE_GAP: f32 = 12.0;
/// The corners of a pill.
pub const PILL_RADIUS: f32 = 16.0;

/// The menu's typefaces: Reaver for titles, Radiance for the rest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Face {
    Display,
    Bold,
    Semibold,
    Regular,
}

/// A role of text: its face, size, letter spacing, and whether its digits are of one width
/// (tables, timers, scores).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Type {
    pub face: Face,
    pub size: f32,
    pub tracking: f32,
    pub figures: bool,
}

const fn role(face: Face, size: f32, tracking: f32) -> Type {
    Type {
        face,
        size,
        tracking,
        figures: false,
    }
}

/// DBOTA beside the logo.
pub const WORDMARK: Type = role(Face::Display, 24.0, 4.0);
/// A window's or a page's title.
pub const TITLE: Type = role(Face::Display, 30.0, 3.0);
/// A heading over a group of rows.
pub const HEADING: Type = role(Face::Semibold, 14.0, 2.0);
/// A row's label.
pub const LABEL: Type = role(Face::Regular, 15.0, 0.0);
/// Running text.
pub const BODY: Type = role(Face::Regular, 14.0, 0.0);
/// A small caption in capitals.
pub const CAPTION: Type = role(Face::Semibold, 12.0, 1.0);
/// A button's label, for a button of this height: PLAY's large one, the others plain.
pub const fn button(height: f32) -> Type {
    if height >= 52.0 {
        role(Face::Bold, 24.0, 3.0)
    } else if height >= 40.0 {
        role(Face::Bold, 15.0, 2.0)
    } else {
        role(Face::Bold, 13.0, 1.0)
    }
}
/// The connection pill's words.
pub const PILL: Type = role(Face::Semibold, 13.0, 1.0);
/// Numbers in a column: tabular figures.
pub const NUMBER: Type = Type {
    face: Face::Semibold,
    size: 14.0,
    tracking: 0.0,
    figures: true,
};

/// A text bundle of `role` in `color`.
pub fn text(s: impl Into<String>, fonts: &Fonts, role: Type, color: Color) -> impl Bundle {
    let features = if role.figures {
        FontFeatures::from([FontFeatureTag::new(b"tnum")])
    } else {
        FontFeatures::default()
    };
    (
        Text::new(s),
        TextFont {
            font: fonts.face(role.face).clone().into(),
            font_size: FontSize::Px(role.size),
            font_features: features,
            ..default()
        },
        LetterSpacing::Px(role.tracking),
        TextColor(color),
    )
}

/// The shadow an overlay window casts on what it covers.
pub fn window_shadow() -> BoxShadow {
    BoxShadow::new(Color::BLACK, px(0), px(24), px(0), px(70))
}

/// A gradient from `colors.0` at the top to `colors.1` at the bottom.
pub fn downward(colors: (Color, Color)) -> BackgroundGradient {
    BackgroundGradient::from(LinearGradient::to_bottom(vec![
        colors.0.into(),
        colors.1.into(),
    ]))
}

/// A gradient from `colors.0` at the left to `colors.1` at the right.
pub fn rightward(colors: (Color, Color)) -> BackgroundGradient {
    BackgroundGradient::from(LinearGradient::to_right(vec![
        colors.0.into(),
        colors.1.into(),
    ]))
}
