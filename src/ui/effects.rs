//! Lightweight, device-independent feedback shared by menu controls and panels.
use super::{WidgetBuild, WidgetPaint};
use bevy::{picking::hover::Hovered, prelude::*, ui::Val2};

#[derive(Component, Default, Clone, Copy, PartialEq)]
pub(crate) struct Motion {
    pub hover: f32,
    pub pressed: f32,
    pulse: f32,
}
#[derive(Resource)]
pub(crate) struct Activations(Vec<Entity>);
impl Default for Activations {
    fn default() -> Self {
        Self(Vec::with_capacity(64))
    }
}
impl Activations {
    pub fn record(&mut self, entity: Entity) {
        if self.0.len() < 64 && !self.0.contains(&entity) {
            self.0.push(entity);
        }
    }
}

#[derive(Component)]
#[require(UiTransform)]
pub(crate) struct PanelMotion {
    pub open: bool,
    progress: f32,
    offset: Vec2,
}
impl PanelMotion {
    pub fn hidden(offset: Vec2) -> Self {
        Self {
            open: false,
            progress: 0.0,
            offset,
        }
    }
}

pub(crate) fn amount(value: f32) -> f32 {
    value * value * (3.0 - 2.0 * value)
}
pub(crate) fn tint(from: Color, to: Color, weight: f32) -> Color {
    if weight <= 0.0 {
        return from;
    }
    if weight >= 1.0 {
        return to;
    }
    let (a, b) = (from.to_linear(), to.to_linear());
    Color::LinearRgba(LinearRgba::new(
        a.red + (b.red - a.red) * weight,
        a.green + (b.green - a.green) * weight,
        a.blue + (b.blue - a.blue) * weight,
        a.alpha + (b.alpha - a.alpha) * weight,
    ))
}
fn approach(value: f32, target: bool, step: f32) -> f32 {
    if target {
        (value + step).min(1.0)
    } else {
        (value - step).max(0.0)
    }
}
fn focus_source(
    keys: Res<ButtonInput<KeyCode>>,
    pads: Query<&Gamepad>,
    mut visible: ResMut<bevy::input_focus::InputFocusVisible>,
) {
    // PointerFocusPlugin clears this on pointer input; mark explicit navigation before picking.
    if keys.any_just_pressed([
        KeyCode::Tab,
        KeyCode::Enter,
        KeyCode::NumpadEnter,
        KeyCode::Space,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
    ]) || pads.iter().take(8).any(|pad| {
        use bevy::input::gamepad::GamepadButton;
        pad.just_pressed(GamepadButton::DPadDown)
            || pad.just_pressed(GamepadButton::DPadUp)
            || pad.just_pressed(GamepadButton::DPadLeft)
            || pad.just_pressed(GamepadButton::DPadRight)
            || pad.just_pressed(GamepadButton::South)
    }) {
        visible.0 = true;
    }
}
fn activated(
    event: On<bevy::ui_widgets::Activate>,
    controls: Query<(), With<Motion>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut requests: ResMut<Activations>,
) {
    // Native Button activation also reports secondary pointer buttons.
    if mouse
        .get_just_pressed()
        .chain(mouse.get_just_released())
        .any(|button| *button != MouseButton::Left)
    {
        return;
    }
    if controls.contains(event.entity) {
        requests.record(event.entity);
    }
}
fn advance(
    time: Res<Time<Real>>,
    focus: Res<bevy::input_focus::InputFocus>,
    focus_visible: Res<bevy::input_focus::InputFocusVisible>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut requests: ResMut<Activations>,
    mut controls: Query<(
        Entity,
        &Hovered,
        Has<bevy::ui::Pressed>,
        &mut Motion,
        Has<bevy::ui::InteractionDisabled>,
        Has<crate::ui::Disabled>,
    )>,
) {
    for entity in requests.0.drain(..) {
        if let Ok((_, _, _, mut motion, false, false)) = controls.get_mut(entity) {
            motion.pulse = 0.09;
        }
    }
    let dt = time.delta_secs().min(0.1);
    for (entity, hover, pressed, mut motion, disabled, unavailable) in &mut controls {
        let enabled = !disabled && !unavailable;
        let focused = focus_visible.0 && focus.get() == Some(entity);
        let pointer_pressed = pressed && mouse.pressed(MouseButton::Left);
        let next = Motion {
            hover: approach(motion.hover, enabled && (hover.get() || focused), dt / 0.12),
            pressed: approach(
                motion.pressed,
                enabled && (pointer_pressed || motion.pulse > 0.0),
                dt / 0.06,
            ),
            pulse: (motion.pulse - dt).max(0.0),
        };
        motion.set_if_neq(next);
    }
}
fn panels(
    time: Res<Time<Real>>,
    mut panels: Query<(
        &mut PanelMotion,
        &mut UiTransform,
        &mut Node,
        &mut Visibility,
    )>,
) {
    for (mut panel, mut transform, mut node, mut visible) in &mut panels {
        panel.progress = approach(
            panel.progress,
            panel.open,
            time.delta_secs().min(0.1) / 0.10,
        );
        let shown = panel.progress > 0.0;
        visible.set_if_neq(if shown {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        let display = if shown { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
        let shift = panel.offset * (1.0 - amount(panel.progress));
        transform.set_if_neq(UiTransform::from_translation(Val2::px(shift.x, shift.y)));
    }
}
pub(super) fn plugin(app: &mut App) {
    app.init_resource::<Activations>()
        .init_resource::<bevy::input_focus::InputFocusVisible>()
        .add_observer(activated)
        .add_systems(
            PreUpdate,
            focus_source
                .after(bevy::input::InputSystems)
                .before(bevy::picking::PickingSystems::Hover),
        )
        .add_systems(Update, advance.after(WidgetBuild).before(WidgetPaint))
        .add_systems(Update, panels.after(WidgetPaint));
}
