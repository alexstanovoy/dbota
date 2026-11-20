//! The end of a match through the whole production renderer: frames from just before the deciding
//! blow to the screen the client ends on, for a full-map Radiant victory seen by a Radiant player,
//! a 1v1 Dire victory seen by its Radiant loser and a 1v1 Radiant victory seen by its winner (at
//! 1920 by 1080, and at 3840 by 2160). The caption must be drawn: in its band of the screen, the
//! frame after it came in holds its words' and glow's pixels where the frame before it held far
//! fewer; and its glow follows the letters, at every size of the screen. `END_FRAMES_DIR` keeps
//! the frames (as the evidence of a clean run), else they go to a folder of this run's own.

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

/// The size of the picture drawn and read back.
#[derive(Resource, Clone, Copy)]
struct Size(UVec2);
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

fn prepare_720(app: &mut App) {
    prepare(app, UVec2::new(1280, 720));
}

fn prepare_1080(app: &mut App) {
    prepare(app, UVec2::new(1920, 1080));
}

fn prepare_2160(app: &mut App) {
    prepare(app, UVec2::new(3840, 2160));
}

fn prepare(app: &mut App, size: UVec2) {
    app.insert_resource(Size(size));
    app.insert_resource(TimeUpdateStrategy::ManualDuration(FRAME));
    app.init_resource::<Capture>()
        .add_systems(PostStartup, surface);
}

fn surface(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut camera: Single<(Entity, &mut RenderTarget), With<crate::camera::RtsCamera>>,
    size: Res<Size>,
) {
    let Size(size) = *size;
    let mut image = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
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

fn received(
    event: On<ReadbackComplete>,
    mut result: ResMut<Capture>,
    mut commands: Commands,
    size: Res<Size>,
) {
    if result.pending != Some(event.entity) {
        return;
    }
    let Size(size) = *size;
    let stride = RenderDevice::align_copy_bytes_per_row(size.x as usize * 4);
    let data = event
        .data
        .chunks_exact(stride)
        .take(size.y as usize)
        .flat_map(|row| row[..size.x as usize * 4].iter().copied())
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

fn save(name: &str, size: UVec2, bytes: &[u8]) {
    let image = Image::new_fill(
        bevy::render::render_resource::Extent3d {
            width: size.x,
            height: size.y,
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

/// A rendered client of the screen `size` in the seat of `server`, its hero on the map and the
/// scene loaded.
fn rendered_seat(server: &Server, size: UVec2) -> Client {
    let window = format!("{}x{}", size.x, size.y);
    let prepare = match size.y {
        720 => prepare_720,
        2160 => prepare_2160,
        _ => prepare_1080,
    };
    let mut client = Client::run_rendered(
        &[
            "--addr",
            server.address.as_str(),
            "--mute",
            "--window",
            &window,
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

/// The caption's band of the screen: its rows and columns.
fn band(size: UVec2) -> (std::ops::Range<u32>, std::ops::Range<u32>) {
    (size.y * 22 / 100..size.y * 50 / 100, size.x * 25 / 100..size.x * 75 / 100)
}

/// Whether a pixel reads as the caption's words, and as its glow.
fn words_and_glow(winner: Team, [r, g, b]: [u8; 3]) -> (bool, bool) {
    let (r, g, b) = (i32::from(r), i32::from(g), i32::from(b));
    match winner {
        // White words in a pale green glow (#A1FFAC).
        Team::Radiant => (
            r > 235 && g > 235 && b > 235,
            g > 150 && g - r > 40 && g - b > 30,
        ),
        // Black words in an orange glow (#FF3300).
        _ => (r < 40 && g < 40 && b < 40, r > 140 && r - g > 70 && r - b > 90),
    }
}

/// How many pixels of the caption's band read as its words or its glow.
fn caption_pixels(frame: &[u8], size: UVec2, winner: Team) -> usize {
    let (rows, columns) = band(size);
    let mut count = 0;
    for y in rows {
        for x in columns.clone() {
            let at = ((y * size.x + x) * 4) as usize;
            let (word, glow) = words_and_glow(winner, [frame[at], frame[at + 1], frame[at + 2]]);
            // A dark part of the terrain can be the same black as Dire's text. Its red
            // outline is the signal that the letters are visible.
            count += usize::from(glow || (winner == Team::Radiant && word));
        }
    }
    count
}

/// The caption's glow pixels in its band, and how many of them lie farther from every pixel of
/// its words than a glow that follows the letters reaches (three times the 8 px blur of Dota's
/// `text-shadow`, at the screen's size).
fn glow_away_from_the_words(frame: &[u8], size: UVec2, winner: Team) -> (usize, usize) {
    let (rows, columns) = band(size);
    let (width, height) = (columns.len(), rows.len());
    let reach = (24.0 * size.y as f32 / 1080.0).ceil() as usize;
    let mut words = vec![false; width * height];
    let mut glow = vec![false; width * height];
    for (j, y) in rows.clone().enumerate() {
        for (i, x) in columns.clone().enumerate() {
            let at = ((y * size.x + x) * 4) as usize;
            let (word, lit) = words_and_glow(winner, [frame[at], frame[at + 1], frame[at + 2]]);
            words[j * width + i] = word;
            glow[j * width + i] = lit;
        }
    }
    // The words grown by `reach` (a square about each), across then down.
    let mut across = vec![false; width * height];
    for j in 0..height {
        for i in 0..width {
            let low = i.saturating_sub(reach);
            let high = (i + reach).min(width - 1);
            across[j * width + i] = (low..=high).any(|k| words[j * width + k]);
        }
    }
    let mut near = vec![false; width * height];
    for j in 0..height {
        for i in 0..width {
            let low = j.saturating_sub(reach);
            let high = (j + reach).min(height - 1);
            near[j * width + i] = (low..=high).any(|k| across[k * width + i]);
        }
    }
    let lit = glow.iter().filter(|lit| **lit).count();
    let away = glow.iter().zip(&near).filter(|(lit, near)| **lit && !**near).count();
    (lit, away)
}

/// One end to film: where the hero stands, what it fells, who wins, on which map, at which size,
/// at which moments.
struct Scene {
    name: &'static str,
    map: &'static str,
    stand_at: SimVec2,
    kind: UnitKind,
    team: Team,
    winner: Team,
    size: UVec2,
    moments: &'static [f32],
}

/// What the frames of an end showed of its caption: its pixels before it came in and after, and
/// its glow (all of it, and what lay away from the words) a little after it came in.
struct Filmed {
    before: usize,
    after: usize,
    glow: (usize, usize),
}

/// Plays to the deciding blow (the client's hero fells `kind` of `team`) and takes the frames of
/// the end at the scene's moments.
fn film(scene: &Scene) -> Filmed {
    let server = Server::start(1, &["--map", scene.map]);
    let mut client = rendered_seat(&server, scene.size);
    client.cheat(Cheat::StartGame);
    // Invulnerable, the hero stands where a player who wins or loses there would.
    client.cheat(Cheat::Invulnerable { on: true });
    client.cheat(Cheat::Teleport { at: scene.stand_at });
    client.until("the building in sight", |c| {
        building(c, scene.kind, scene.team, scene.stand_at).is_some()
    });
    // The camera on the hero, as a player's is.
    client.core_mut().pinned = client.core().my_hero().map(bota_client_core::Pin::Unit);
    client.frames(45);
    client.core_mut().pinned = None;
    let target = building(&client, scene.kind, scene.team, scene.stand_at).expect("in sight");
    let name = scene.name;
    if scene.moments.len() == MOMENTS.len() {
        save(&format!("{name}-00-before"), scene.size, &pixels(&mut client));
    }
    client.core_mut().act(Intent::Cheat(Cheat::Destroy {
        target: Target::Unit(target),
    }));
    client.until("the match decided", |c| c.core().over.is_some());
    let start = client.world().resource::<Time<Real>>().elapsed_secs();
    let mut filmed = Filmed {
        before: 0,
        after: 0,
        glow: (0, 0),
    };
    for moment in scene.moments.iter().filter(|moment| **moment > 0.0) {
        while client.world().resource::<Time<Real>>().elapsed_secs() - start < *moment {
            client.step_frames(1);
        }
        let frame = pixels(&mut client);
        let caption = caption_pixels(&frame, scene.size, scene.winner);
        if *moment < 1.9 {
            filmed.before = filmed.before.max(caption);
        } else if (3.0..6.0).contains(moment) {
            filmed.after = filmed.after.max(caption);
        }
        if (3.0..3.5).contains(moment) {
            filmed.glow = glow_away_from_the_words(&frame, scene.size, scene.winner);
        }
        let index = MOMENTS.iter().position(|m| m == moment).unwrap_or(0);
        save(&format!("{name}-{index:02}-at-{moment:04.1}s"), scene.size, &frame);
    }
    filmed
}

/// The caption came in, and its glow lies about its letters, not in a patch of its own.
fn assert_captioned(scene: &Scene) {
    let Filmed {
        before,
        after,
        glow: (lit, away),
    } = film(scene);
    let area = (scene.size.x * scene.size.y) as usize / (1920 * 1080 / 100);
    assert!(
        after > before + 20 * area,
        "{}: the caption's pixels: {before} before it, {after} after",
        scene.name
    );
    assert!(lit > 5 * area, "{}: the glow is drawn: {lit} pixels", scene.name);
    assert!(
        away * 20 <= lit,
        "{}: the glow follows the letters: {away} of its {lit} pixels lie away from them",
        scene.name
    );
}

const AT_1080: UVec2 = UVec2::new(1920, 1080);

#[test]
fn a_full_map_radiant_victory_is_drawn_with_its_caption_through_the_renderer() {
    assert_captioned(&Scene {
        name: "5v5-radiant-victory-seen-by-a-winner",
        map: "0",
        stand_at: SimVec2::from_ints(13900, 13400),
        kind: UnitKind::Ancient,
        team: Team::Dire,
        winner: Team::Radiant,
        size: AT_1080,
        moments: &MOMENTS,
    });
}

#[test]
fn a_1v1_dire_victory_is_drawn_with_its_caption_to_the_radiant_loser_through_the_renderer() {
    assert_captioned(&Scene {
        name: "1v1-dire-victory-seen-by-the-loser",
        map: "2",
        stand_at: SimVec2::from_ints(7500, 7500),
        kind: UnitKind::Tower,
        team: Team::Radiant,
        winner: Team::Dire,
        size: AT_1080,
        moments: &MOMENTS,
    });
}

/// The 1v1 seen by its winner, at 1920 by 1080 and at 3840 by 2160, and its caption alone at
/// 1280 by 720: the caption keeps its size and its glow on every screen.
#[test]
fn a_1v1_radiant_victory_is_drawn_with_its_caption_to_its_winner_on_every_screen() {
    for (name, size, moments) in [
        ("1v1-radiant-victory-seen-by-the-winner", AT_1080, &MOMENTS[..]),
        (
            "1v1-radiant-victory-seen-by-the-winner-3840x2160",
            UVec2::new(3840, 2160),
            &MOMENTS[..],
        ),
        (
            "1v1-radiant-victory-seen-by-the-winner-1280x720",
            UVec2::new(1280, 720),
            &MOMENTS[1..9],
        ),
    ] {
        assert_captioned(&Scene {
            name,
            map: "2",
            stand_at: SimVec2::from_ints(7500, 7500),
            kind: UnitKind::Tower,
            team: Team::Dire,
            winner: Team::Radiant,
            size,
            moments,
        });
    }
}
