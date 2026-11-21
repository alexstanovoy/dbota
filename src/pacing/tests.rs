use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use bevy::{
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        pipelined_rendering::PipelinedRenderingPlugin, view::window::ExtractedWindow,
    },
    winit::WinitPlugin,
};
use clap::Parser;

use super::*;

#[derive(Clone, Default, Resource)]
struct Presents(Arc<AtomicU32>);

#[derive(Default, Resource)]
struct Acquired(bool);

#[derive(Component)]
struct Hidden;

#[test]
#[ignore = "requires a real native window in the private compositor"]
fn native_static_ui_stops_presenting_until_its_next_change() {
    let mut app = App::new();
    let presents = Presents::default();
    app.add_plugins(
        DefaultPlugins
            .build()
            .disable::<PipelinedRenderingPlugin>()
            .disable::<bevy::audio::AudioPlugin>()
            .disable::<bevy::log::LogPlugin>()
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .set(WinitPlugin {
                run_on_any_thread: true,
            })
            .set(WindowPlugin {
                primary_window: Some(Window {
                    resolution: (320, 240).into(),
                    ..default()
                }),
                ..default()
            }),
    )
    .insert_resource(crate::Args::parse_from(["dbota"]))
    .init_resource::<crate::loading::Loading>()
    .init_resource::<crate::ui::HudShown>()
    .insert_resource(presents.clone())
    .add_plugins(super::plugin)
    .add_systems(Startup, |mut commands: Commands| {
        commands.spawn(Camera2d);
        commands.spawn((crate::ui::fill(), BackgroundColor(Color::BLACK)));
        commands.spawn((
            Hidden,
            crate::ui::fill(),
            Visibility::Hidden,
            BackgroundColor(Color::BLACK),
        ));
    })
    .add_systems(Update, (exercise, change_hidden))
    .add_systems(
        Last,
        (|mut redraw: MessageWriter<RequestRedraw>| {
            // Count actual presentations while simulating extra event-loop updates.
            redraw.write(RequestRedraw);
        })
        .after(super::pace),
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(presents)
        .init_resource::<Acquired>()
        .add_systems(
            Render,
            remember_acquisition
                .after(RenderSystems::PrepareViews)
                .before(RenderSystems::Render),
        )
        .add_systems(Render, count_presentation.after(RenderSystems::Render));
    assert_eq!(app.run(), AppExit::Success);
}

fn remember_acquisition(windows: Query<&ExtractedWindow>, mut acquired: ResMut<Acquired>) {
    acquired.0 = windows
        .iter()
        .any(|window| window.swap_chain_texture.is_some());
}

fn count_presentation(
    windows: Query<&ExtractedWindow>,
    acquired: Res<Acquired>,
    presents: Res<Presents>,
) {
    if acquired.0
        && windows
            .iter()
            .any(|window| window.swap_chain_texture.is_none())
    {
        presents.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn change_hidden(mut node: Single<(&mut Node, &mut BackgroundColor), With<Hidden>>) {
    // Some systems write style even while their screen is hidden.
    node.0.width = percent(100);
    node.1.0 = Color::WHITE;
}

fn exercise(
    presents: Res<Presents>,
    mut frame: Local<u32>,
    mut before: Local<u32>,
    mut color: Single<&mut BackgroundColor, (With<Node>, Without<Hidden>)>,
    mut exit: MessageWriter<AppExit>,
) {
    *frame += 1;
    let presents = presents.0.load(Ordering::Relaxed);
    match *frame {
        20 => {
            assert!(presents > 0, "the initial interface was never presented");
            *before = presents;
        }
        50 => {
            assert_eq!(
                presents, *before,
                "native static UI presented during 30 idle updates"
            );
            color.0 = Color::WHITE;
        }
        55 => {
            assert!(
                presents > *before,
                "the changed native interface was never presented"
            );
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}
