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

/// A field to pick from, one to type in, and their edge.
pub const FIELD: Color = hex(0x222629);
pub const FIELD_SUNKEN: Color = hex(0x0B0C0E);
pub const FIELD_EDGE: Color = hex(0x495255);

/// A card of rows on a window's ground, its edge and the rule under its title; the rule between rows.
pub const CARD: Color = hex(0x15181B);
pub const CARD_EDGE: Color = hex(0x2A2E32);
pub const CARD_RULE: Color = hex(0x2D3236);
pub const ROW_RULE: Color = Color::srgba(45.0 / 255.0, 50.0 / 255.0, 54.0 / 255.0, 0.6);
/// A field under the cursor: its ground and edge.
pub const FIELD_HOVER: (Color, Color) = (hex(0x2A2F33), hex(0x6B747D));
/// A field that cannot be used now: its ground and edge.
pub const FIELD_OFF: (Color, Color) = (hex(0x191C1F), hex(0x30363B));
/// A box ticked: its ground and edge; unticked, only the edge.
pub const TICK: (Color, Color) = (hex(0x3D7F46), hex(0x6FBF73));
pub const TICK_EDGE_OFF: Color = hex(0x8B939C);
/// A slider's rail, and what it fills.
pub const RAIL: Color = hex(0x3A4148);
/// An open list: its ground, its picked option and the option under the cursor.
pub const LIST: Color = hex(0x161B20);
pub const LIST_PICKED: Color = hex(0x1D252C);
pub const LIST_HOVER: Color = hex(0x2A3A47);
/// Small text of the lowest rank: counters, keys of an unbound slot.
pub const INK_6: Color = hex(0x6B747D);
/// The ground of a tooltip's neighbour, a text field, and an unbound key's.
pub const FIELD_TYPED: Color = hex(0x0F1215);
pub const KEY_UNBOUND: (Color, Color) = (hex(0x1E2124), hex(0x3A4148));
/// A key field being set, and one that clashes (ground, edge, ink).
pub const KEY_SETTING: (Color, Color, Color) = (
    Color::srgba(89.0 / 255.0, 176.0 / 255.0, 210.0 / 255.0, 0.16),
    FOCUS,
    SELECT,
);
pub const KEY_CLASH: (Color, Color, Color) = (hex(0x2A1714), hex(0xA3372D), ERROR_TEXT);

/// The links under an open tab that are not the one in view.
pub const NAV_RULE: Color = hex(0x2A3036);
/// A scroll bar's rail and thumb.
pub const SCROLL_RAIL: Color = hex(0x24292D);
pub const SCROLL_THUMB: Color = hex(0x5A6670);
/// A row's label.
pub const ROW_INK: Color = hex(0xE8EBEE);

/// A tooltip or an open list, and its edge.
pub const TOOLTIP: Color = hex(0x0E1216);
pub const TOOLTIP_EDGE: Color = hex(0x3A4650);

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

/// A link, at rest and under the cursor.
pub const LINK: (Color, Color) = (hex(0x59B0D2), hex(0x8FCDE6));

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
    if height >= 64.0 {
        role(Face::Bold, 32.0, 4.0)
    } else if height >= 52.0 {
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
/// A running clock (the search's time).
pub const CLOCK: Type = Type {
    size: 22.0,
    ..NUMBER
};
/// The compact finding board's running clock.
pub const FINDING_CLOCK: Type = Type {
    size: 17.0,
    ..CLOCK
};
/// The Play drawer’s section heading.
pub const PLAY_SECTION: Type = role(Face::Semibold, 18.0, 2.0);
/// A selectable queue’s name.
pub const QUEUE_NAME: Type = role(Face::Bold, 16.0, 1.0);
/// The question mark in a help control.
pub const HELP_GLYPH: Type = role(Face::Regular, 10.0, 0.0);
/// Compact running text: a tooltip, sub-line or link.
pub const SMALL_BODY: Type = role(Face::Regular, 13.0, 0.0);
/// The mode beneath the accept heading.
pub const LARGE_BODY: Type = role(Face::Regular, 16.0, 0.0);
/// A player’s connection status in loading.
pub const STATUS_TEXT: Type = role(Face::Regular, 12.0, 0.0);
/// A loading roster’s team heading.
pub const LOADING_TEAM: Type = role(Face::Semibold, 17.0, 2.0);
/// The loading sidebar’s player count.
pub const LOADING_SUMMARY: Type = role(Face::Semibold, 16.0, 1.0);
/// The match offer’s heading.
pub const OFFER_TITLE: Type = role(Face::Bold, 32.0, 3.0);
/// The finding board’s title.
pub const FINDING_TITLE: Type = role(Face::Regular, 20.0, 1.5);
/// The finding board’s cancel cross.
pub const CLOSE_GLYPH: Type = role(Face::Regular, 24.0, 0.0);
/// Compact tabular numbers, such as a queue’s team sizes.
pub const SMALL_NUMBER: Type = Type {
    size: 13.0,
    ..NUMBER
};
/// The top bar's tabs while today's pages are more than its two tabs of the design: smaller.
pub const INTERIM_TAB: Type = role(Face::Semibold, 15.0, 1.5);
/// The pill's service name, and its RECONNECTING.
pub const PILL_NAME: Type = role(Face::Regular, 13.0, 0.0);
pub const PILL_ALERT: Type = role(Face::Bold, 13.0, 0.0);
/// The player's card: the initial in its square, and the name.
pub const INITIAL: Type = role(Face::Bold, 20.0, 0.0);
pub const PLAYER_NAME: Type = role(Face::Semibold, 17.0, 0.0);
/// DBOTA over the way in's card, and the card's title.
pub const WAY_IN_WORDMARK: Type = role(Face::Display, 56.0, 12.0);
pub const CARD_TITLE: Type = role(Face::Display, 24.0, 2.0);
/// A link in running text.
pub const LINK_TEXT: Type = role(Face::Semibold, 14.0, 0.0);

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

/// Play's drawer, open section header, queue body and idle row.
pub const PLAY_PANEL: (Color, Color) = (hex(0x1C2229), hex(0x020304));
pub const PLAY_HEADING: (Color, Color) = (hex(0x525E64), hex(0x3A4348));
pub const PLAY_BODY: Color = hex(0x111519);
pub const PLAY_ROW: Color = hex(0x1A1F24);
pub const PLAY_SELECTION: (Color, Color) = (hex(0x1E3E70), hex(0x142035));
/// The flat finding band, the accept window and its unfilled player squares.
pub const FINDING: Color = hex(0x10181F);
pub const ACCEPT_PANEL: (Color, Color) = (hex(0x161B21), GROUND);
pub const ACCEPT_EMPTY: Color = hex(0x141A20);
/// Time to accept moves from green to yellow as the deadline approaches.
pub const ACCEPT_TIME: (Color, Color) = (RADIANT, GOLD);
/// A quiet decline link.
pub const DECLINE: Color = hex(0xC9705F);
/// The loading sidebar and its local progress bar.
pub const LOADING_PANEL: Color = hex(0x080A0C);
pub const LOADING_TRACK: Color = hex(0x222629);

/// Loading rows are opaque sRGB blends over their dark sidebar.
pub const LOADING_SELF: Color = hex(0x14252C);
pub const LOADING_ROW: Color = hex(0x0F1113);

/// Shared dashboard dock geometry, in interface pixels.
pub const CORNER_WIDTH: f32 = 300.0;
pub const CORNER_RIGHT: f32 = 48.0;
pub const CORNER_BOTTOM: f32 = 18.0;
pub const CORNER_GAP: f32 = 8.0;
pub const ACTION_HEIGHT: f32 = 52.0;
pub const PLAY_DRAWER_WIDTH: f32 = 380.0;
pub const FINDING_HEIGHT: f32 = 56.0;

/// Disabled queue admission action, kept readable with its reason strip.
pub const QUEUE_DISABLED: (Color, Color) = (hex(0x3A3F44), hex(0x24282C));
