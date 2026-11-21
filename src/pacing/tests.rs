use std::{
    io::Write,
    net::{TcpListener, TcpStream},
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
        mpsc::{self, Receiver, Sender},
    },
};

use bevy::{
    camera::{ImageRenderTarget, RenderTarget},
    platform::time::Instant,
    render::{
        Render, RenderApp, RenderPlugin, RenderSystems,
        gpu_readback::{Readback, ReadbackComplete},
        pipelined_rendering::PipelinedRenderingPlugin,
        render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages},
        view::window::ExtractedWindow,
    },
    tasks::{AsyncComputeTaskPool, Task},
    ui::UiTargetCamera,
    winit::WinitPlugin,
};
use clap::Parser;

use super::*;

const ART: u32 = 46;
const NETWORK: u32 = ART + 10;
const ARM_TIMER: u32 = NETWORK + 1;
const TIMER_FIRED: u32 = ARM_TIMER + 1;
const WORKER: u32 = TIMER_FIRED + 1;
const EXIT: u32 = u32::MAX;
const SIDE: u32 = 64;
const BLUE: [u8; 4] = [0, 0, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];

#[derive(Clone, Resource)]
struct Probe(Arc<Shared>);

struct Shared {
    step: AtomicU32,
    applied: AtomicU32,
    pixel: AtomicU32,
    frames: Sender<Sample>,
}

struct Sample {
    step: u32,
    presents: u32,
    pixel: [u8; 4],
}

#[derive(Default, Resource)]
struct Acquired(bool);

#[derive(Component)]
struct Hidden;

#[derive(Component)]
struct WindowBackground;

#[derive(Resource, Default)]
struct TestDeadline(Option<Instant>);

#[derive(Resource)]
struct Worker {
    task: Task<()>,
    done: bool,
}

#[derive(Resource)]
struct Delayed(Handle<Image>);

#[test]
#[ignore = "requires a real native window in the private compositor"]
fn native_static_ui_stops_presenting_until_its_next_change() {
    let mut app = App::new();
    let (report, frames) = mpsc::channel();
    let (ready, started) = mpsc::channel();
    let probe = Probe(Arc::new(Shared {
        step: AtomicU32::new(0),
        applied: AtomicU32::new(0),
        pixel: AtomicU32::new(0),
        frames: report,
    }));

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
    .insert_resource(probe.clone())
    .add_plugins(super::plugin)
    .init_resource::<TestDeadline>()
    .add_systems(Startup, (scene, move || ready.send(()).unwrap()))
    .add_systems(Update, (receive, change_hidden))
    .add_systems(Last, arm_test_timer.after(super::pace));
    app.sub_app_mut(RenderApp)
        .insert_resource(probe.clone())
        .init_resource::<Acquired>()
        .add_systems(
            Render,
            remember_acquisition
                .after(RenderSystems::PrepareViews)
                .before(RenderSystems::Render),
        )
        .add_systems(Render, report_frame.after(RenderSystems::Render));

    let wake = waker();
    let (release, work) = mpsc::channel();
    let task = AsyncComputeTaskPool::get().spawn(async move {
        let _ = work.recv_timeout(Duration::from_secs(30));
    });
    app.insert_resource(Worker { task, done: false });
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let link = bota_client_core::Net::connect_with(
        &listener.local_addr().unwrap().to_string(),
        bota_client_core::Redial::OFF,
    )
    .unwrap();
    link.set_waker(wake.clone());
    app.insert_non_send(link);
    let (peer, _) = listener.accept().unwrap();
    let driver = std::thread::spawn(move || {
        started
            .recv_timeout(Duration::from_secs(15))
            .expect("window start");
        let report = exercise(&probe, &wake, &frames, peer, release);
        probe.0.step.store(EXIT, Ordering::Release);
        wake.wake();
        report
    });
    assert_eq!(app.run(), AppExit::Success);
    let report = driver.join().unwrap();
    assert!(report.is_ok(), "{report:?}");
}

fn scene(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    commands.spawn(Camera2d);
    commands.spawn((
        WindowBackground,
        crate::ui::fill(),
        BackgroundColor(Color::BLACK),
    ));
    commands.spawn((
        Hidden,
        crate::ui::fill(),
        Visibility::Hidden,
        BackgroundColor(Color::BLACK),
    ));
    let mut surface = Image::new_target_texture(SIDE, SIDE, TextureFormat::Rgba8UnormSrgb, None);
    surface.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let surface = images.add(surface);
    let camera = commands
        .spawn((
            Camera2d,
            Camera {
                order: -1,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.0, 0.0, 1.0)),
                ..default()
            },
            RenderTarget::Image(ImageRenderTarget {
                handle: surface.clone(),
                scale_factor: 1.0,
            }),
        ))
        .id();
    let image = images.reserve_handle();
    commands.spawn((
        Node {
            width: px(SIDE as f32),
            height: px(SIDE as f32),
            ..default()
        },
        UiTargetCamera(camera),
        ImageNode::new(image.clone()),
    ));
    commands.insert_resource(Delayed(image));
    commands.spawn(Readback::texture(surface)).observe(
        |capture: On<ReadbackComplete>, probe: Res<Probe>| {
            let offset = (SIDE as usize * (SIDE as usize / 2) + SIDE as usize / 2) * 4;
            let pixel = capture.data[offset..offset + 4].try_into().unwrap();
            probe
                .0
                .pixel
                .store(u32::from_ne_bytes(pixel), Ordering::Release);
        },
    );
}

fn receive(
    probe: Res<Probe>,
    delayed: Res<Delayed>,
    mut images: ResMut<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
    mut link: NonSendMut<bota_client_core::Net>,
    mut alarm: ResMut<TestDeadline>,
    mut background: Single<&mut BackgroundColor, With<WindowBackground>>,
    mut worker: ResMut<Worker>,
) {
    if !worker.done {
        worker.done = super::poll(&mut worker.task).is_some();
    }
    let step = probe.0.step.load(Ordering::Acquire);
    let applied = probe.0.applied.load(Ordering::Acquire);
    if step == ARM_TIMER && applied == ARM_TIMER {
        if alarm.0.is_some_and(|at| Instant::now() >= at) {
            alarm.0 = None;
            background.0 = Color::BLACK;
            probe.0.applied.store(TIMER_FIRED, Ordering::Release);
        }
        return;
    }
    if step == applied || (step == ARM_TIMER && applied == TIMER_FIRED) {
        return;
    }
    if step == NETWORK
        && !link
            .poll()
            .iter()
            .any(|message| matches!(message, bota_proto::ServerMsg::Refused { .. }))
    {
        return;
    }
    if step == WORKER && !worker.done {
        return;
    }
    probe.0.applied.store(step, Ordering::Release);
    if step == ART {
        images
            .insert(
                delayed.0.id(),
                Image::new_fill(
                    Extent3d::default(),
                    TextureDimension::D2,
                    &RED,
                    TextureFormat::Rgba8UnormSrgb,
                    default(),
                ),
            )
            .unwrap();
    } else if step == NETWORK {
        background.0 = Color::WHITE;
    } else if step == ARM_TIMER {
        alarm.0 = Some(Instant::now() + Duration::from_millis(100));
    } else if step == WORKER {
        background.0 = Color::WHITE;
    } else if step == EXIT {
        exit.write(AppExit::Success);
    }
}

fn arm_test_timer(deadline: Res<TestDeadline>, mut timer: NonSendMut<super::timer::Timer>) {
    if let Some(at) = deadline.0 {
        timer.set(Some(at));
    }
}

fn remember_acquisition(windows: Query<&ExtractedWindow>, mut acquired: ResMut<Acquired>) {
    acquired.0 = windows
        .iter()
        .any(|window| window.swap_chain_texture.is_some());
}

fn report_frame(
    windows: Query<&ExtractedWindow>,
    acquired: Res<Acquired>,
    probe: Res<Probe>,
    mut presents: Local<u32>,
    mut reported: Local<u32>,
) {
    if acquired.0
        && windows
            .iter()
            .any(|window| window.swap_chain_texture.is_none())
    {
        *presents += 1;
    }
    let step = probe.0.applied.load(Ordering::Acquire);
    if step != *reported {
        *reported = step;
        let _ = probe.0.frames.send(Sample {
            step,
            presents: *presents,
            pixel: probe.0.pixel.load(Ordering::Acquire).to_ne_bytes(),
        });
    }
}

fn change_hidden(mut node: Single<(&mut Node, &mut BackgroundColor), With<Hidden>>) {
    // Some systems write style even while their screen is hidden.
    node.0.width = percent(100);
    node.1.0 = Color::WHITE;
}

fn exercise(
    probe: &Probe,
    wake: &std::task::Waker,
    frames: &Receiver<Sample>,
    mut peer: TcpStream,
    release: Sender<()>,
) -> Result<(), String> {
    let mut sample = None;
    let mut idle_presents = 0;
    for step in 1..NETWORK {
        probe.0.step.store(step, Ordering::Release);
        wake.wake_by_ref();
        let frame = frames
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| format!("no response at step {step}: {error}"))?;
        if frame.step != step {
            return Err(format!("received step {} instead of {step}", frame.step));
        }
        if step == 15 {
            if frame.presents == 0 || frame.pixel != BLUE {
                return Err(format!(
                    "initial window or background missing at step {step}: {:?}",
                    frame.pixel
                ));
            }
            idle_presents = frame.presents;
        }
        if step == ART - 1 && frame.presents != idle_presents {
            return Err("native static UI presented during idle wakes".into());
        }
        sample = Some(frame);
    }
    let last = sample.unwrap();
    if last.pixel != RED {
        return Err(format!(
            "image arrived but fixed-size node still shows {:?}",
            last.pixel
        ));
    }
    let receive = |step| -> Result<Sample, String> {
        let frame = frames
            .recv_timeout(Duration::from_secs(10))
            .map_err(|error| format!("no response at step {step}: {error}"))?;
        if frame.step != step {
            return Err(format!("received step {} instead of {step}", frame.step));
        }
        Ok(frame)
    };
    probe.0.step.store(NETWORK, Ordering::Release);
    let message = bota_proto::ServerMsg::Refused {
        reason: bota_proto::RejectReason::SeatHeld,
    };
    peer.write_all(&bota_proto::encode_frame_to_vec(&message).unwrap())
        .unwrap();
    let online = receive(NETWORK)?;
    if online.presents == last.presents {
        return Err("an inbound packet changed the interface but was never presented".into());
    }
    probe.0.step.store(ARM_TIMER, Ordering::Release);
    wake.wake_by_ref();
    let armed = receive(ARM_TIMER)?;
    let alarm = receive(TIMER_FIRED)?;
    if alarm.presents == armed.presents {
        return Err("the timer changed the interface but was never presented".into());
    }
    probe.0.step.store(WORKER, Ordering::Release);
    release.send(()).unwrap();
    let result = receive(WORKER)?;
    if result.presents == alarm.presents {
        return Err("the worker changed the interface but was never presented".into());
    }
    Ok(())
}
