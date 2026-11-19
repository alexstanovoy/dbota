//! The HUD portrait's face frame by frame, as a player's client draws it: the production
//! renderer draws the portrait at its own rate into its image, read back on every frame of the
//! client, with the frame times a display gives (240 Hz, 60 Hz, and a browser's uneven frames)
//! and the voice heard through an output pulled a device buffer at a time (a line's face keeps
//! time with its sound's reported position). Each capture leaves contact sheets of consecutive
//! frames and a per-frame trace (`FACE_FRAMES_DIR`, else a folder of the run's own).

use std::fmt::Write as _;
use std::sync::Arc;
use std::time::Duration;

use bevy::mesh::morph::MorphWeights;
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::renderer::RenderDevice;
use bevy::time::TimeUpdateStrategy;
use bota_proto::{Cheat, HeroId, Order, Target, Vec2 as At};

use super::harness::{Client, Server};
use crate::portrait::Stage;
use crate::talk::{LineFace, Speech};

/// The frame times of a display.
#[derive(Clone, Copy, Debug)]
pub(super) enum Pacing {
    Hz240,
    Hz60,
    Hz30,
    Hz144,
    /// A browser's frames: its 60 Hz animation frames, of which a busy single thread misses some
    /// (a quarter of them take two vsyncs, one in twenty three), each timestamp off by up to 1 ms.
    Browser,
}

impl Pacing {
    pub(super) fn name(self) -> &'static str {
        match self {
            Pacing::Hz240 => "240hz",
            Pacing::Hz60 => "60hz",
            Pacing::Hz30 => "30hz",
            Pacing::Hz144 => "144hz",
            Pacing::Browser => "browser",
        }
    }

    /// Buffer lengths for the controlled 2048-frame fixtures, at 48 or 44.1 kHz.
    pub(super) fn device_buffer(self) -> Duration {
        match self {
            Pacing::Browser => Duration::from_micros(46_440),
            _ => Duration::from_micros(42_667),
        }
    }

    pub(super) fn sample_rate(self) -> u32 {
        match self {
            Pacing::Browser => 44_100,
            _ => 48_000,
        }
    }

    /// The frame times covering `seconds`, the same on every run.
    pub(super) fn frames(self, seconds: f32) -> Vec<Duration> {
        let mut seed: u64 = 0x5eed_f00d;
        let mut random = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (seed >> 33) as f32 / (1u64 << 31) as f32
        };
        let mut out = Vec::new();
        let mut total = 0.0;
        let mut jitter_before = 0.0;
        while total < seconds {
            let step = match self {
                Pacing::Hz240 => 1.0 / 240.0,
                Pacing::Hz60 => 1.0 / 60.0,
                Pacing::Hz30 => 1.0 / 30.0,
                Pacing::Hz144 => 1.0 / 144.0,
                Pacing::Browser => {
                    let vsyncs = match random() {
                        r if r < 0.05 => 3.0,
                        r if r < 0.30 => 2.0,
                        _ => 1.0,
                    };
                    let jitter = (random() - 0.5) * 0.002;
                    let step = vsyncs / 60.0 + jitter - jitter_before;
                    jitter_before = jitter;
                    step
                }
            };
            total += step;
            out.push(Duration::from_secs_f32(step));
        }
        out
    }
}

/// The portrait's image read back on every frame, in order.
#[derive(Resource, Default)]
struct Captured {
    size: UVec2,
    images: Vec<Vec<u8>>,
}

fn prepare(app: &mut App) {
    app.init_resource::<Captured>();
}

fn received(event: On<ReadbackComplete>, mut captured: ResMut<Captured>) {
    let size = captured.size;
    let stride = RenderDevice::align_copy_bytes_per_row(size.x as usize * 4);
    let data = event
        .data
        .chunks_exact(stride)
        .take(size.y as usize)
        .flat_map(|row| row[..size.x as usize * 4].iter().copied())
        .collect::<Vec<_>>();
    captured.images.push(data);
}

/// One frame's state: its time, whether the portrait was drawn on it, the line said, the face's
/// morph weights, the head and jaw against the stage's pivot, and the body's playing clips.
struct Row {
    dt: f32,
    now: f32,
    drawn: bool,
    said: Option<(String, f32)>,
    presented: Option<f32>,
    weights: Vec<f32>,
    bones: Vec<(String, Vec3, Quat)>,
    clips: Vec<(u32, f32, f32)>,
}

fn row(client: &Client, dt: f32) -> Row {
    let world = client.world();
    let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
    let stage = world.resource::<Stage>();
    let speech = world.resource::<Speech>();
    let said = speech
        .of(Some(client.me().id), now)
        .map(|(face, t)| (face.file.clone(), t));
    let presented = speech.active_playback().and_then(|(clock, entity)| {
        let elapsed = world.resource::<Time<bevy::time::Real>>().elapsed();
        let presented = world
            .resource::<crate::audio_output::Output>()
            .presentation(elapsed)?;
        if let crate::audio_output::Played::At(position) = presented(
            clock,
            world.get::<bevy::audio::AudioPlayer>(entity).is_some(),
        ) {
            Some(position)
        } else {
            None
        }
    });
    let mut weights = Vec::new();
    let mut bones = Vec::new();
    let mut clips = Vec::new();
    if let Some(actor) = stage.actor.as_ref() {
        if let Some(face) = actor.face.as_ref() {
            for (node, _) in face.nodes() {
                if let Some(w) = world.get::<MorphWeights>(node) {
                    weights.extend_from_slice(w.weights());
                }
            }
        }
        let pivot = world
            .get::<GlobalTransform>(actor.pivot)
            .map(|g| g.affine().inverse());
        let mut stack = vec![actor.pivot];
        let mut steps = 0;
        while let Some(e) = stack.pop() {
            steps += 1;
            if steps > 100_000 {
                break;
            }
            if let Some(children) = world.get::<Children>(e) {
                stack.extend(children.iter());
            }
            if let Some(player) = world.get::<AnimationPlayer>(e) {
                for (node, active) in player.playing_animations() {
                    clips.push((node.index() as u32, active.weight(), active.seek_time()));
                }
            }
            let Some(name) = world.get::<Name>(e) else {
                continue;
            };
            let lower = name.as_str().to_lowercase();
            let wanted = ["head", "jaw"].iter().any(|bone| lower.contains(bone));
            if let (true, Some(pivot), Some(at)) = (wanted, pivot, world.get::<GlobalTransform>(e))
            {
                let (_, turn, place) = (pivot * at.affine()).to_scale_rotation_translation();
                bones.push((name.as_str().to_owned(), place, turn));
            }
        }
    }
    bones.sort_by(|a, b| a.0.cmp(&b.0));
    Row {
        dt,
        now,
        drawn: stage.drawing(),
        said,
        presented,
        weights,
        bones,
        clips,
    }
}

/// What a capture does on its frames: nothing more, or something on a given frame.
pub(super) type Action = Box<dyn FnMut(&mut Client, usize)>;

/// Runs `pacing`'s frames for `seconds` with `act` called before each, the portrait read back on
/// every one: the frames' rows and images.
fn capture(
    client: &mut Client,
    pacing: Pacing,
    seconds: f32,
    mut act: Action,
) -> (Vec<Row>, Vec<Vec<u8>>) {
    let live = client.world().resource::<Stage>().live().clone();
    let size = client
        .world()
        .resource::<Assets<Image>>()
        .get(&live)
        .expect("the portrait's image")
        .size();
    {
        let mut captured = client.world_mut().resource_mut::<Captured>();
        captured.size = size;
        captured.images.clear();
    }
    let reader = client
        .world_mut()
        .spawn(Readback::texture(live))
        .observe(received)
        .id();
    let mut rows = Vec::new();
    for (frame, step) in pacing.frames(seconds).into_iter().enumerate() {
        act(client, frame);
        client
            .world_mut()
            .insert_resource(TimeUpdateStrategy::ManualDuration(step));
        client.step_frames(1);
        rows.push(row(client, step.as_secs_f32()));
    }
    // The readbacks still in flight arrive on the next frames, the clock held.
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO));
    for _ in 0..8 {
        client.step_frames(1);
    }
    client.world_mut().despawn(reader);
    let mut images = std::mem::take(&mut client.world_mut().resource_mut::<Captured>().images);
    assert!(
        images.len() >= rows.len(),
        "a portrait read back on every frame: {} of {}",
        images.len(),
        rows.len()
    );
    images.truncate(rows.len());
    (rows, images)
}

/// Where the sheets and traces go.
fn evidence() -> std::path::PathBuf {
    let dir = std::env::var_os("FACE_FRAMES_DIR").map_or_else(
        || std::env::temp_dir().join(format!("face_frames_{}", std::process::id())),
        std::path::PathBuf::from,
    );
    std::fs::create_dir_all(&dir).expect("the evidence folder");
    dir
}

/// The part of the portrait the sheets show (x, y, width, height of its 352 by 320 image): the
/// face, from the brow to the jaw.
const CROP: (u32, u32, u32, u32) = (40, 40, 272, 272);
/// How many times larger the sheets show it than its image (four times what the HUD shows: it
/// draws the image at half its size).
const ZOOM: u32 = 2;
/// Frames on a sheet: its columns and rows.
const SHEET: (u32, u32) = (8, 5);

/// The mean change of a pixel's channel from `a` to `b` over the crop.
fn pixel_change(a: &[u8], b: &[u8], width: u32) -> f32 {
    let (x0, y0, w, h) = CROP;
    let mut sum = 0u64;
    for y in y0..y0 + h {
        for x in x0..x0 + w {
            let i = ((y * width + x) * 4) as usize;
            for c in 0..3 {
                sum += u64::from(a[i + c].abs_diff(b[i + c]));
            }
        }
    }
    sum as f32 / (w * h * 3) as f32
}

/// A 3x5 digit font, rows of three bits.
const DIGITS: [[u8; 5]; 10] = [
    [7, 5, 5, 5, 7],
    [2, 6, 2, 2, 7],
    [7, 1, 7, 4, 7],
    [7, 1, 7, 1, 7],
    [5, 5, 7, 1, 1],
    [7, 4, 7, 1, 7],
    [7, 4, 7, 5, 7],
    [7, 1, 1, 1, 1],
    [7, 5, 7, 5, 7],
    [7, 5, 7, 1, 7],
];

/// An RGBA picture being drawn.
struct Canvas {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

impl Canvas {
    fn new(width: u32, height: u32) -> Canvas {
        let mut data = vec![0; (width * height * 4) as usize];
        data.as_chunks_mut::<4>()
            .0
            .iter_mut()
            .for_each(|p| p[3] = 255);
        Canvas {
            width,
            height,
            data,
        }
    }

    fn save(self, path: &std::path::Path) {
        let image = Image::new(
            bevy::render::render_resource::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
            bevy::render::render_resource::TextureDimension::D2,
            self.data,
            bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
            bevy::asset::RenderAssetUsages::MAIN_WORLD,
        );
        let dynamic = image.try_into_dynamic().expect("a picture");
        dynamic.to_rgba8().save(path).expect("a sheet written");
    }
}

fn put(image: &mut Canvas, x: u32, y: u32, colour: [u8; 4]) {
    if x < image.width && y < image.height {
        let i = ((y * image.width + x) * 4) as usize;
        image.data[i..i + 4].copy_from_slice(&colour);
    }
}

fn number(image: &mut Canvas, mut x: u32, y: u32, n: usize, colour: [u8; 4]) {
    for digit in n.to_string().bytes() {
        let glyph = DIGITS[(digit - b'0') as usize];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3 {
                if bits & (4 >> col) != 0 {
                    for dy in 0..3 {
                        for dx in 0..3 {
                            put(image, x + col * 3 + dx, y + row as u32 * 3 + dy, colour);
                        }
                    }
                }
            }
        }
        x += 12;
    }
}

/// Writes the sheets of a capture (`<prefix>-<nn>.png`), each frame's crop with its number, a
/// green bar under the frames the portrait was drawn on, and the trace (`<prefix>.txt`).
fn write(prefix: &str, rows: &[Row], images: &[Vec<u8>], size: UVec2) {
    let dir = evidence();
    let (x0, y0, w, h) = CROP;
    let cell = (w * ZOOM, h * ZOOM + 16);
    let per = (SHEET.0 * SHEET.1) as usize;
    let keep = std::env::var("FACE_FRAMES_SHEETS").ok();
    for (sheet, chunk) in images.chunks(per).enumerate() {
        let name = format!("{prefix}-{sheet:02}.png");
        if keep
            .as_ref()
            .is_some_and(|kept| !kept.split(',').any(|sheet| sheet == name))
        {
            continue;
        }
        let mut out = Canvas::new(cell.0 * SHEET.0, cell.1 * SHEET.1);
        for (k, data) in chunk.iter().enumerate() {
            let frame = sheet * per + k;
            let (cx, cy) = ((k as u32 % SHEET.0) * cell.0, (k as u32 / SHEET.0) * cell.1);
            for y in 0..h * ZOOM {
                for x in 0..w * ZOOM {
                    let i = (((y0 + y / ZOOM) * size.x + x0 + x / ZOOM) * 4) as usize;
                    put(
                        &mut out,
                        cx + x,
                        cy + y,
                        [data[i], data[i + 1], data[i + 2], 255],
                    );
                }
            }
            let bar = if rows[frame].drawn {
                [40, 200, 60, 255]
            } else {
                [70, 70, 70, 255]
            };
            for y in 0..14 {
                for x in 0..cell.0 - 2 {
                    put(&mut out, cx + x, cy + h * ZOOM + 1 + y, bar);
                }
            }
            number(&mut out, cx + 4, cy + h * ZOOM, frame, [255, 255, 255, 255]);
        }
        out.save(&dir.join(name));
    }
    let mut text = String::from(
        "# frame dt_ms now_s drawn pixel_change said_file said_t | weights | bones name x y z qx qy qz qw | clips node weight seek | presented media_s\n",
    );
    for (frame, row) in rows.iter().enumerate() {
        let change = frame.checked_sub(1).map_or(0.0, |before| {
            pixel_change(&images[before], &images[frame], size.x)
        });
        let said = row
            .said
            .as_ref()
            .map_or("- -".to_owned(), |(file, t)| format!("{file} {t:.4}"));
        let _ = write!(
            text,
            "{frame} {:.3} {:.4} {} {change:.3} {said} |",
            row.dt * 1000.0,
            row.now,
            u8::from(row.drawn)
        );
        for w in &row.weights {
            let _ = write!(text, " {w:.4}");
        }
        text.push_str(" |");
        for (name, at, turn) in &row.bones {
            let _ = write!(
                text,
                " {name} {:.3} {:.3} {:.3} {:.4} {:.4} {:.4} {:.4}",
                at.x, at.y, at.z, turn.x, turn.y, turn.z, turn.w
            );
        }
        text.push_str(" |");
        for (node, weight, seek) in &row.clips {
            let _ = write!(text, " {node} {weight:.3} {seek:.4}");
        }
        let presented = row
            .presented
            .map_or("-".to_owned(), |position| format!("{position:.4}"));
        let _ = writeln!(text, " | {presented}");
    }
    std::fs::write(dir.join(format!("{prefix}.txt")), text).expect("the trace written");
}

/// The hero's pack's sounds with a lip sync: their index, face and voice event.
fn sounds(client: &Client) -> Vec<(usize, Arc<LineFace>, String)> {
    let world = client.world();
    let voice = world.resource::<crate::hero_voice::HeroVoice>();
    let resident = voice.resident().expect("the hero's voice pack");
    let faces = resident.faces();
    let mut out = Vec::new();
    for (index, face) in faces.iter().enumerate() {
        let Some(face) = face else { continue };
        let event = resident
            .rules
            .events
            .iter()
            .find(|e| e.sounds.contains(&index))
            .map(|e| e.name.clone());
        if let Some(event) = event {
            out.push((index, face.clone(), event));
        }
    }
    out
}

/// Starts the pack's sound `index` as the hero's voice starts a line: through the mixer, its
/// face said on the sound's clock.
fn say(client: &mut Client, (index, face, event): &(usize, Arc<LineFace>, String)) {
    use bevy::ecs::system::RunSystemOnce;
    let unit = client.me().id;
    let source = client
        .world()
        .resource::<crate::hero_voice::HeroVoice>()
        .resident()
        .expect("the hero's voice pack")
        .sounds[*index]
        .clone();
    let (face, event) = (face.clone(), event.clone());
    client
        .world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  mut mixer: ResMut<crate::mixer::Mixer>,
                  mut speech: ResMut<Speech>,
                  time: Res<Time>,
                  real: Res<Time<bevy::time::Real>>| {
                let request = crate::mixer::Request {
                    event: &event,
                    on_player: true,
                    channel: crate::channels::Channel::Voices,
                    ..default()
                };
                let player =
                    mixer.start_heard(&mut commands, time.elapsed(), &request, source.clone());
                let player = player.unwrap_or_else(|| panic!("the mixer plays {event}"));
                let clock = crate::audio_output::VoiceClock::new(1.0);
                commands.entity(player).insert(clock.clone());
                speech.say_heard(Some(unit), face.clone(), player, clock, real.elapsed_secs());
            },
        )
        .expect("the line started");
}

/// The client of a capture: Pudge on the map, his portrait drawn as a player's is, the voice
/// played through a fixture that presents its samples two buffers after each pull.
fn client(hero: HeroId, pacing: Pacing) -> (Server, Client) {
    let server = Server::start(1, &[]);
    let mut client = Client::join_rendered(&server, hero, &["--no-vsync"], prepare);
    client.cheat(Cheat::StartGame);
    client
        .world_mut()
        .insert_resource(crate::audio_output::DeviceBuffers {
            buffer: pacing.device_buffer(),
            ahead: 2,
            sample_rate: pacing.sample_rate(),
        });
    client
        .world_mut()
        .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            16,
        )));
    super::hud_layout::on_screen(&mut client);
    client.until("the portrait's face posed", |c| {
        c.world()
            .resource::<Stage>()
            .actor
            .as_ref()
            .is_some_and(|a| a.face.is_some())
    });
    (server, client)
}

fn voice(client: &mut Client, on: bool) {
    client
        .world_mut()
        .resource_mut::<crate::config::Settings>()
        .voice = on;
}

/// Every case of `pacing`, written with `prefix` (`before`, `after`).
fn captures(pacing: Pacing, prefix: &str) {
    let (_server, mut client) = client(HeroId::PUDGE, pacing);
    let p = pacing.name();
    voice(&mut client, false);
    let sounds = sounds(&client);
    let pick = |n: &str| {
        sounds
            .iter()
            .find(|(_, face, _)| face.file.ends_with(&format!("/{n}.txt")))
            .unwrap_or_else(|| panic!("line {n}"))
            .clone()
    };
    // Seconds of each case: a 240 Hz capture is four times the frames of a 60 Hz one.
    let scale = match pacing {
        Pacing::Hz240 => 0.3,
        _ => 1.0,
    };
    // (a) idle, no line.
    let (rows, images) = capture(&mut client, pacing, 10.0 * scale, Box::new(|_, _| {}));
    let size = client.world().resource::<Captured>().size;
    write(&format!("{prefix}-idle-{p}"), &rows, &images, size);
    // (b) lines of different kinds, among them lines that end at rest and drop their last face.
    let picks: Vec<_> = ["6", "25", "78", "98", "135", "159"]
        .iter()
        .map(|n| pick(n))
        .collect();
    let lines = picks.clone();
    let mut next = 0usize;
    let mut quiet = 0usize;
    let (rows, images) = capture(
        &mut client,
        pacing,
        24.0 * scale,
        Box::new(move |c, _| {
            let said = {
                let world = c.world();
                let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
                world
                    .resource::<Speech>()
                    .of(Some(c.me().id), now)
                    .is_some()
            };
            quiet = if said { 0 } else { quiet + 1 };
            if !said && quiet > 20 && next < lines.len() {
                say(c, &lines[next]);
                next += 1;
            }
        }),
    );
    write(&format!("{prefix}-lines-{p}"), &rows, &images, size);
    // (c) a line, another a fifth of a second after it, and a third taking over the second.
    let order = [pick("6"), pick("98"), pick("25")];
    let mut stage = 0usize;
    let mut quiet_since: Option<f32> = None;
    let (rows, images) = capture(
        &mut client,
        pacing,
        7.0,
        Box::new(move |c, _| {
            let (now, said) = {
                let world = c.world();
                let now = world.resource::<Time<bevy::time::Real>>().elapsed_secs();
                let said = world
                    .resource::<Speech>()
                    .of(Some(c.me().id), now)
                    .map(|(_, t)| t);
                (now, said)
            };
            match (stage, said) {
                (0, None) => {
                    say(c, &order[0]);
                    stage = 1;
                }
                (1, None) => {
                    let since = *quiet_since.get_or_insert(now);
                    if now - since >= 0.2 {
                        say(c, &order[1]);
                        stage = 2;
                    }
                }
                (2, Some(t)) if t >= 1.0 => {
                    say(c, &order[2]);
                    stage = 3;
                }
                _ => {}
            }
        }),
    );
    write(&format!("{prefix}-gaps-{p}"), &rows, &images, size);
    // (d) running, attacking and casting while the hero answers.
    voice(&mut client, true);
    let (rows, images) = capture(
        &mut client,
        pacing,
        8.0 * scale.max(0.5),
        Box::new(|c, frame| {
            let away = c.me().pos + At::from_ints(-800, -600);
            match frame {
                0 => c.core_mut().send_order_to(
                    None,
                    Order::Move {
                        target: Target::Pos(away),
                    },
                ),
                300 => {
                    let back = c.me().pos + At::from_ints(700, 500);
                    c.core_mut().send_order_to(
                        None,
                        Order::Move {
                            target: Target::Pos(back),
                        },
                    );
                }
                _ => {}
            }
        }),
    );
    write(&format!("{prefix}-action-{p}"), &rows, &images, size);
}

#[test]
#[ignore = "evidence capture, run by hand: writes the face's frames"]
fn capture_pudges_face_frame_by_frame() {
    let prefix = std::env::var("FACE_FRAMES_PREFIX").unwrap_or_else(|_| "before".to_owned());
    let pacings =
        std::env::var("FACE_FRAMES_PACINGS").unwrap_or_else(|_| "240hz,60hz,browser".to_owned());
    for pacing in [Pacing::Hz240, Pacing::Hz60, Pacing::Browser] {
        if pacings.split(',').any(|p| p == pacing.name()) {
            captures(pacing, &prefix);
        }
    }
}
