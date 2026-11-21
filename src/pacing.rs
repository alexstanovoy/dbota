//! Let winit sleep between changes on screens without an animated world.

use std::time::Duration;

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
    text::{FontSource, TextLayoutInfo, TextSpan},
    ui::{
        BackgroundGradient, BorderGradient, CalculatedClip, ComputedNode, ComputedStackIndex,
        UiGlobalTransform,
    },
    window::{RequestRedraw, WindowEvent},
    winit::{UpdateMode, WinitSettings},
};

#[cfg(all(test, any(target_os = "linux", target_os = "windows")))]
mod tests;
mod timer;
mod wake;

pub(crate) use wake::{poll, waker};

#[derive(Resource, Default)]
struct HeldCameras(Vec<Entity>);

pub fn plugin(app: &mut App) {
    wake::install(app);
    let idle = UpdateMode::reactive_low_power(Duration::MAX);
    app.insert_resource(WinitSettings {
        focused_mode: idle,
        unfocused_mode: idle,
    })
    .insert_non_send(timer::Timer::new())
    .init_resource::<HeldCameras>()
    .add_systems(First, restore)
    .add_systems(Last, pace);
}

fn restore(mut held: ResMut<HeldCameras>, mut cameras: Query<&mut Camera>) {
    for entity in held.0.drain(..) {
        if let Ok(mut camera) = cameras.get_mut(entity) {
            camera.is_active = true;
        }
    }
}

#[derive(SystemParam)]
#[allow(clippy::type_complexity)]
struct Interface<'w, 's> {
    layout: Query<
        'w,
        's,
        &'static InheritedVisibility,
        Or<(
            Changed<ComputedNode>,
            Changed<UiGlobalTransform>,
            Changed<CalculatedClip>,
            Changed<ComputedStackIndex>,
        )>,
    >,
    paint: Query<
        'w,
        's,
        (&'static ComputedNode, &'static InheritedVisibility),
        Or<(
            Changed<BackgroundColor>,
            Changed<BackgroundGradient>,
            Changed<BorderColor>,
            Changed<BorderGradient>,
            Changed<Outline>,
            Changed<BoxShadow>,
            // Bevy's UI renderer marks this changed when its image asset arrives.
            Changed<ImageNode>,
        )>,
    >,
    text: Query<
        'w,
        's,
        (&'static ComputedNode, &'static InheritedVisibility),
        Or<(Changed<Text>, Changed<TextColor>, Changed<TextLayoutInfo>)>,
    >,
    spans: Query<'w, 's, &'static ChildOf, (With<TextSpan>, Changed<TextColor>)>,
    visibility: Query<'w, 's, &'static InheritedVisibility, With<ComputedNode>>,
    order: Query<'w, 's, (), (With<ComputedNode>, Changed<InheritedVisibility>)>,
    gone: RemovedComponents<'w, 's, ComputedNode>,
    outline_gone: RemovedComponents<'w, 's, Outline>,
    images: Query<'w, 's, (&'static ImageNode, &'static InheritedVisibility)>,
    fonts: Query<'w, 's, (&'static TextFont, &'static InheritedVisibility)>,
}

impl Interface<'_, '_> {
    fn changed(&mut self) -> bool {
        let removed = self.gone.read().count() != 0;
        let outlines = self.outline_gone.read().count() != 0;
        removed
            || outlines
            || self.layout.iter().any(|visibility| visibility.get())
            || self
                .paint
                .iter()
                .any(|(node, visibility)| visibility.get() && !node.is_empty())
            || self
                .text
                .iter()
                .any(|(node, visibility)| visibility.get() && !node.is_empty())
            || self
                .spans
                .iter()
                .any(|parent| self.visibility.get(parent.parent()).is_ok_and(|v| v.get()))
            || !self.order.is_empty()
    }

    fn loading(&self, server: &AssetServer) -> bool {
        let loading = |id| {
            server
                .get_load_state(id)
                .is_some_and(|state| state.is_loading())
        };
        self.images.iter().any(|(node, visibility)| {
            visibility.get() && loading(node.image.id().untyped())
        }) || self.fonts.iter().any(|(font, visibility)| {
            visibility.get()
                && matches!(&font.font, FontSource::Handle(handle) if loading(handle.id().untyped()))
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn pace(
    args: Res<crate::Args>,
    loading: Res<crate::loading::Loading>,
    hud: Res<crate::ui::HudShown>,
    menu: Option<NonSend<crate::main_menu::MenuCore>>,
    server: Res<AssetServer>,
    mut interface: Interface,
    mut events: MessageReader<WindowEvent>,
    mut winit: ResMut<WinitSettings>,
    mut redraw: MessageWriter<RequestRedraw>,
    mut cameras: Query<(Entity, &mut Camera)>,
    mut held: ResMut<HeldCameras>,
    mut startup: Local<u8>,
    mut timer: NonSendMut<timer::Timer>,
    (stage, status): (
        Option<Res<crate::main_menu::HeroStage>>,
        Option<Res<crate::main_menu::StatusAnimation>>,
    ),
    time: Res<Time<Real>>,
    post: Option<NonSend<crate::postgame::PostGame>>,
) {
    // The standalone replay build has no menu core. On desktop, its absence means the
    // window is at first run or showing a menu preview, even if the CLI named a match.
    let match_open = menu.as_ref().map_or(
        cfg!(all(target_arch = "wasm32", feature = "web-replay")),
        |menu| menu.in_game(),
    );
    let live_world = match_open && (hud.0 || loading.shown);
    let continuous = args.unattended() || args.script.is_some() || live_world;
    if set_mode(&mut winit, continuous) && !continuous {
        // Drain anything delivered after the last continuous update polled its inputs.
        redraw.write(RequestRedraw);
    }
    let (window_changed, input) = window_changes(&mut events);
    let starting = *startup < 4;
    *startup = startup.saturating_add(1);
    if continuous {
        timer.set(None);
        return;
    }
    let changed = interface.changed();
    let rendered_image = stage.as_ref().is_some_and(|stage| stage.drawing());
    let settling = starting || changed || window_changed;
    if settling || input || interface.loading(&server) {
        timer.set(None);
        redraw.write(RequestRedraw);
    } else {
        let hero = stage.as_ref().and_then(|stage| stage.next_pose(&time));
        let deadline = menu
            .as_ref()
            .and_then(|menu| menu.next_wake(bevy::platform::time::Instant::now()));
        let post = post.as_ref().and_then(|post| post.next_retry());
        let status = status.as_ref().and_then(|animation| animation.next(&time));
        timer.set(
            hero.into_iter()
                .chain(deadline)
                .chain(post)
                .chain(status)
                .min(),
        );
    }
    if settling || rendered_image {
        return;
    }
    for (entity, mut camera) in &mut cameras {
        if camera.is_active {
            camera.is_active = false;
            held.0.push(entity);
        }
    }
}

fn set_mode(winit: &mut WinitSettings, continuous: bool) -> bool {
    let mode = if continuous {
        WinitSettings::game()
    } else {
        let reactive = UpdateMode::reactive_low_power(Duration::MAX);
        WinitSettings {
            focused_mode: reactive,
            unfocused_mode: reactive,
        }
    };
    if (mode.focused_mode, mode.unfocused_mode) == (winit.focused_mode, winit.unfocused_mode) {
        return false;
    }
    *winit = mode;
    wake::set_continuous(continuous);
    true
}

fn window_changes(events: &mut MessageReader<WindowEvent>) -> (bool, bool) {
    let (mut window, mut input) = (false, false);
    for event in events.read() {
        match event {
            WindowEvent::RequestRedraw(_) => {}
            WindowEvent::CursorMoved(_)
            | WindowEvent::CursorEntered(_)
            | WindowEvent::CursorLeft(_)
            | WindowEvent::MouseButtonInput(_)
            | WindowEvent::MouseWheel(_)
            | WindowEvent::KeyboardInput(_)
            | WindowEvent::TouchInput(_)
            | WindowEvent::MouseMotion(_) => input = true,
            _ => window = true,
        }
    }
    (window, input)
}
