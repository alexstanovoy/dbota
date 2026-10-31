//! A real game server and the whole client against it, as `main` builds the client, Bevy's
//! window and GPU left out: the app's frames run when a test asks for them.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use bevy::app::PluginsState;
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseButtonInput;
use bevy::prelude::*;
use bevy::window::{CursorMoved, PrimaryWindow};
use bota_client_core::{App as Core, Intent};
use clap::Parser;

use crate::session::Session;

/// How long a wait for the match lasts before the test calls it stuck: long enough for a machine
/// running many other jobs.
const PATIENCE: Duration = Duration::from_secs(240);
static NEXT: AtomicU32 = AtomicU32::new(0);

/// A game server of `seats` seats on Map 2, in lockstep (the match moves as fast as the client
/// takes it in), with cheats for the scenes the tests set up; ended when dropped, when the next
/// test's may start (see [`TURN`]).
pub struct Server {
    child: Child,
    // Read to the end by nobody, but open: a server whose pipe is closed fails on its next line.
    _output: BufReader<ChildStdout>,
    pub address: String,
    // Last: the server is gone before the next test's may start.
    _turn: std::sync::MutexGuard<'static, ()>,
}

impl Server {
    /// The server with `extra` options besides the defaults above, which they may set.
    pub fn start(seats: u8, extra: &[&str]) -> Server {
        let turn = TURN
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let seats = seats.to_string();
        let mut args = vec!["--map", "2", "--players", &seats];
        args.extend(["--bind", "127.0.0.1", "--port", "0", "--cheats"]);
        let defaults = [
            ("--mode", "lockstep"),
            ("--pick-secs", "1"),
            ("--tactical-secs", "0"),
        ];
        for (option, value) in defaults {
            if !extra.contains(&option) {
                args.extend([option, value]);
            }
        }
        let mut child = Command::new(crate::main_menu::tests::server_binary())
            .args(args)
            .args(extra)
            .stdout(Stdio::piped())
            .spawn()
            .expect("bota_server starts");
        let mut output = BufReader::new(child.stdout.take().expect("piped"));
        let address = loop {
            let mut line = String::new();
            let read = output.read_line(&mut line).expect("the server's output");
            assert!(read > 0, "the server ended before it listened");
            if let Some(address) = line.trim().strip_prefix("bota_server listening on ") {
                break address.to_owned();
            }
        };
        Server {
            child,
            _output: output,
            address,
            _turn: turn,
        }
    }
}

impl Server {
    /// Waits for the server to end by itself, as it does once its match is over.
    pub fn wait(&mut self) {
        let _ = self.child.wait();
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A scratch folder for the client's settings, emptied when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "target/e2e/{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The client, all of it but the window and the GPU.
pub struct Client {
    pub app: App,
    _scratch: Scratch,
}

/// One match at a time in the process, held by its [`Server`]: each client loads the whole
/// asset cache on the process's few IO threads, and a dozen at once starve one another (models
/// not drawn for many seconds). A test holds one server at a time. One that failed holding the
/// turn poisons it; the next is no worse for that.
static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());

impl Client {
    /// The client joined to `server`'s seat, run with `extra` command-line arguments, its hero
    /// picked: the match is under way and the hero on the map when it returns.
    pub fn join(server: &Server, hero: bota_proto::HeroId, extra: &[&str]) -> Client {
        let mut client = Client::pick(server, hero, extra);
        client.until("the hero on the map", |c| c.core().ears().is_some());
        client
    }

    /// The client joined to `server`'s seat, run with `extra` command-line arguments, its hero
    /// picked: it returns once the pick is past (in the strategy phase, where the server has
    /// one).
    pub fn pick(server: &Server, hero: bota_proto::HeroId, extra: &[&str]) -> Client {
        let mut args = vec!["--addr", server.address.as_str()];
        args.extend_from_slice(extra);
        let mut client = Client::run(&args);
        client.until("the hero picked", |c| {
            let core = c.core_mut();
            if core.view().is_none() && core.my_slot.is_some() && !core.ready {
                core.act(Intent::PickHero(hero.0));
                core.act(Intent::ToggleReady);
            }
            core.in_tactical_phase() || core.ears().is_some()
        });
        client
    }

    /// The client run with `extra` command-line arguments, its settings in a folder of its own,
    /// in a window of 1920 by 1080 unless they say another, as `main` builds it (with no sound device: see `audio_output`).
    pub fn run(extra: &[&str]) -> Client {
        let scratch = Scratch::new();
        let config = scratch.0.join("client.conf");
        let config = config.to_str().expect("a UTF-8 path");
        let mut line = vec!["dbota", "--config", config];
        if !extra.contains(&"--window") {
            line.extend(["--window", "1920x1080"]);
        }
        line.extend_from_slice(extra);
        let args = crate::Args::parse_from(line);
        assert!(
            crate::import::need(&args.cache_dir).is_none(),
            "{} holds no asset cache of this build: DBOTA_ASSETS names one `dbota import` made",
            args.cache_dir.display()
        );
        let store = crate::main_menu::SettingsStore::new(Some(config.into()));
        let mut settings = store.load_settings();
        for line in &args.set {
            let (name, value) = line.split_once('=').expect("--set NAME=VALUE");
            settings.set(name.trim(), value.trim()).expect("a setting");
        }
        let menu_first = args.wants_menu();
        let session = if menu_first {
            crate::main_menu::idle()
        } else {
            crate::session::open(&args).expect("the game the arguments name")
        };
        roomy_io_threads();
        let mut app = App::new();
        crate::client_resources(
            &mut app,
            &args,
            &settings,
            Some(config.into()),
            &store,
            session,
        );
        app.add_plugins(headless(&args, &settings));
        crate::client_plugins(&mut app);
        crate::menu(&mut app, &args, store, menu_first).expect("the menu");
        // What the renderer would take of the entities drawn: kept, as no render world takes it.
        app.add_plugins(bevy::render::sync_world::SyncWorldPlugin);
        while app.plugins_state() == PluginsState::Adding {
            bevy::tasks::tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();
        Client {
            app,
            _scratch: scratch,
        }
    }

    /// Runs frames until `done` holds, within [`PATIENCE`].
    pub fn until(&mut self, what: &str, done: impl FnMut(&mut Client) -> bool) {
        self.within(PATIENCE, what, done);
    }

    /// Runs frames until `done` holds, within `patience`: for what follows at once from what
    /// the test did, so a failure is told without waiting out [`PATIENCE`].
    pub fn within(
        &mut self,
        patience: Duration,
        what: &str,
        mut done: impl FnMut(&mut Client) -> bool,
    ) {
        let deadline = Instant::now() + patience;
        while !done(self) {
            assert!(Instant::now() < deadline, "{what}: not within {patience:?}");
            self.app.update();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// Runs `n` frames.
    pub fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.app.update();
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn world(&self) -> &World {
        self.app.world()
    }

    pub fn world_mut(&mut self) -> &mut World {
        self.app.world_mut()
    }

    pub fn core(&self) -> &Core {
        &self.world().non_send::<Session>().0
    }

    pub fn core_mut(&mut self) -> &mut Core {
        &mut self.world_mut().non_send_mut::<Session>().into_inner().0
    }

    /// The tick the client's view stands at.
    pub fn tick(&self) -> u32 {
        self.core().view().map_or(0, |v| v.tick)
    }

    /// The view of the match the client stands at.
    pub fn view(&self) -> &bota_proto::WorldView {
        self.core().view().expect("a view of the match")
    }

    /// One's own hero, as the view tells it.
    pub fn me(&self) -> &bota_proto::UnitView {
        let id = self.core().ears().expect("one's hero");
        self.view().unit(id).expect("one's hero in the view")
    }

    /// The entity drawing a unit of the view.
    pub fn body(&self, id: bota_proto::EntityId) -> Option<Entity> {
        self.world()
            .resource::<crate::units::UnitIndex>()
            .0
            .get(&id)
            .copied()
    }

    /// Asks the server for a cheat, as the console's lines do, and waits for the next tick.
    pub fn cheat(&mut self, cheat: bota_proto::Cheat) {
        self.core_mut().act(Intent::Cheat(cheat));
        let tick = self.tick();
        self.until("the tick after a cheat", |c| c.tick() > tick + 1);
    }

    /// The window's pixel a world point (bota's x, y, on the ground) is drawn at, if the camera
    /// sees it.
    pub fn screen_of(&mut self, (x, y): (f32, f32)) -> Option<Vec2> {
        let ground = self.world().resource::<crate::map::Ground>().at(x, y);
        let world = self.world_mut();
        let (camera, place) = world
            .query_filtered::<(&Camera, &GlobalTransform), With<crate::camera::RtsCamera>>()
            .single(world)
            .expect("the game's camera");
        camera
            .world_to_viewport(place, Vec3::new(x, ground, -y))
            .ok()
    }

    /// Moves the pointer to a pixel of the window, as the window reports it.
    pub fn mouse(&mut self, at: Vec2) {
        let world = self.world_mut();
        let (window, mut win) = world
            .query_filtered::<(Entity, &mut Window), With<PrimaryWindow>>()
            .single_mut(world)
            .expect("the primary window");
        win.set_cursor_position(Some(at));
        world.write_message(CursorMoved {
            window,
            position: at,
            delta: None,
        });
        self.frames(2);
    }

    /// Presses and lets go of a mouse button where the pointer is, a frame apart.
    pub fn click(&mut self, button: MouseButton) {
        self.button(button, ButtonState::Pressed);
        self.button(button, ButtonState::Released);
    }

    /// Presses or lets go of a mouse button where the pointer is, and runs a frame.
    pub fn button(&mut self, button: MouseButton, state: ButtonState) {
        let world = self.world_mut();
        let window = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .expect("the primary window");
        world.write_message(MouseButtonInput {
            button,
            state,
            window,
        });
        self.frames(1);
    }

    /// Presses or lets go of a key, as the window reports it, and runs a frame.
    pub fn key(&mut self, key_code: KeyCode, state: ButtonState) {
        let world = self.world_mut();
        let window = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .expect("the primary window");
        world.write_message(KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window,
        });
        self.frames(1);
    }

    /// The lines laid out for the GPU this frame: each line strip of the map's lines (rings,
    /// ranges, order marks), its points and its colour.
    pub fn line_strips(&self) -> Vec<(Vec<Vec3>, LinearRgba)> {
        let world = self.world();
        let handles = world.resource::<bevy::gizmos::GizmoHandles>();
        let Some(Some(handle)) = handles
            .handles()
            .get(&std::any::TypeId::of::<DefaultGizmoConfigGroup>())
        else {
            return Vec::new();
        };
        let assets = world.resource::<Assets<bevy::gizmos::GizmoAsset>>();
        let Some(asset) = assets.get(handle) else {
            return Vec::new();
        };
        let buffer = asset.buffer();
        let mut strips = Vec::new();
        let mut points = Vec::new();
        let mut colour = LinearRgba::NONE;
        for (at, c) in buffer.strip_positions.iter().zip(&buffer.strip_colors) {
            if at.is_nan() {
                if !points.is_empty() {
                    strips.push((std::mem::take(&mut points), colour));
                }
                continue;
            }
            colour = *c;
            points.push(*at);
        }
        if !points.is_empty() {
            strips.push((points, colour));
        }
        strips
    }

    /// Every text of the interface that shows: visible and laid out (a node of `Display::None`,
    /// or under one, is laid out as nothing).
    pub fn shown_texts(&mut self) -> Vec<String> {
        let world = self.world_mut();
        world
            .query::<(&Text, &InheritedVisibility, &ComputedNode)>()
            .iter(world)
            .filter(|(_, shown, laid)| shown.get() && laid.size() != Vec2::ZERO)
            .map(|(text, ..)| text.0.clone())
            .filter(|text| !text.is_empty())
            .collect()
    }
}

/// A second seat, held by a client with no front end that gives no orders: it acknowledges
/// what it is sent, so a lockstep match moves on. Joined first, it takes the first seat.
pub fn quiet_seat(server: &Server, hero: bota_proto::HeroId) {
    scripted_seat(server, hero, |_| Vec::new());
}

/// A second seat, held by a client with no front end that gives the orders `decide` gives it
/// from each view it is sent.
pub fn scripted_seat(
    server: &Server,
    hero: bota_proto::HeroId,
    decide: impl FnMut(&bota_client_core::ClientSession) -> Vec<bota_client_core::IssuedOrder>
    + Send
    + 'static,
) {
    let (mut link, seated) =
        bota_client_core::Link::join(&server.address, "quiet", hero, None).expect("a seat");
    std::thread::spawn(move || {
        let _ = bota_client_core::play(&mut link, seated, &mut (), decide);
    });
}

/// A second seat held by a client with no front end that hunts wards: its hero stands at `at`,
/// plants a Sentry Ward there to see the other side's wards, and attacks the first one it
/// sees. Joined first, it takes the first seat.
pub fn ward_hunter_seat(server: &Server, hero: bota_proto::HeroId, at: bota_proto::Vec2) {
    use bota_proto::{Cheat, ItemId, ItemSlot, Order, Target, UnitKind};
    let (mut link, seated) =
        bota_client_core::Link::join(&server.address, "hunter", hero, None).expect("a seat");
    let mut stage = 0;
    std::thread::spawn(move || {
        let _ = bota_client_core::play(&mut link, seated, &mut (), |session| {
            let issue = bota_client_core::IssuedOrder::hero;
            let Some(me) = session.own_hero() else {
                return Vec::new();
            };
            let sentry = me
                .items
                .iter()
                .position(|item| item.is_some_and(|item| item.id == ItemId::SENTRY_WARD));
            let ward = session.current().and_then(|view| {
                view.units
                    .iter()
                    .find(|unit| unit.kind == UnitKind::Ward && unit.team != me.team)
                    .map(|unit| unit.id)
            });
            match (stage, sentry, ward) {
                (0, _, _) => {
                    stage = 1;
                    vec![
                        issue(Order::Cheat {
                            cheat: Cheat::Teleport { at },
                        }),
                        issue(Order::Cheat {
                            cheat: Cheat::Item {
                                item: ItemId::SENTRY_WARD,
                            },
                        }),
                    ]
                }
                (1, Some(slot), _) => {
                    stage = 2;
                    vec![issue(Order::Use {
                        slot: ItemSlot(slot as u8),
                        target: Target::Pos(at),
                    })]
                }
                (2, _, Some(ward)) => {
                    stage = 3;
                    vec![issue(Order::Attack {
                        target: Target::Unit(ward),
                    })]
                }
                _ => Vec::new(),
            }
        });
    });
}

/// A ring's middle on the ground and its radius, in bota's units (x, y).
pub fn ring_of(points: &[Vec3]) -> (Vec2, f32) {
    let flat: Vec<Vec2> = points.iter().map(|p| Vec2::new(p.x, -p.z)).collect();
    // A closed strip repeats its first point last.
    let around = &flat[..flat.len() - 1];
    let middle = around.iter().copied().sum::<Vec2>() / around.len() as f32;
    let radius = around.iter().map(|p| p.distance(middle)).sum::<f32>() / around.len() as f32;
    (middle, radius)
}

/// The asset loaders' threads with the stacks `main` gives them (see `io_pool`): this crate's own
/// loaders run unoptimised in a test build, where two clients loading at once overflowed the
/// default 2 MiB. The pools are the process's, made once by the first app.
fn roomy_io_threads() {
    static MADE: std::sync::Once = std::sync::Once::new();
    MADE.call_once(|| crate::io_pool::make(4));
}

/// Bevy's plugins as `main` sets them up, without the window system (no window is opened; the
/// primary window is an entity of the size asked for) and without a GPU (no renderer is made:
/// what would be drawn stays in the main world, where the tests read it).
fn headless(args: &crate::Args, settings: &crate::config::Settings) -> impl PluginGroup {
    use bevy::render::settings::WgpuSettings;
    crate::default_plugins(args, settings)
        .build()
        .disable::<bevy::winit::WinitPlugin>()
        .disable::<bevy::log::LogPlugin>()
        .set(bevy::render::RenderPlugin {
            render_creation: WgpuSettings {
                backends: None,
                ..default()
            }
            .into(),
            ..default()
        })
}

impl Client {
    /// Carries the camera with one's own hero, as a second tap on its portrait does, and waits
    /// for it to be there.
    pub fn follow_me(&mut self) {
        let slot = self.core().my_slot.expect("a seat");
        self.core_mut().pinned = Some(bota_client_core::Pin::Hero(slot));
        self.within(Duration::from_secs(20), "the camera on one's hero", |c| {
            let Some(body) = c.core().ears().and_then(|id| c.body(id)) else {
                return false;
            };
            let at = c
                .world()
                .get::<crate::units::Unit>(body)
                .expect("a unit")
                .at;
            let world = c.world_mut();
            let focus = world
                .query::<&crate::camera::RtsCamera>()
                .single(world)
                .expect("the game's camera")
                .focus;
            Vec2::new(focus.x, -focus.z).distance(at) < 50.0
        });
    }

    /// Puts one's hero at a point of the map (the cheat's teleport) with the camera on it.
    pub fn stand_at(&mut self, at: (i32, i32)) {
        self.cheat(bota_proto::Cheat::Teleport {
            at: bota_proto::Vec2::from_ints(at.0, at.1),
        });
        self.follow_me();
    }
}

/// The middle of the mid lane, where it crosses the river: open ground away from every
/// building's guns.
pub const MID: (i32, i32) = (8000, 9600);

impl Client {
    /// The middle of an interface node on the window, in the window's pixels (the layout's
    /// physical pixels: the window is of scale factor 1).
    pub fn centre_of(&self, node: Entity) -> Option<Vec2> {
        let placed = self.world().get::<bevy::ui::UiGlobalTransform>(node)?;
        Some(placed.translation)
    }

    /// The box of an item slot of the bottom panel (inventory, backpack, stash, town portal)
    /// that shows.
    pub fn item_box(&mut self, slot: u8) -> Option<Entity> {
        let world = self.world_mut();
        world
            .query::<(Entity, &crate::hud::ItemBox, &InheritedVisibility)>()
            .iter(world)
            .find(|(_, b, shown)| b.0 == slot && shown.get())
            .map(|(e, _, _)| e)
    }

    /// The bag slot of an item one's hero carries.
    pub fn slot_of(&self, item: bota_proto::ItemId) -> Option<u8> {
        (0..bota_client_core::TOWN_PORTAL_SLOT)
            .find(|s| self.core().item_in(*s).is_some_and(|i| i.id == item))
    }
}
