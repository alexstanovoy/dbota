//! The main menu's background: Dota's dashboard background of the player's side, under the
//! pages and darkened at the sides and the foot so the panels over it read.
//!
//! Dota draws it as a small 3D scene (`maps/backgrounds/dashboard_background_crownfall_creeps_
//! radiant` / `_dire`); until dbota renders that, the menu shows the still art the install has of
//! the side's Ancient. [`Backdrop`] is where the scene takes over.

use bevy::prelude::*;

use super::kit::Kit;
use super::view::Side;
use crate::theme;
use crate::ui::Cover;

/// What stands behind the menu's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backdrop {
    /// The install's still art of the side's Ancient.
    Still(Side),
}

impl Backdrop {
    /// The background of the side the player last played.
    pub fn of(side: Side) -> Backdrop {
        Backdrop::Still(side)
    }
}

/// The darkening over the background, left to right and top to bottom: `(alpha, at %)`, the
/// alphas as the mockups give them (see [`theme::dim`]).
const ACROSS: [(f32, f32); 4] = [(0.55, 0.0), (0.05, 38.0), (0.05, 62.0), (0.70, 100.0)];
const DOWN: [(f32, f32); 2] = [(0.10, 0.0), (0.75, 100.0)];

/// The backdrop over the whole of `root` from `top` down, under everything spawned after it.
pub fn draw(kit: &mut Kit, root: Entity, backdrop: Backdrop, top: f32) {
    let area = kit
        .commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(top),
                bottom: px(0),
                ..default()
            },
            BackgroundColor(theme::GROUND),
            Pickable::IGNORE,
            Name::new("menu background"),
            ChildOf(root),
        ))
        .id();
    match backdrop {
        Backdrop::Still(side) => {
            let art = match side {
                Side::Radiant => "menu.background.radiant",
                Side::Dire => "menu.background.dire",
            };
            let picture = kit.art(area, art, crate::ui::fill());
            kit.commands.entity(picture).insert(Cover);
        }
    }
    for (stops, across) in [(&ACROSS[..], true), (&DOWN[..], false)] {
        let stops = stops
            .iter()
            .map(|(alpha, at)| {
                ColorStop::new(theme::GROUND.with_alpha(theme::dim(*alpha)), percent(*at))
            })
            .collect();
        let gradient = if across {
            LinearGradient::to_right(stops)
        } else {
            LinearGradient::to_bottom(stops)
        };
        kit.commands.spawn((
            crate::ui::fill(),
            BackgroundGradient::from(gradient),
            Pickable::IGNORE,
            ChildOf(area),
        ));
    }
}
