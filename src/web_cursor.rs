//! The cursor kept in the game in a browser, as the desktop confines it to
//! the window (`window.rs`, [`crate::window::keeps_cursor`]): while a match
//! shows, the page has focus and the settings overlay is closed. A page
//! cannot confine the pointer, only lock it (the Pointer Lock API). A locked
//! pointer has no place, and the platform draws no cursor: the page moves
//! the game's cursor by the mouse's motion, held inside the canvas so the
//! edge pan works on every edge, and hands it to the client as the
//! pointer's own (`web/pointer.js`); the game draws it in the shape the
//! window wears ([`crate::cursor`]).
//!
//! A browser locks only on a gesture and lets go on Esc (and when the page
//! loses focus), so the page asks for the lock itself, inside the gestures
//! (`web/pointer.js`): when it is told the game keeps the cursor (in the
//! frame after the click or key that brought a match on screen or closed
//! the settings), in a press on the canvas, and in F11 before it asks for
//! fullscreen (which uses the gesture up). After an Esc the next press on the
//! canvas takes the cursor again, where the pointer is.

use crate::session::Session;
use bevy::{
    prelude::*,
    ui::FocusPolicy,
    window::{CursorIcon, CustomCursor, PrimaryWindow},
};

/// Logical pixels a side of a cursor image (Dota's are 32 by 32).
const CURSOR_SIDE: f32 = 32.0;
/// Logical pixels a side of the marker drawn where the install's cursors are missing.
const MARKER_SIDE: f32 = 8.0;

pub fn plugin(app: &mut App) {
    app.init_non_send::<Lock>()
        .add_systems(Update, keep)
        // After the shape is picked (`cursor`, in Update), before the layout.
        .add_systems(PostUpdate, draw.before(bevy::ui::UiSystems::Prepare));
}

/// What the lock remembers between frames.
#[derive(Default)]
struct Lock {
    /// Whether the page was last told the game keeps the cursor.
    told: Option<bool>,
    was_locked: bool,
}

/// The cursor drawn while the pointer is locked.
#[derive(Component)]
struct Drawn;

fn document() -> Option<web_sys::Document> {
    web_sys::window().and_then(|w| w.document())
}

fn canvas(document: &web_sys::Document) -> Option<web_sys::Element> {
    document.get_element_by_id("dbota")
}

fn locked(document: &web_sys::Document) -> bool {
    match (document.pointer_lock_element(), canvas(document)) {
        (Some(held), Some(canvas)) => held == canvas,
        _ => false,
    }
}

/// Tells the page whether the game keeps the cursor: the page locks the
/// pointer then (in the gesture it is asked in: a press on the canvas, F11,
/// or the one that got the game here) and lets it go otherwise.
fn keep(
    window: Single<&Window, With<PrimaryWindow>>,
    session: NonSend<Session>,
    screen: Res<crate::settings::Screen>,
    args: Res<crate::Args>,
    mut lock: NonSendMut<Lock>,
) {
    let Some(document) = document() else {
        return;
    };
    let want = crate::window::keeps_cursor(&window, &screen, &args, &session);
    if lock.told != Some(want) {
        lock.told = Some(want);
        crate::webpage::keep_cursor(want);
    }
    let is_locked = locked(&document);
    if is_locked != lock.was_locked {
        let now = if is_locked {
            "locked to the canvas"
        } else {
            "released"
        };
        let at = window.cursor_position().unwrap_or_default();
        info!("cursor {now} at {:.0},{:.0}", at.x, at.y);
    }
    lock.was_locked = is_locked;
}

/// Draws the game's cursor while the pointer is locked: the image the
/// window wears, its hotspot on the cursor; a small white square where the
/// window wears one of the platform's (no install).
fn draw(
    mut commands: Commands,
    window: Single<(&Window, Option<&CursorIcon>), With<PrimaryWindow>>,
    ui_scale: Res<UiScale>,
    mut drawn: Query<(&mut Node, &mut UiTransform, &mut ImageNode, &mut Visibility), With<Drawn>>,
) {
    let Ok((mut node, mut place, mut image, mut visibility)) = drawn.single_mut() else {
        commands.spawn((
            Drawn,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                ..default()
            },
            UiTransform::default(),
            ImageNode::default(),
            GlobalZIndex(i32::MAX),
            Pickable::IGNORE,
            FocusPolicy::Pass,
            Visibility::Hidden,
        ));
        return;
    };
    let (window, icon) = *window;
    let Some(at) = window
        .cursor_position()
        .filter(|_| document().is_some_and(|d| locked(&d)))
    else {
        visibility.set_if_neq(Visibility::Hidden);
        return;
    };
    let (handle, hotspot) = match icon {
        Some(CursorIcon::Custom(CustomCursor::Image(cursor))) => (
            cursor.handle.clone(),
            Vec2::new(f32::from(cursor.hotspot.0), f32::from(cursor.hotspot.1)),
        ),
        _ => (Handle::default(), Vec2::ZERO),
    };
    let stand_in = handle == Handle::default();
    let side = if stand_in { MARKER_SIDE } else { CURSOR_SIDE };
    // Interface pixels are scaled by the interface's scale; the cursor is
    // not. It moves by its transform, which takes no new layout.
    let k = ui_scale.0.max(f32::EPSILON);
    let corner = (at - hotspot) / k;
    place.set_if_neq(UiTransform::from_translation(Val2::px(corner.x, corner.y)));
    let side = Val::Px(side / k);
    if node.width != side || node.height != side {
        node.width = side;
        node.height = side;
    }
    // The default image is a white texel: the stand-in's marker.
    if image.image != handle {
        image.image = handle;
    }
    visibility.set_if_neq(Visibility::Inherited);
}
