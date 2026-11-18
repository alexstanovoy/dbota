//! The main menu's background: Dota's dashboard background of the player's side, under the
//! pages and darkened at the sides and the foot so the panels over it read.
//!
//! Dota draws it as a small 3D scene (`maps/backgrounds/dashboard_background_crownfall_creeps_
//! radiant` / `_dire`); until dbota renders that, the menu shows the still art the install has of
//! the side's Ancient. [`Backdrop`] is where the scene takes over.

use bevy::image::{ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor};
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

/// Marks the backdrop's picture.
#[derive(Component)]
pub struct Painting;

/// The install holds the Ancient's painting at 2048 by 1024, block-compressed, which a 4K window
/// magnifies two times over: unsoftened, its compression blocks would show as a pixel grid. The
/// importer softens it once, into the cache (`soften=` of the manifest's lines), so that no
/// client does any work on it; what is left here is to sample it linearly, by name, whatever the
/// engine's default is.
fn sampled_linearly(settings: &mut ImageLoaderSettings) {
    settings.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor::linear());
}

/// The darkening over the background, left to right and top to bottom: `(alpha, at %)`, the
/// alphas as the mockups give them (see [`theme::dim`]).
const ACROSS: [(f32, f32); 4] = [(0.55, 0.0), (0.05, 38.0), (0.05, 62.0), (0.70, 100.0)];
const DOWN: [(f32, f32); 2] = [(0.10, 0.0), (0.75, 100.0)];

/// The backdrop over the whole of `root` from `top` down, under everything spawned after it.
pub fn draw(kit: &mut Kit, root: Entity, backdrop: Backdrop, top: f32) {
    let Backdrop::Still(side) = backdrop;
    let accent = match side {
        Side::Radiant => theme::RESULT_RADIANT_ROW,
        Side::Dire => theme::RESULT_DIRE_ROW,
    };
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
            // A side-coloured backdrop fills the first menu frame while its full-size art decodes.
            BackgroundGradient::from(LinearGradient::to_bottom(vec![
                ColorStop::new(accent, percent(0.0)),
                ColorStop::new(theme::GROUND, percent(100.0)),
            ])),
            Pickable::IGNORE,
            Name::new("menu background"),
            ChildOf(root),
        ))
        .id();
    let art = match side {
        Side::Radiant => "menu.background.radiant",
        Side::Dire => "menu.background.dire",
    };
    let image = match kit.manifest.file(art) {
        Some(path) => ImageNode::new(
            kit.server
                .load_builder()
                .with_settings(sampled_linearly)
                .load(path),
        ),
        None => ImageNode::default().with_color(Color::NONE),
    };
    kit.commands.spawn((
        crate::ui::fill(),
        image,
        BackgroundColor(Color::NONE),
        Pickable::IGNORE,
        ChildOf(area),
        Cover,
        Painting,
    ));
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
