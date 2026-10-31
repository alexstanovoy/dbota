//! The mouse cursor, Dota's own: the arrow, the arrow that turns red over an
//! enemy and green over a friend, the attack arrow, the reticle of an ability
//! or an item taken up for aiming (white on the ground, tinted over an enemy
//! or a friend), and the reticle an item dragged out into the world wears.
//! The images are the install's (`resource/cursor/source`); until one has loaded the window's own
//! arrow, crosshair and hand are worn.

use bevy::{
    prelude::*,
    window::{CursorIcon, CustomCursor, CustomCursorImage, PrimaryWindow, SystemCursorIcon},
};
use bota_client_core::Source;
use bota_proto::UnitKind;

use crate::input::OverHud;
use crate::manifest::Manifest;
use crate::pick::{Hover, Left};
use crate::session::Session;

/// A cursor of Dota's: its manifest name, and the pixel of its 32 by 32 image
/// that points (an arrow's tip is its corner, a reticle's centre the 14, 14
/// of Dota's `readme.txt`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Shape {
    Default,
    DefaultEnemy,
    DefaultTeam,
    AttackDefault,
    AttackEnemy,
    AttackTeam,
    SpellDefault,
    SpellEnemy,
    SpellTeam,
    ItemDrop,
}

impl Shape {
    const ALL: [Shape; 10] = [
        Shape::Default,
        Shape::DefaultEnemy,
        Shape::DefaultTeam,
        Shape::AttackDefault,
        Shape::AttackEnemy,
        Shape::AttackTeam,
        Shape::SpellDefault,
        Shape::SpellEnemy,
        Shape::SpellTeam,
        Shape::ItemDrop,
    ];

    fn logical(self) -> &'static str {
        match self {
            Shape::Default => "cursor.default",
            Shape::DefaultEnemy => "cursor.default_enemy",
            Shape::DefaultTeam => "cursor.default_team",
            Shape::AttackDefault => "cursor.attack_default",
            Shape::AttackEnemy => "cursor.attack_enemy",
            Shape::AttackTeam => "cursor.attack_team",
            Shape::SpellDefault => "cursor.spell_default",
            Shape::SpellEnemy => "cursor.spell_enemy",
            Shape::SpellTeam => "cursor.spell_team",
            Shape::ItemDrop => "cursor.item_drop",
        }
    }

    fn hotspot(self) -> (u16, u16) {
        match self {
            Shape::SpellDefault | Shape::SpellEnemy | Shape::SpellTeam | Shape::ItemDrop => {
                (14, 14)
            }
            _ => (0, 0),
        }
    }

    /// The window's own cursor worn until the image has loaded.
    fn system_icon(self) -> SystemCursorIcon {
        match self {
            Shape::Default => SystemCursorIcon::Default,
            Shape::DefaultTeam => SystemCursorIcon::Pointer,
            Shape::DefaultEnemy | Shape::AttackDefault | Shape::AttackEnemy => {
                SystemCursorIcon::Crosshair
            }
            Shape::AttackTeam => SystemCursorIcon::Pointer,
            _ => SystemCursorIcon::Crosshair,
        }
    }
}

/// The loaded images of the shapes, and the shape the window wears.
#[derive(Resource, Default)]
pub struct Cursors {
    images: Vec<(Shape, Handle<Image>)>,
    /// The same images lightened, worn while an order sent has not been received (Dota's
    /// `dota_debug_change_cursor_during_pending_order`: the cursor is brighter until the server
    /// confirms the command).
    bright: Vec<(Shape, Handle<Image>)>,
    /// The shape worn, whether it is the install's image, and whether it is the lightened one.
    shown: Option<(Shape, bool, bool)>,
}

impl Cursors {
    /// Whether the cursor worn is the one of an order still on its way.
    pub fn is_bright(&self) -> bool {
        self.shown.is_some_and(|(_, _, bright)| bright)
    }
}

/// An image with each colour moved halfway to white, its transparency kept; none for a format
/// that is not four bytes a pixel.
fn lightened(image: &Image) -> Option<Image> {
    use bevy::render::render_resource::TextureFormat;
    if !matches!(
        image.texture_descriptor.format,
        TextureFormat::Rgba8Unorm | TextureFormat::Rgba8UnormSrgb
    ) {
        return None;
    }
    let mut copy = image.clone();
    let (pixels, _) = copy.data.as_mut()?.as_chunks_mut::<4>();
    for pixel in pixels {
        for channel in &mut pixel[..3] {
            *channel += (255 - *channel) / 2;
        }
    }
    Some(copy)
}

/// Makes the lightened copy of each cursor image once it has loaded.
fn lighten(mut cursors: ResMut<Cursors>, mut images: ResMut<Assets<Image>>) {
    if cursors.bright.len() == cursors.images.len() {
        return;
    }
    let loaded: Vec<(Shape, Image)> = cursors
        .images
        .iter()
        .filter(|(shape, _)| !cursors.bright.iter().any(|(done, _)| done == shape))
        .filter_map(|(shape, handle)| Some((*shape, images.get(handle)?.clone())))
        .collect();
    for (shape, image) in loaded {
        // A format that cannot be lightened wears the plain image.
        let handle = match lightened(&image) {
            Some(copy) => images.add(copy),
            None => cursors
                .images
                .iter()
                .find(|(s, _)| *s == shape)
                .map(|(_, h)| h.clone())
                .unwrap_or_default(),
        };
        cursors.bright.push((shape, handle));
    }
}

pub fn plugin(app: &mut App) {
    app.init_resource::<Cursors>()
        .add_systems(Startup, load)
        .add_systems(Update, (lighten, show.after(crate::pick::update)));
}

fn load(mut cursors: ResMut<Cursors>, manifest: Res<Manifest>, server: Res<AssetServer>) {
    cursors.images = Shape::ALL
        .into_iter()
        .filter_map(|shape| Some((shape, server.load(manifest.file(shape.logical())?))))
        .collect();
}

/// Whether the unit is one a click on it works against: not the fountain.
fn enemy_of(core: &bota_client_core::App, id: bota_proto::EntityId) -> Option<bool> {
    let unit = core.view()?.unit(id)?;
    if unit.kind == UnitKind::Fountain {
        return None;
    }
    Some(Some(unit.team) != core.my_team())
}

/// The shape the cursor has now, by what a click would do.
fn shape(core: &bota_client_core::App, hover: &Hover, over_hud: bool) -> Shape {
    if core.held_item.is_some() && !over_hud {
        return Shape::ItemDrop;
    }
    if over_hud {
        return Shape::Default;
    }
    let relation = hover.left().and_then(|(id, _)| enemy_of(core, id));
    let live = core.my_slot.is_some() && matches!(core.source, Source::Live(_));
    match (hover.left, relation) {
        (Left::Aim, Some(true)) => Shape::SpellEnemy,
        (Left::Aim, Some(false)) => Shape::SpellTeam,
        (Left::Aim, None) => Shape::SpellDefault,
        (Left::Attack, Some(true)) => Shape::AttackEnemy,
        (Left::Attack, Some(false)) => Shape::AttackTeam,
        (Left::Attack, None) => Shape::AttackDefault,
        (Left::Select, Some(true)) if live => Shape::DefaultEnemy,
        (Left::Select, Some(false)) if live => Shape::DefaultTeam,
        (Left::Select, _) => Shape::Default,
    }
}

fn show(
    session: NonSend<Session>,
    hover: Res<Hover>,
    over: Res<OverHud>,
    window: Single<Entity, With<PrimaryWindow>>,
    mut cursors: ResMut<Cursors>,
    images: Res<Assets<Image>>,
    mut commands: Commands,
) {
    let wanted = shape(&session.0, &hover, over.0);
    let bright = session.0.source.pending_orders() > 0;
    let image = if bright {
        &cursors.bright
    } else {
        &cursors.images
    }
    .iter()
    .find(|(s, _)| *s == wanted)
    .map(|(_, handle)| handle.clone());
    // Until the image has loaded the window's own cursor is worn, and the cursor is set again then.
    let ready = image.as_ref().is_some_and(|h| images.contains(h));
    if cursors.shown == Some((wanted, ready, bright)) {
        return;
    }
    cursors.shown = Some((wanted, ready, bright));
    let icon = match image.filter(|_| ready) {
        Some(handle) => CursorIcon::Custom(CustomCursor::Image(CustomCursorImage {
            handle,
            hotspot: wanted.hotspot(),
            ..default()
        })),
        None => CursorIcon::System(wanted.system_icon()),
    };
    commands.entity(*window).insert(icon);
}
