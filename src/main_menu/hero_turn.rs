//! The hero page's turntable: dragging the hero with the left button, a finger or a pen turns it
//! about its vertical axis, and a hero let go keeps turning a little and settles where it is.
//!
//! It stays where it was left rather than swinging back to face the player: the page is where
//! one looks a model over, and a model that moves again by itself fights the person who has just
//! stopped to look at its back.

use std::f32::consts::{PI, TAU};
use std::time::Duration;

use bevy::{
    picking::{
        events::{PointerCancel, PointerDrag, PointerDragEnd, PointerPress, PointerRelease},
        hover::Hovered,
        pointer::{PointerButton, PointerId},
    },
    prelude::*,
    window::{CursorLeft, WindowFocused},
};

use super::hero_stage::HeroStage;
use crate::cursor::{Grip, GripCursor};

/// How far a pixel of the drag turns the hero: one full turn for a drag across this many
/// interface pixels, about the height of the hero on the page, so that a point of the model
/// under the pointer keeps up with it.
const DRAG_PER_TURN: f32 = 520.0;
/// How long the speed a drag lets go with remembers the drag: a pointer held still for a moment
/// before it is let go sends the hero off slowly or not at all, a pointer let go in motion
/// sends it off at that motion's speed.
const GRIP_MEMORY: f32 = 0.06;
/// The time constant of the coast after the drag: the hero's speed falls to 1/e of itself in
/// this long. Light: a quick drag (a turn a second) carries it about a seventh of a turn on.
pub(crate) const COAST: f32 = 0.15;
/// The fastest the hero is sent off: two turns a second, whatever the flick.
const MAX_SPEED: f32 = 2.0 * TAU;
/// Below this speed (radians a second) the hero is at rest: the coast ends instead of
/// creeping for ever.
const REST_SPEED: f32 = 0.03;
/// The grab zone's measures, in interface pixels: its width (across the hero and its swing),
/// and how far it stands over the hero's head and under its feet.
const ZONE_WIDTH: f32 = 420.0;
const ZONE_OVER: f32 = 40.0;
const ZONE_UNDER: f32 = 40.0;

/// The hero's turning: its angle about the vertical axis (radians, within a half turn either
/// way), the speed it coasts at, and whether the last step was a gripped one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Turntable {
    yaw: f32,
    speed: f32,
    gripped: bool,
}

impl Turntable {
    /// One frame of `elapsed` time. A gripped frame turns the hero by what the pointer moved
    /// (`dragged`, radians) and learns the speed it is moving at; a free one coasts. The
    /// coast is the exact integral of an exponential decay, so the same drag ends at the same
    /// angle however the time was cut into frames, and a long frame cannot throw the hero.
    pub(crate) fn step(&mut self, elapsed: Duration, dragged: f32, gripped: bool) {
        let elapsed = elapsed.as_secs_f32();
        if gripped {
            if !self.gripped {
                // Taking hold of a coasting hero stops it.
                self.speed = 0.0;
            }
            self.yaw += dragged;
            if elapsed > 0.0 {
                let remembered = 1.0 - (-elapsed / GRIP_MEMORY).exp();
                self.speed += (dragged / elapsed - self.speed) * remembered;
                self.speed = self.speed.clamp(-MAX_SPEED, MAX_SPEED);
            }
        } else if self.speed != 0.0 {
            let kept = (-elapsed / COAST).exp();
            self.yaw += self.speed * COAST * (1.0 - kept);
            self.speed *= kept;
            if self.speed.abs() < REST_SPEED {
                self.speed = 0.0;
            }
        }
        self.gripped = gripped;
        self.yaw = (self.yaw + PI).rem_euclid(TAU) - PI;
    }

    /// The angle the hero faces: 0 is as the page found it, positive turns the side nearest
    /// the viewer to the viewer's right.
    pub(crate) fn yaw(&self) -> f32 {
        self.yaw
    }
}

/// What the pointer did to the zone since the last frame: the turn it asked for, and whether
/// it holds the hero.
#[derive(Resource, Default, Debug)]
pub(crate) struct TurnInput {
    /// The pointer that took hold: others are ignored until it lets go (two fingers do not
    /// pull against each other).
    holder: Option<PointerId>,
    /// Held at some point of this frame.
    held: bool,
    /// The turn the drag asked for in this frame, radians.
    dragged: f32,
}

impl TurnInput {
    fn take(&mut self, pointer: PointerId) {
        if self.holder.is_none() {
            self.holder = Some(pointer);
            self.held = true;
        }
    }

    fn drag(&mut self, pointer: PointerId, pixels: f32) {
        if self.holder == Some(pointer) {
            self.dragged += pixels / DRAG_PER_TURN * TAU;
        }
    }

    fn release(&mut self, pointer: PointerId) {
        if self.holder == Some(pointer) {
            self.holder = None;
        }
    }
}

/// The transparent area over the hero that takes the drag.
#[derive(Component)]
pub(crate) struct TurnZone;

/// Puts the zone over the hero standing with its feet `feet` pixels down the page and its middle
/// `offset` pixels from `share` per cent of the page's width: over the page's body (which scrolls
/// and would take the press itself) and under the corners and everything that opens over the
/// page, so that a control above the hero keeps its clicks and the zone takes only what nothing
/// else does.
pub(super) fn zone(
    commands: &mut Commands,
    root: Entity,
    (share, offset): (f32, f32),
    (feet, height): (f32, f32),
) {
    commands
        .spawn((
            TurnZone,
            Hovered::default(),
            Node {
                position_type: PositionType::Absolute,
                left: percent(share),
                margin: UiRect::left(px(offset - ZONE_WIDTH / 2.0)),
                top: px(feet - height - ZONE_OVER),
                width: px(ZONE_WIDTH),
                height: px(height + ZONE_OVER + ZONE_UNDER),
                ..default()
            },
            Name::new("hero turn zone"),
            ChildOf(root),
        ))
        .observe(pressed)
        .observe(dragged)
        .observe(released);
}

fn pressed(event: On<PointerPress>, mut input: ResMut<TurnInput>) {
    if event.button == PointerButton::Primary {
        input.take(event.pointer.id);
    }
}

fn dragged(event: On<PointerDrag>, scale: Res<UiScale>, mut input: ResMut<TurnInput>) {
    if event.button == PointerButton::Primary {
        input.drag(event.pointer.id, event.delta.x / scale.0);
    }
}

fn released(event: On<PointerDragEnd>, mut input: ResMut<TurnInput>) {
    if event.button == PointerButton::Primary {
        input.release(event.pointer.id);
    }
}

/// A pointer taken away (the window lost it, a touch was cancelled).
fn cancelled(event: On<PointerCancel>, mut input: ResMut<TurnInput>) {
    input.release(event.pointer.id);
}

/// Whether the turntable has anything to do: the hero page is shown, or a hold or a hand cursor
/// of a page that has just gone is still to be let go of.
pub(crate) fn wanted(
    zones: Query<(), With<TurnZone>>,
    input: Res<TurnInput>,
    grip: Res<GripCursor>,
) -> bool {
    !zones.is_empty() || input.holder.is_some() || grip.0.is_some()
}

/// Lets go of what the window has lost: a button let go outside the window or the page's canvas
/// is never told to the client (a browser tells the canvas only what happens over it, and the
/// system tells a window that lost focus nothing more), so a pointer that left the window, or a
/// window that lost the pointer's focus, no longer holds the hero.
fn let_go_of_the_lost(
    mut input: ResMut<TurnInput>,
    mut left: MessageReader<CursorLeft>,
    mut focused: MessageReader<WindowFocused>,
) {
    if left.read().count() > 0 {
        input.release(PointerId::Mouse);
    }
    if focused.read().any(|event| !event.focused)
        && let Some(holder) = input.holder
    {
        input.release(holder);
    }
}

/// Steps the hero's turntable by the frame's time and what the pointer did, and turns the model
/// on the stage.
fn turn(
    time: Res<Time<Real>>,
    mut input: ResMut<TurnInput>,
    mut stage: ResMut<HeroStage>,
    mut pivots: Query<&mut Transform>,
    zones: Query<(), With<TurnZone>>,
) {
    // A page rebuilt or left under a drag takes the zone, and its release, with it.
    if zones.is_empty() {
        *input = TurnInput::default();
    }
    // Gripped for the whole frame that held it at any point, a press that came after a release
    // of the same frame included; the next frame is held only while a pointer still holds.
    let (dragged, gripped) = (std::mem::take(&mut input.dragged), input.held);
    input.held = input.holder.is_some();
    let Some(pivot) = stage.actor.as_ref().map(|actor| actor.pivot) else {
        return;
    };
    let before = stage.turntable.yaw();
    stage.turntable.step(time.delta(), dragged, gripped);
    if stage.turntable.yaw() != before
        && let Ok(mut at) = pivots.get_mut(pivot)
    {
        at.rotation = Quat::from_rotation_y(stage.turntable.yaw());
    }
}

/// The cursor over the zone: an open hand over it, a closed one while it holds the hero.
fn show_grip(
    input: Res<TurnInput>,
    zones: Query<&Hovered, With<TurnZone>>,
    mut grip: ResMut<GripCursor>,
) {
    let wanted = if input.holder.is_some() {
        Some(Grip::Closed)
    } else if zones.iter().any(Hovered::get) {
        Some(Grip::Open)
    } else {
        None
    };
    if grip.0 != wanted {
        grip.0 = wanted;
    }
}

pub(super) fn plugin(app: &mut App) {
    app.init_resource::<TurnInput>()
        .add_observer(let_go_anywhere)
        .add_observer(cancelled)
        .add_systems(
            Update,
            (let_go_of_the_lost, turn, show_grip)
                .chain()
                .run_if(wanted)
                .after(super::screens::draw)
                .in_set(TurnSet),
        );
}

/// The turning, for what must come after it (the stage renders the turned model).
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct TurnSet;

/// A press let go anywhere, a click that was never a drag included, is the hold let go.
fn let_go_anywhere(event: On<PointerRelease>, mut input: ResMut<TurnInput>) {
    if event.button == PointerButton::Primary {
        input.release(event.pointer.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<TurnInput>();
        world.init_resource::<GripCursor>();
        world
    }

    fn is_wanted(world: &mut World) -> bool {
        world.run_system_once(wanted).expect("the condition runs")
    }

    #[test]
    fn the_turntable_is_idle_while_the_hero_page_is_not_shown() {
        assert!(!is_wanted(&mut world()));
    }

    #[test]
    fn the_turntable_runs_while_the_hero_page_has_its_zone() {
        let mut world = world();
        world.spawn(TurnZone);
        assert!(is_wanted(&mut world));
    }

    #[test]
    fn the_turntable_runs_once_more_to_let_go_of_a_hold_the_page_left_behind() {
        let mut world = world();
        world.resource_mut::<TurnInput>().holder = Some(PointerId::Mouse);
        assert!(is_wanted(&mut world));
    }

    #[test]
    fn the_turntable_runs_once_more_to_take_off_a_hand_the_page_left_behind() {
        let mut world = world();
        world.resource_mut::<GripCursor>().0 = Some(Grip::Open);
        assert!(is_wanted(&mut world));
    }
}
