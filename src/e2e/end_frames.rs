//! The end of a match through the whole production renderer, at 1920 by 1080: frames from just
//! before the deciding blow to the screen the client ends on, for a full-map Radiant victory seen
//! by a Radiant player and a 1v1 Dire victory seen by its Radiant loser. The caption must be
//! drawn: in its band of the screen, the frame after it came in holds its words' and glow's
//! pixels where the frame before it held far fewer. `END_FRAMES_DIR` keeps the frames (as the
//! evidence of a clean run), else they go to a folder of this run's own.

use std::time::Duration;

use bevy::{
    camera::{ImageRenderTarget, RenderTarget},
    prelude::*,
    render::{
        gpu_readback::{Readback, ReadbackComplete},
        render_resource::{TextureFormat, TextureUsages},
        renderer::RenderDevice,
    },
    time::TimeUpdateStrategy,
};
use bota_client_core::Intent;
use bota_proto::{Cheat, HeroId, Target, Team, UnitKind, Vec2 as SimVec2};

use super::harness::{Client, Patience, Server};

const SIZE: UVec2 = UVec2::new(1920, 1080);
const FRAME: Duration = Duration::from_nanos(1_000_000_000 / 30);
/// The seconds after the deciding blow each frame is taken at (a negative one before it).
const MOMENTS: [f32; 15] = [
    -0.5, 0.05, 0.4, 0.8, 1.2, 1.6, 2.4, 3.2, 4.5, 6.0, 8.0, 10.0, 11.3, 12.3, 15.0,
];

#[derive(Resource)]
struct Surface(Handle<Image>);
#[derive(Resource, Default)]
struct Capture {
    pending: Option<Entity>,
    pixels: Option<Vec<u8>>,
}

fn prepare(app: &mut App) {
    app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    app.init_resource::<Capture>()
        .add_systems(PostStartup, surface);
}

fn surface(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut camera: Single<(Entity, &mut RenderTarget), With<crate::camera::RtsCamera>>,
) {
    let mut image = Image::new_target_texture(SIZE.x, SIZE.y, TextureFormat::Rgba8UnormSrgb, None);
    image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
    let handle = images.add(image);
    let (entity, target) = &mut *camera;
    **target = RenderTarget::Image(ImageRenderTarget {
        handle: handle.clone(),
        scale_factor: 1.0,
    });
    // The interface is drawn over the world in the same picture, as the window shows them.
    commands.entity(*entity).insert(bevy::ui::IsDefaultUiCamera);
    commands.insert_resource(Surface(handle));
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
    result.pixels = Some(data);
    result.pending = None;
    commands.entity(event.entity).despawn();
}

/// The frame drawn now. Reading it back takes frames of their own: the clock is held still.
fn pixels(client: &mut Client) -> Vec<u8> {
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
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
        patience.pass("the frame read back");
        client.frames(1);
    }
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    client
        .world_mut()
        .resource_mut::<Capture>()
        .pixels
        .take()
        .expect("captured pixels")
}

fn directory() -> std::path::PathBuf {
    let directory = std::env::var_os("END_FRAMES_DIR").map_or_else(
        || std::env::temp_dir().join(format!("end_frames_{}", std::process::id())),
        std::path::PathBuf::from,
    );
    std::fs::create_dir_all(&directory).expect("the frames' folder");
    directory
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
    image
        .try_into_dynamic()
        .expect("RGBA capture")
        .save(directory().join(format!("{name}.png")))
        .expect("the frame saved");
}

/// A rendered client in the seat of `server`, its hero on the map and the scene loaded.
fn rendered_seat(server: &Server) -> Client {
    let mut client = Client::run_rendered(
        &[
            "--addr",
            server.address.as_str(),
            "--mute",
            "--window",
            "1920x1080",
            "--no-vsync",
        ],
        prepare,
    );
    client.until("the hero picked", |c| {
        let core = c.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(Intent::ToggleReady);
        }
        core.ears().is_some()
    });
    client.until("the loaded scene", |c| {
        c.world().resource::<crate::loading::Loading>().done()
    });
    client
}

/// The building of `kind` and `team` nearest `to`.
fn building(
    client: &Client,
    kind: UnitKind,
    team: Team,
    to: SimVec2,
) -> Option<bota_proto::EntityId> {
    client
        .view()
        .units
        .iter()
        .filter(|unit| unit.kind == kind && unit.team == team)
        .min_by_key(|unit| unit.pos.distance_squared(to))
        .map(|unit| unit.id)
}

/// How many pixels of the caption's band read as its words or its glow.
fn caption_pixels(frame: &[u8], winner: Team) -> usize {
    let (x0, x1) = (SIZE.x * 3 / 10, SIZE.x * 7 / 10);
    let (y0, y1) = (SIZE.y * 22 / 100, SIZE.y * 50 / 100);
    let mut count = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            let at = ((y * SIZE.x + x) * 4) as usize;
            let [r, g, b] = [frame[at], frame[at + 1], frame[at + 2]];
            let caption = match winner {
                // White words in a pale green glow.
                Team::Radiant => r > 225 && g > 235 && b > 220,
                // Black words in an orange-red glow.
                _ => r > 170 && g < 110 && b < 70,
            };
            count += usize::from(caption);
        }
    }
    count
}

/// Plays to the deciding blow (the client's hero fells `kind` of `team`) and takes the frames of
/// the end; returns the caption's pixels in the frames before and after it came in.
fn frames_of_the_end(
    name: &str,
    map: &str,
    stand_at: SimVec2,
    kind: UnitKind,
    team: Team,
    winner: Team,
) -> (usize, usize) {
    let server = Server::start(1, &["--map", map]);
    let mut client = rendered_seat(&server);
    client.cheat(Cheat::StartGame);
    // Invulnerable, the hero stands where a player who wins or loses there would.
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::Teleport { at: stand_at });
    client.until("the building in sight", |c| {
        building(c, kind, team, stand_at).is_some()
    });
    // The camera on the hero, as a player's is.
    client.core_mut().pinned = client.core().my_hero().map(bota_client_core::Pin::Unit);
    client.frames(45);
    client.core_mut().pinned = None;
    let target = building(&client, kind, team, stand_at).expect("in sight");
    save(&format!("{name}-00-before"), &pixels(&mut client));
    client.core_mut().act(Intent::Cheat(Cheat::Destroy {
        target: Target::Unit(target),
    }));
    client.until("the match decided", |c| c.core().over.is_some());
    let start = client.world().resource::<Time<Real>>().elapsed_secs();
    let (mut before, mut after) = (0, 0);
    for (index, moment) in MOMENTS.iter().enumerate().skip(1) {
        while client.world().resource::<Time<Real>>().elapsed_secs() - start < *moment {
            client.step_frames(1);
        }
        let frame = pixels(&mut client);
        if *moment < 1.9 {
            before = before.max(caption_pixels(&frame, winner));
        } else if (3.0..6.0).contains(moment) {
            after = after.max(caption_pixels(&frame, winner));
        }
        save(&format!("{name}-{index:02}-at-{moment:04.1}s"), &frame);
    }
    (before, after)
}

#[test]
fn a_full_map_radiant_victory_is_drawn_with_its_caption_through_the_renderer() {
    let (before, after) = frames_of_the_end(
        "5v5-radiant-victory-seen-by-a-winner",
        "0",
        SimVec2::from_ints(13900, 13400),
        UnitKind::Ancient,
        Team::Dire,
        Team::Radiant,
    );
    assert!(
        after > before + 2000,
        "the caption's pixels: {before} before it, {after} after"
    );
}

#[test]
fn a_1v1_dire_victory_is_drawn_with_its_caption_to_the_radiant_loser_through_the_renderer() {
    let (before, after) = frames_of_the_end(
        "1v1-dire-victory-seen-by-the-loser",
        "2",
        SimVec2::from_ints(7500, 7500),
        UnitKind::Tower,
        Team::Radiant,
        Team::Dire,
    );
    assert!(
        after > before + 2000,
        "the caption's pixels: {before} before it, {after} after"
    );
}
