//! The visible contribution of objective effects through the whole production renderer.
use super::{
    creep_walk::ControlledFrames,
    harness::{Client, Patience, Server},
};
use bevy::{
    camera::{ImageRenderTarget, RenderTarget},
    gizmos::config::{GizmoConfigGroup, GizmoConfigStore},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
    time::TimeUpdateStrategy,
};
use bota_proto::{Cheat, HeroId, UnitKind};
use std::{f32::consts::TAU, sync::Arc, time::Duration};

const SIZE: UVec2 = UVec2::new(1600, 900);
const RADIUS: f32 = 450.0;
const PIXEL_CONTRAST: u8 = 8;
#[derive(Resource)]
struct Surface(Handle<Image>);
#[derive(Resource, Default)]
struct Capture {
    pending: Option<Entity>,
    pixels: Option<Vec<u8>>,
}
#[derive(Resource, Default)]
struct Reference {
    at: Option<Vec2>,
}
#[derive(Default, Reflect, GizmoConfigGroup)]
struct ReferenceGroup;

fn prepare(app: &mut App) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    app.init_gizmo_group::<ReferenceGroup>()
        .init_resource::<Capture>()
        .init_resource::<Reference>()
        .add_systems(PostStartup, (surface, calibrate))
        .add_systems(PostUpdate, reference.after(crate::camera::CursorPicked));
}
fn surface(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut camera: Single<&mut RenderTarget, With<crate::camera::RtsCamera>>,
) {
    let mut image = Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = images.add(image);
    **camera = RenderTarget::Image(ImageRenderTarget {
        handle: handle.clone(),
        scale_factor: 1.0,
    });
    commands.insert_resource(Surface(handle));
}
fn calibrate(mut store: ResMut<GizmoConfigStore>) {
    let mut config = store.config::<DefaultGizmoConfigGroup>().0.clone();
    // Keep the same clearly visible two-pixel range stroke when comparing older builds.
    config.line.width = 2.0;
    *store.config_mut::<ReferenceGroup>().0 = config;
}
fn reference(
    probe: Res<Reference>,
    ground: Res<crate::map::Ground>,
    mut gizmos: Gizmos<ReferenceGroup>,
) {
    let Some(at) = probe.at else {
        return;
    };
    // The same visible, terrain-following renderer and stroke configuration as range indicators.
    gizmos.linestrip(
        (0..=128).map(|i| {
            let angle = i as f32 * TAU / 128.0;
            let p = at + Vec2::new(angle.cos(), angle.sin()) * RADIUS;
            Vec3::new(
                p.x,
                ground.surface_at(p.x, p.y) + crate::overlay::LIFT,
                -p.y,
            )
        }),
        crate::overlay::GOOD.with_alpha(0.9),
    );
}
fn received(event: On<ReadbackComplete>, mut result: ResMut<Capture>, mut commands: Commands) {
    if result.pending != Some(event.entity) {
        return;
    }
    let stride = RenderDevice::align_copy_bytes_per_row(SIZE.x as usize * 4);
    let data = event
        .data
        .chunks_exact(stride)
        .take(SIZE.y as usize)
        .flat_map(|row| row[..SIZE.x as usize * 4].iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(data.len(), SIZE.x as usize * SIZE.y as usize * 4);
    result.pixels = Some(data);
    result.pending = None;
    commands.entity(event.entity).despawn();
}
fn pixels(client: &mut Client) -> Vec<u8> {
    let handle = client.world().resource::<Surface>().0.clone();
    let entity = client
        .world_mut()
        .spawn(Readback::texture(handle))
        .observe(received)
        .id();
    *client.world_mut().resource_mut::<Capture>() = Capture {
        pending: Some(entity),
        pixels: None,
    };
    let mut patience = Patience::of(Duration::from_secs(60));
    while client.world().resource::<Capture>().pixels.is_none() {
        patience.pass("the offscreen objective pixels");
        client.frames(1);
    }
    client
        .world_mut()
        .resource_mut::<Capture>()
        .pixels
        .take()
        .expect("captured pixels")
}
fn save(name: &str, bytes: &[u8]) {
    let image = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: SIZE.x,
            height: SIZE.y,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8UnormSrgb,
        default(),
    );
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("temp/evidence/item14");
    std::fs::create_dir_all(&directory).expect("the pixel evidence directory");
    image
        .try_into_dynamic()
        .expect("RGBA capture")
        .save(directory.join(format!("{name}.png")))
        .expect("pixel evidence");
}
#[derive(serde::Serialize, Debug)]
struct Difference {
    changed: usize,
    energy: u64,
    peak: u8,
}
fn difference(a: &[u8], b: &[u8], low: UVec2, high: UVec2, unstable: &[usize]) -> Difference {
    let mut measured = Difference {
        changed: 0,
        energy: 0,
        peak: 0,
    };
    for y in low.y..high.y {
        for x in low.x..high.x {
            let i = ((y * SIZE.x + x) * 4) as usize;
            if unstable.contains(&i) {
                continue;
            }
            let mut peak = 0;
            for channel in 0..3 {
                let delta = a[i + channel].abs_diff(b[i + channel]);
                peak = peak.max(delta);
                measured.energy += u64::from(delta);
            }
            measured.peak = measured.peak.max(peak);
            measured.changed += usize::from(peak >= PIXEL_CONTRAST);
        }
    }
    measured
}
fn raster_noise(
    on: &[u8],
    on_again: &[u8],
    off: &[u8],
    off_again: &[u8],
    low: UVec2,
    high: UVec2,
) -> Vec<usize> {
    let mut unstable = Vec::new();
    for y in low.y..high.y {
        for x in low.x..high.x {
            let i = ((y * SIZE.x + x) * 4) as usize;
            if (0..3).any(|c| {
                on[i + c].abs_diff(on_again[i + c]) >= PIXEL_CONTRAST
                    || off[i + c].abs_diff(off_again[i + c]) >= PIXEL_CONTRAST
            }) {
                unstable.push(i);
            }
        }
    }
    // Exclude only a tiny, bounded control mask; it can never count as effect signal.
    assert!(
        unstable.len() <= 16,
        "held-frame raster changes at {} pixels",
        unstable.len()
    );
    unstable
}
fn region(client: &mut Client, at: Vec2) -> (UVec2, UVec2) {
    let mut points = Vec::new();
    for i in 0..32 {
        let angle = i as f32 * TAU / 32.0;
        let p = at + Vec2::new(angle.cos(), angle.sin()) * 650.0;
        if let Some(point) = client.screen_of((p.x, p.y)) {
            points.push(point);
        }
    }
    assert!(points.len() >= 16, "the effect region is on camera");
    let low = points
        .iter()
        .copied()
        .fold(Vec2::INFINITY, Vec2::min)
        .floor()
        .max(Vec2::ZERO)
        .as_uvec2();
    let high = points
        .iter()
        .copied()
        .fold(Vec2::NEG_INFINITY, Vec2::max)
        .ceil()
        .min(SIZE.as_vec2())
        .as_uvec2();
    (low, high)
}
fn compare(client: &mut Client, name: &str, at: Vec2, effects: Vec<Arc<crate::particles::Effect>>) {
    let (low, high) = region(client, at);
    client.frames(3);
    let held_tick = client.tick();
    let held_time = client.world().resource::<Time>().elapsed();
    let on = pixels(client);
    let repeated = pixels(client);
    assert_eq!(
        client.tick(),
        held_tick,
        "the captured game frame stays fixed"
    );
    assert_eq!(client.world().resource::<Time>().elapsed(), held_time);
    client
        .world_mut()
        .resource_mut::<crate::particles::Playing>()
        .hide_effects(effects);
    client.frames(3);
    let off = pixels(client);
    let off_again = pixels(client);
    let unstable = raster_noise(&on, &repeated, &off, &off_again, low, high);
    client.world_mut().resource_mut::<Reference>().at = Some(at);
    client.frames(3);
    let reference = pixels(client);
    client.world_mut().resource_mut::<Reference>().at = None;
    client
        .world_mut()
        .resource_mut::<crate::particles::Playing>()
        .hide_effects(Vec::new());
    let effect = difference(&on, &off, low, high, &unstable);
    let known = difference(&reference, &off, low, high, &unstable);
    save(&format!("{name}_enabled"), &on);
    save(&format!("{name}_disabled"), &off);
    let report = serde_json::json!({"case":name,"tick":client.tick(),"region":[low.to_array(),high.to_array()],"effect":effect,"reference":known,"excluded_control_pixels":unstable.len(),"expected_visible":true,"threshold":{"minimum_pixels":(known.changed/5).max(64),"minimum_rgb_difference":PIXEL_CONTRAST,"minimum_peak":known.peak/4}});
    let directory =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("temp/evidence/item14");
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec_pretty(&report).expect("metrics"),
    )
    .expect("metrics saved");
    assert_eq!(client.tick(), held_tick);
    assert_eq!(client.world().resource::<Time>().elapsed(), held_time);
    println!("objective pixels {report}");
    assert!(
        known.changed >= 64 && known.peak >= 32,
        "{name}: the reference range ring is clearly visible: {known:?}"
    );
    let required = (known.changed / 5).max(64);
    assert!(
        effect.changed >= required && effect.peak >= known.peak / 4,
        "{name}: ground effect is not clearly visible: {effect:?}, reference {known:?}"
    );
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    tick: u32,
    at: bota_proto::Vec2,
}
struct StopAfter(std::rc::Rc<std::cell::Cell<bool>>);
impl bota_client_core::Observer for StopAfter {
    type Error = &'static str;
    fn after_snapshot(&mut self, _: &bota_client_core::ClientSession) -> Result<(), Self::Error> {
        if self.0.get() {
            Err("four states captured")
        } else {
            Ok(())
        }
    }
}

fn recorded_states() -> (std::path::PathBuf, Vec<Case>) {
    use bota_client_core::{IssuedOrder, Link, join, play};
    use bota_proto::{EffectId, Order, Vec2 as Spot};
    let file = crate::test_support::scratch_path("objective_states.brp");
    let mut server = Server::start(
        1,
        &[
            "--map",
            "0",
            "--objectives",
            "lotus_pools,shrines",
            "--replay",
            file.to_str().expect("path"),
        ],
    );
    let mut link =
        Link::connect(&server.address, Duration::from_secs(10)).expect("fixture transport");
    let seated = join(&mut link, "observer", HeroId::SHADOW_FIEND, None).expect("fixture joins");
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let mut observer = StopAfter(done.clone());
    let mut stage = 0;
    let mut cases = Vec::new();
    let mut setup = std::collections::VecDeque::from([
        Cheat::CreepSpawning { on: false },
        Cheat::Invulnerable { on: true },
        Cheat::StartGame,
    ]);
    let result = play(&mut link, seated, &mut observer, |session| {
        let view = session.current().expect("tick");
        let me = session.own_hero().expect("hero");
        let pool = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::LotusPool)
            .expect("pool");
        let shrine = view
            .units
            .iter()
            .find(|u| u.kind == UnitKind::Shrine)
            .expect("shrine");
        assert!(
            view.tick < 30_000,
            "bounded fixture: stage {stage}, hero {:?}, lotuses {:?}",
            me.pos,
            view.pools
        );
        let teleport = |unit: &bota_proto::UnitView, offset: i32| Cheat::Teleport {
            at: unit.pos + Spot::from_ints(offset, 0),
        };
        let cheat = if let Some(cheat) = setup.pop_front() {
            Some(cheat)
        } else {
            match stage {
                0 => {
                    stage = 1;
                    Some(teleport(pool, -500))
                }
                1 if view.pools.iter().any(|p| p.id == pool.id && p.lotuses > 0) => {
                    cases.push(Case {
                        name: "lotus_available",
                        tick: view.tick,
                        at: pool.pos,
                    });
                    stage = 2;
                    Some(teleport(pool, -200))
                }
                2 if pool.gathering.is_some_and(|g| g.progress * 2 >= g.ticks) => {
                    cases.push(Case {
                        name: "lotus_gathering",
                        tick: view.tick,
                        at: pool.pos,
                    });
                    stage = 3;
                    Some(teleport(shrine, -500))
                }
                3 if shrine
                    .effects
                    .iter()
                    .any(|e| e.id == EffectId::SHRINE_READY) =>
                {
                    stage = 4;
                    Some(Cheat::TimeOfDay { night: false })
                }
                4 => {
                    cases.push(Case {
                        name: "wisdom_ready",
                        tick: view.tick,
                        at: shrine.pos,
                    });
                    stage = 5;
                    Some(teleport(shrine, 0))
                }
                5 if shrine.gathering.is_some_and(|g| g.progress * 2 >= g.ticks) => {
                    cases.push(Case {
                        name: "wisdom_gathering",
                        tick: view.tick,
                        at: shrine.pos,
                    });
                    done.set(true);
                    None
                }
                _ => None,
            }
        };
        cheat.map(|cheat| IssuedOrder::hero(Order::Cheat { cheat }))
    });
    assert!(
        matches!(
            result,
            Err(bota_client_core::PlayError::Observer(
                "four states captured"
            ))
        ),
        "{result:?}"
    );
    bota_client_core::Transport::send(&mut link, &bota_proto::ClientMsg::LeaveGame)
        .expect("end fixture");
    drop(link);
    server.wait();
    assert_eq!(cases.len(), 4);
    (file, cases)
}

fn seek(client: &mut Client, tick: u32) {
    let delta = tick.checked_sub(client.tick()).expect("increasing cases");
    if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
        player.advance_ticks(f64::from(delta));
    }
    let render = client
        .app
        .remove_sub_app(bevy::render::RenderApp)
        .expect("renderer");
    client.until("the recorded objective tick", |client| {
        client.tick() == tick
    });
    client.app.insert_sub_app(bevy::render::RenderApp, render);
}

#[test]
#[ignore = "offscreen production renderer; visual_check runs this contract"]
fn four_objective_states_have_visible_authored_effects() {
    let (file, cases) = recorded_states();
    let mut client = Client::run_rendered(
        &[
            "--replay",
            file.to_str().expect("path"),
            "--mute",
            "--window",
            "1600x900",
            "--distance",
            "3500",
            "--no-vsync",
        ],
        prepare,
    );
    if let bota_client_core::Source::Replay(player) = &mut client.core_mut().source {
        player.paused = true;
    }
    client.until("first replay snapshot", |client| {
        client.core().view().is_some()
    });
    let source = std::mem::replace(
        &mut client.core_mut().source,
        crate::main_menu::idle().0.source,
    );
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            100,
        )));
    client.until("the loaded scene", |client| {
        client.world().resource::<crate::loading::Loading>().done()
    });
    client.core_mut().source = source;
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    for (case, logical) in cases.into_iter().zip([
        "lotus_pool.ambient",
        "lotus_pool.gathering",
        "shrine.ambient",
        "shrine.active",
    ]) {
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        seek(&mut client, case.tick);
        let kind = if case.name.starts_with("lotus") {
            UnitKind::LotusPool
        } else {
            UnitKind::Shrine
        };
        let unit = client
            .view()
            .units
            .iter()
            .find(|unit| unit.kind == kind && unit.pos == case.at)
            .expect("recorded objective");
        let id = unit.id;
        let at = Vec2::new(unit.pos.x.to_f32(), unit.pos.y.to_f32());
        if case.name.ends_with("gathering") {
            assert!(unit.gathering.is_some_and(|g| g.progress > 0));
        }
        let replay = std::mem::replace(
            &mut client.core_mut().source,
            bota_client_core::Source::Live(Box::new(ControlledFrames)),
        );
        client.core_mut().pinned = Some(bota_client_core::Pin::Unit(id));
        client.frames(2);
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 30.0,
            )));
        client.frames(30);
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
        let effect = client
            .world()
            .resource::<crate::particles::Library>()
            .effect(logical)
            .expect("effect")
            .clone();
        compare(&mut client, case.name, at, vec![effect]);
        client.core_mut().source = replay;
    }
}
