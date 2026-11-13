//! What the hero pick and the strategy screens share: the original game's two screens measured in
//! their own pixels and drawn at the same shares of the window's height, the bar of seats along the
//! top, and the hero panel.
//!
//! Every length of these screens is a length measured in a reference screenshot ([`PICK`],
//! [`STRATEGY`]) and goes through [`Shot`], so the screens follow the interface's one scale rule
//! ([`crate::ui::dota`]): a measured length is the same share of the window's height at any size.
//! The measurements themselves live in [`reference`], which the render tests check the screens
//! against.

pub(crate) mod bar;
pub(crate) mod panel;
pub(crate) mod reference;

use bevy::prelude::*;

use crate::dota_text::Texts;
use crate::manifest::Manifest;
use crate::theme::{self, Face, Type};
use crate::ui::{self, Fonts};

/// A reference screenshot, by its height in its own pixels.
#[derive(Clone, Copy, Debug)]
pub struct Shot {
    height: f32,
}

impl Shot {
    pub const fn of(height: f32) -> Shot {
        Shot { height }
    }

    /// A length of the screenshot in Dota's pixels of a 1080-line screen.
    pub fn units(self, px: f32) -> f32 {
        px * 1080.0 / self.height
    }

    /// A length of the screenshot as the interface's length.
    pub fn val(self, px: f32) -> Val {
        ui::dota(self.units(px))
    }

    /// A length of the screenshot in interface pixels (a font's size).
    pub fn size(self, px: f32) -> f32 {
        self.units(px) * ui::DOTA
    }

    /// A text of `face` whose size is measured in the screenshot's pixels, with the spacing
    /// between its letters.
    pub fn text(
        self,
        kit: &Kit,
        s: impl Into<String>,
        (face, size, tracking): (Face, f32, f32),
        color: Color,
    ) -> impl Bundle {
        let role = Type {
            face,
            size: self.size(size),
            tracking: self.size(tracking),
            figures: false,
        };
        theme::text(s, kit.fonts, role, color)
    }
}

/// The hero pick screen's reference, 2868 by 1618.
pub const PICK: Shot = Shot::of(reference::PICK_HEIGHT);
/// The strategy screen's reference, 2872 by 1611.
pub const STRATEGY: Shot = Shot::of(reference::STRATEGY_HEIGHT);

/// What the parts of the screens are drawn with.
pub struct Kit<'a> {
    pub fonts: &'a Fonts,
    pub manifest: &'a Manifest,
    pub server: &'a AssetServer,
    pub texts: &'a Texts,
}

impl Kit<'_> {
    /// Dota's line of `key` from the cache, else `plain`, in capitals.
    pub fn word(&self, key: &str, plain: &str) -> String {
        self.texts
            .label(key, &[])
            .unwrap_or_else(|| plain.to_owned())
            .to_uppercase()
    }

    /// A picture of a path, or nothing drawn where there is none.
    pub fn image(&self, path: Option<String>) -> ImageNode {
        path.map_or_else(
            || ImageNode::default().with_color(Color::NONE),
            |path| ImageNode::new(self.server.load(path)),
        )
    }

    /// A picture of a manifest's logical name.
    pub fn art(&self, logical: &str) -> ImageNode {
        self.image(self.manifest.file(logical))
    }
}

/// Marks a node whose rectangle the render tests compare with the measurements the test holds, by the
/// element's name.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Spot(pub &'static str);

/// The face of the screens' big words: the hero's name, the clock.
pub const NAME_FACE: Face = Face::Display;
/// The face of their small ones.
pub const WORD_FACE: Face = Face::Semibold;

/// A tile that grows a little about its middle and takes a ring while the pointer is over it:
/// the menu's small living effect ([`crate::main_menu`]'s highlight), a transform and an outline,
/// so no neighbour moves, and it is raised over them while it is lit.
#[derive(Component)]
#[require(
    bevy::picking::hover::Hovered,
    UiTransform,
    ZIndex,
    Outline = Outline::new(px(2), px(0), Color::NONE)
)]
pub struct Lift(pub f32);

fn lift(
    time: Res<Time<Real>>,
    mut tiles: Query<(
        &Lift,
        &bevy::picking::hover::Hovered,
        &mut UiTransform,
        &mut Outline,
        &mut ZIndex,
    )>,
) {
    // The tile eases to its size, a few frames from one to the other, so it swells and settles
    // instead of jumping.
    let ease = 1.0 - (-time.delta_secs() * LIFT_RATE).exp();
    for (lift, hovered, mut transform, mut outline, mut layer) in &mut tiles {
        let (target, ring, z) = if hovered.get() {
            (lift.0, theme::FOCUS, 1)
        } else {
            (1.0, Color::NONE, i32::from(transform.scale.x > 1.001))
        };
        let now = transform.scale.x;
        let next = if (target - now).abs() < 0.002 {
            target
        } else {
            now + (target - now) * ease
        };
        if now != next {
            transform.scale = Vec2::splat(next);
        }
        if outline.color != ring {
            outline.color = ring;
        }
        layer.set_if_neq(ZIndex(z));
    }
}

/// How fast a lifted tile eases, as the rate of the exponential approach per second.
const LIFT_RATE: f32 = 22.0;

pub fn plugin(app: &mut App) {
    bar::plugin(app);
    app.add_systems(Update, lift);
}
