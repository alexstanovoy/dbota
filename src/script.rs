//! Debug: `--script "STEP;STEP;…"` plays a person at the keys and the mouse,
//! for the end-to-end tests of the menu and the way out of a game. Keys go in
//! as the window reports them (through the bindings, as a person's do), and
//! a click lands on the menu button of a name as the pointer would make it.
//! Each step done is printed (`script: STEP`); one that waits longer than
//! [`PATIENCE`] ends the client with exit code 2.
//!
//! Steps:
//! - `key:NAME`: a key pressed and let go (`Escape`, `Enter`, `F1`: the
//!   settings file's names);
//! - `click:NAME`: the button of that name once it shows (the label in
//!   lower case, or the name the menu gives it: `quit`, `ok`, `cancel`,
//!   `return`, `disconnect`, `reconnect`; in a game `pick ready` and
//!   `pick <hero>`); a button shown dimmed (`READY` alone in a lobby)
//!   ends the client with exit code 2 at once;
//! - `await:WHAT`: until `menu` covers the screen, a `game` is on it, a
//!   `popup` (a question or the match-found offer) is up or `nopopup` is not, or a button named `button:NAME`
//!   shows, or `nobutton:NAME` when none of that name does (hidden or not there);
//! - `await:seats:R,D`: the top bar holds R places of Radiant and D of Dire;
//! - `await:horn`: the game clock past the horn;
//! - `await:model:NAME`: a unit drawn with the cache model of that logical name;
//! - `await:loot:NAME`: an item lying on the ground, drawn with the cache model of that logical name;
//! - `await:heard:PREFIX`: a bound effect sounded a sound of that logical prefix at least once;
//! - `await:started:NAME`: the particle effect of that logical name started at least once;
//! - `await:items:N`: exactly N items in the hero's bag;
//! - `expect:WHAT`: what `await` names, held at once, else the client ends with exit code 2;
//! - `await:towershots:N`: exactly N lines drawn from a tower to a hero it is set on;
//! - `await:ping:LOW,HIGH`: the ping the top right corner tells, in milliseconds, between the two;
//! - `await:level:N`, `await:gold:N`: one's hero at that level, that gold;
//! - `say:LINE`: the line typed into the chat and sent (a `-command` runs);
//! - `pick:ID`: the hero of that id picked, and the seat readied;
//! - `shot:FILE`: the screen saved under `--screenshot-dir`;
//! - `await:daylight:day` or `await:daylight:night`: the light fully turned
//!   to that time of day;
//! - `light`: prints `light: sun=R,G,B,LUX ambient=R,G,B,BRIGHTNESS`, the
//!   scene's sun and ambient light in linear colour, for the tests to compare;
//! - `wait:SECONDS`;
//! - `exit`: the client ends, with exit code 0;
//! - `mouse:X,Y`: the pointer moved to that point of the window, in shares
//!   of its width and height (`0.5,0.5` is the middle), so a script plays
//!   the same at any size;
//! - `lclick`, `rclick`: the left or right button pressed and let go where
//!   the pointer is;
//! - `ldown`, `lup`: the left button held down and let go, to drag;
//! - `down:NAME`, `up:NAME`: a key held (a modifier for the clicks that
//!   follow) and let go;
//! - `text:TEXT`: text typed (through the input method's commit, as any
//!   script's text comes in), into the focused field of the menu or the chat
//!   line (`key:Tab` moves between fields).

use bevy::{
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::{CursorMoved, Ime, PrimaryWindow},
};

use crate::main_menu::MenuCore;
use crate::session::Session;

/// Seconds any step may wait for what it waits for.
const PATIENCE: f32 = 120.0;

/// One step.
#[derive(Clone, Debug, PartialEq)]
enum Step {
    Key(KeyCode),
    Click(String),
    Await(Awaited),
    /// What `Await` waits for, held at once or the client ends with exit code 2.
    Expect(Awaited),
    Shot(String),
    /// The scene's lights printed.
    Light,
    Wait(f32),
    Mouse(Vec2),
    Button(MouseButton),
    /// The left button held down, or let go: a drag.
    Drag(ButtonState),
    /// The client ends, with exit code 0.
    Exit,
    Down(KeyCode),
    Up(KeyCode),
    Text(String),
    /// A line typed into the chat and sent.
    Say(String),
    /// A hero picked, by id, and the seat readied.
    Pick(u16),
}

/// A key or a mouse button pressed last frame, to let go of now.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Held {
    Key(KeyCode),
    Mouse(MouseButton),
}

#[derive(Clone, Debug, PartialEq)]
enum Awaited {
    Menu,
    Game,
    Popup,
    NoPopup,
    Button(String),
    /// No button of that name on the screen: none made, or one hidden.
    NoButton(String),
    /// The top bar holds this many places of Radiant and of Dire.
    Seats(u8, u8),
    /// The game clock past the horn.
    Horn,
    /// One's hero at this level or above.
    Level(u8),
    /// One's gold at this or above.
    Gold(i32),
    /// Exactly this many items in the hero's bag.
    Items(usize),
    /// The light fully turned to night (`true`) or to day.
    Daylight(bool),
    /// A unit drawn with the model of this logical name.
    Model(String),
    /// An item on the ground drawn with the model of this logical name.
    Loot(String),
    /// A bound effect sounded a sound of this logical prefix.
    Heard(String),
    /// The particle effect of this logical name started at least once.
    Started(String),
    /// Exactly this many tower-to-hero lines drawn.
    TowerShots(u32),
    /// The ping the corner tells, in milliseconds, within this range.
    Ping(u32, u32),
}

/// What the game has drawn, for the awaited steps that look at it.
#[derive(bevy::ecs::system::SystemParam)]
struct Drawn<'w, 's> {
    units: Query<'w, 's, &'static crate::units::Unit>,
    loot: Query<'w, 's, &'static crate::loot::Loot>,
    bank: Res<'w, crate::units::ModelBank>,
    library: Option<Res<'w, crate::particles::Library>>,
    playing: Res<'w, crate::particles::Playing>,
    heard: Res<'w, crate::fx::Heard>,
    shots: Res<'w, crate::overlay::TowerShots>,
    corner: Query<'w, 's, &'static Text, With<crate::hud::FrameRate>>,
}

/// Where the script is.
#[derive(Resource, Default)]
struct Script {
    steps: Vec<(String, Step)>,
    at: usize,
    /// When the step under way began.
    began: Option<f32>,
    held: Option<Held>,
    /// The button to press after the pointer's own pass this frame.
    click: Option<String>,
    /// A screenshot asked for, until its file is there.
    shot: Option<std::path::PathBuf>,
}

/// What an `await:` or `expect:` step names, or what is wrong with it.
fn awaited(arg: &str) -> Result<Awaited, String> {
    Ok(match arg {
        "menu" => Awaited::Menu,
        "game" => Awaited::Game,
        "popup" => Awaited::Popup,
        "nopopup" => Awaited::NoPopup,
        "horn" => Awaited::Horn,
        other => {
            let (what, value) = other.split_once(':').unwrap_or((other, ""));
            match what {
                "button" => Awaited::Button(value.to_owned()),
                "seats" => {
                    let bad = || format!("'{value}' is no pair of seat counts");
                    let (r, d) = value.split_once(',').ok_or_else(bad)?;
                    Awaited::Seats(r.parse().map_err(|_| bad())?, d.parse().map_err(|_| bad())?)
                }
                "nobutton" => Awaited::NoButton(value.to_owned()),
                "model" => Awaited::Model(value.to_owned()),
                "loot" => Awaited::Loot(value.to_owned()),
                "heard" => Awaited::Heard(value.to_owned()),
                "started" => Awaited::Started(value.to_owned()),
                "level" => Awaited::Level(
                    value
                        .parse()
                        .map_err(|_| format!("'{value}' is no level"))?,
                ),
                "gold" => {
                    Awaited::Gold(value.parse().map_err(|_| format!("'{value}' is no gold"))?)
                }
                "daylight" => Awaited::Daylight(match value {
                    "night" => true,
                    "day" => false,
                    _ => return Err(format!("'{value}' is neither day nor night")),
                }),
                "ping" => value
                    .split_once(',')
                    .and_then(|(low, high)| {
                        Some(Awaited::Ping(low.parse().ok()?, high.parse().ok()?))
                    })
                    .ok_or_else(|| format!("'{value}' is not LOW,HIGH"))?,
                "towershots" => Awaited::TowerShots(
                    value
                        .parse()
                        .map_err(|_| format!("'{value}' is no count"))?,
                ),
                "items" => Awaited::Items(
                    value
                        .parse()
                        .map_err(|_| format!("'{value}' is no count"))?,
                ),
                _ => return Err(format!("cannot await '{other}'")),
            }
        }
    })
}

/// The steps of `text`, or what is wrong with it.
fn parse(text: &str) -> Result<Vec<(String, Step)>, String> {
    text.split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|line| {
            let (what, arg) = line.split_once(':').unwrap_or((line, ""));
            let step = match what {
                "key" => Step::Key(
                    crate::keys::key_named(arg).ok_or_else(|| format!("no key named '{arg}'"))?,
                ),
                "click" => Step::Click(arg.to_owned()),
                "await" => Step::Await(awaited(arg)?),
                "expect" => Step::Expect(awaited(arg)?),
                "shot" => Step::Shot(arg.to_owned()),
                "light" => Step::Light,
                "exit" => Step::Exit,
                "mouse" => Step::Mouse(
                    arg.split_once(',')
                        .and_then(|(x, y)| Some(Vec2::new(x.parse().ok()?, y.parse().ok()?)))
                        .filter(|at| at.cmpge(Vec2::ZERO).all() && at.cmple(Vec2::ONE).all())
                        .ok_or_else(|| format!("'{arg}' is not X,Y within 0 to 1"))?,
                ),
                "lclick" => Step::Button(MouseButton::Left),
                "rclick" => Step::Button(MouseButton::Right),
                "ldown" => Step::Drag(ButtonState::Pressed),
                "lup" => Step::Drag(ButtonState::Released),
                "down" | "up" => {
                    let key = crate::keys::key_named(arg)
                        .ok_or_else(|| format!("no key named '{arg}'"))?;
                    if what == "down" {
                        Step::Down(key)
                    } else {
                        Step::Up(key)
                    }
                }
                "text" => Step::Text(arg.to_owned()),
                "say" => Step::Say(arg.to_owned()),
                "pick" => Step::Pick(arg.parse().map_err(|_| format!("'{arg}' is no hero id"))?),
                "wait" => Step::Wait(
                    arg.parse()
                        .map_err(|_| format!("'{arg}' is not a number of seconds"))?,
                ),
                _ => return Err(format!("no step '{line}'")),
            };
            Ok((line.to_owned(), step))
        })
        .collect()
}

pub fn plugin(app: &mut App) {
    let Some(text) = app.world().resource::<crate::Args>().script.clone() else {
        return;
    };
    let steps = match parse(&text) {
        Ok(steps) => steps,
        Err(why) => {
            eprintln!("dbota: --script: {why}");
            std::process::exit(2);
        }
    };
    app.insert_resource(Script { steps, ..default() })
        .add_systems(PreUpdate, play.before(InputSystems))
        .add_systems(PreUpdate, press.after(bevy::ui::UiSystems::Focus));
}

/// What the awaited steps look at on the screen: the buttons that are there and the top bar's places.
#[derive(bevy::ecs::system::SystemParam)]
struct Screen<'w, 's> {
    buttons:
        Query<'w, 's, (&'static Name, &'static Node, &'static InheritedVisibility), With<Button>>,
    columns: Query<'w, 's, (&'static crate::hud::SeatColumn, &'static Node)>,
}

/// What the screen shows now, for the awaited steps.
fn holds(
    awaited: &Awaited,
    menu: Option<&MenuCore>,
    session: &Session,
    screen: &Screen,
    daylight: &crate::daylight::Daylight,
    drawn: &Drawn,
) -> bool {
    let view = menu.map(MenuCore::view);
    match awaited {
        Awaited::Menu => view.as_ref().is_some_and(|v| v.covers()),
        Awaited::Game => {
            let core = &session.0;
            menu.is_none_or(MenuCore::shows_game)
                && (core.view().is_some() || core.my_slot.is_some())
        }
        Awaited::Popup => view
            .as_ref()
            .is_some_and(|v| v.confirm.is_some() || v.offer.is_some()),
        Awaited::NoPopup => view.as_ref().is_some_and(|v| v.confirm.is_none()),
        Awaited::Button(name) => screen.buttons.iter().any(|(n, ..)| n.as_str() == name),
        Awaited::NoButton(name) => !screen.buttons.iter().any(|(n, node, seen)| {
            n.as_str() == name && node.display != Display::None && seen.get()
        }),
        Awaited::Seats(radiant, dire) => {
            crate::hud::seats_shown(&screen.columns) == (*radiant, *dire)
        }
        Awaited::Horn => {
            let core = &session.0;
            core.view()
                .is_some_and(|view| view.game_tick() >= core.pregame_ticks())
        }
        Awaited::Level(level) => own(session).is_some_and(|me| me.level >= *level),
        Awaited::Items(count) => {
            let core = &session.0;
            core.view().is_some()
                && (0..bota_client_core::BAG_SLOTS)
                    .filter(|slot| core.item_in(*slot).is_some())
                    .count()
                    == *count
        }
        Awaited::Ping(low, high) => drawn
            .corner
            .iter()
            .filter_map(|text| crate::hud::ping_told(&text.0))
            .any(|ping| (*low..=*high).contains(&ping)),
        Awaited::TowerShots(count) => session.0.view().is_some() && drawn.shots.0 == *count,
        Awaited::Daylight(night) => daylight.night == f32::from(u8::from(*night)),
        Awaited::Gold(gold) => own(session).is_some_and(|me| me.gold.is_some_and(|g| g >= *gold)),
        Awaited::Model(name) => drawn
            .units
            .iter()
            .filter_map(|unit| drawn.bank.0.get(unit.model?))
            .any(|entry| entry.model.logical == *name),
        Awaited::Loot(name) => drawn
            .loot
            .iter()
            .any(|loot| loot.model.as_deref() == Some(name.as_str())),
        Awaited::Heard(prefix) => drawn.heard.0.get(prefix).is_some_and(|n| *n > 0),
        Awaited::Started(name) => drawn
            .library
            .as_deref()
            .and_then(|library| library.effect(name))
            .is_some_and(|effect| drawn.playing.started(effect) > 0),
    }
}

/// One's own row of the scoreboard.
fn own(session: &Session) -> Option<&bota_proto::PlayerView> {
    let core = &session.0;
    core.view()?.player(core.my_slot?)
}

/// The scene's light, for the steps that wait for it or print it.
#[derive(bevy::ecs::system::SystemParam)]
struct Light<'w, 's> {
    daylight: Res<'w, crate::daylight::Daylight>,
    sun: Query<'w, 's, &'static DirectionalLight, With<crate::map::MainSun>>,
    ambient: Res<'w, GlobalAmbientLight>,
}

/// What the scene shows, for the steps that wait for it or print it.
#[derive(bevy::ecs::system::SystemParam)]
struct Scene<'w, 's> {
    light: Light<'w, 's>,
    drawn: Drawn<'w, 's>,
}

/// Plays the step due: keys before Bevy reads the keyboard this frame.
#[allow(clippy::too_many_arguments)]
fn play(
    mut script: ResMut<Script>,
    time: Res<Time>,
    menu: Option<NonSend<MenuCore>>,
    mut session: NonSendMut<Session>,
    screen: Screen,
    disabled: Query<&Name, With<crate::main_menu::Disabled>>,
    mut window: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut mouse: MessageWriter<MouseButtonInput>,
    mut moved: MessageWriter<CursorMoved>,
    mut ime: MessageWriter<Ime>,
    mut commands: Commands,
    args: Res<crate::Args>,
    mut exit: MessageWriter<AppExit>,
    mut said: ResMut<crate::chat::Queued>,
    scene: Scene,
) {
    let Ok((window, mut win)) = window.single_mut() else {
        return;
    };
    let typed = |key_code: KeyCode, state: ButtonState| KeyboardInput {
        key_code,
        logical_key: match key_code {
            KeyCode::Escape => Key::Escape,
            KeyCode::Enter => Key::Enter,
            KeyCode::Tab => Key::Tab,
            KeyCode::Backspace => Key::Backspace,
            _ => Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        },
        state,
        text: None,
        repeat: false,
        window,
    };
    match script.held.take() {
        Some(Held::Key(key)) => {
            keys.write(typed(key, ButtonState::Released));
            return;
        }
        Some(Held::Mouse(button)) => {
            mouse.write(MouseButtonInput {
                button,
                state: ButtonState::Released,
                window,
            });
            return;
        }
        None => {}
    }
    let now = time.elapsed_secs();
    let Some((line, step)) = script.steps.get(script.at).cloned() else {
        return;
    };
    let began = *script.began.get_or_insert(now);
    let done = match &step {
        Step::Key(key) => {
            keys.write(typed(*key, ButtonState::Pressed));
            script.held = Some(Held::Key(*key));
            true
        }
        Step::Down(key) => {
            keys.write(typed(*key, ButtonState::Pressed));
            true
        }
        Step::Up(key) => {
            keys.write(typed(*key, ButtonState::Released));
            true
        }
        Step::Mouse(share) => {
            let position = *share * win.size();
            win.set_cursor_position(Some(position));
            moved.write(CursorMoved {
                window,
                position,
                delta: None,
            });
            true
        }
        Step::Button(button) => {
            mouse.write(MouseButtonInput {
                button: *button,
                state: ButtonState::Pressed,
                window,
            });
            script.held = Some(Held::Mouse(*button));
            true
        }
        Step::Exit => {
            exit.write(AppExit::Success);
            true
        }
        Step::Drag(state) => {
            mouse.write(MouseButtonInput {
                button: MouseButton::Left,
                state: *state,
                window,
            });
            true
        }
        Step::Text(text) => {
            ime.write(Ime::Commit {
                window,
                value: text.clone(),
            });
            true
        }
        // Once the button shows.
        Step::Click(name) if screen.buttons.iter().any(|(n, ..)| n.as_str() == name) => {
            script.click = Some(name.clone());
            true
        }
        Step::Click(name) if disabled.iter().any(|n| n.as_str() == name) => {
            println!("script: {line} is disabled");
            exit.write(AppExit::from_code(2));
            true
        }
        Step::Click(_) => false,
        Step::Await(awaited) => holds(
            awaited,
            menu.as_deref(),
            &session,
            &screen,
            &scene.light.daylight,
            &scene.drawn,
        ),
        Step::Expect(awaited) => {
            if !holds(
                awaited,
                menu.as_deref(),
                &session,
                &screen,
                &scene.light.daylight,
                &scene.drawn,
            ) {
                println!("script: {line} does not hold");
                exit.write(AppExit::from_code(2));
            }
            true
        }
        Step::Shot(file) => match &script.shot {
            Some(path) => path.is_file(),
            None => {
                let path = args.screenshot_dir.join(file);
                if let Some(dir) = path.parent() {
                    let _ = std::fs::create_dir_all(dir);
                }
                let _ = std::fs::remove_file(&path);
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path.clone()));
                script.shot = Some(path);
                false
            }
        },
        Step::Light => {
            let sun = scene.light.sun.single().ok();
            let [sr, sg, sb, _] = sun.map_or([0.0; 4], |s| s.color.to_linear().to_f32_array());
            let lux = sun.map_or(0.0, |s| s.illuminance);
            let [ar, ag, ab, _] = scene.light.ambient.color.to_linear().to_f32_array();
            println!(
                "light: sun={sr:.4},{sg:.4},{sb:.4},{lux:.1} ambient={ar:.4},{ag:.4},{ab:.4},{:.1}",
                scene.light.ambient.brightness
            );
            true
        }
        Step::Wait(secs) => now - began >= *secs,
        Step::Pick(hero) => {
            session.0.act(bota_client_core::Intent::PickHero(*hero));
            if !session.0.ready {
                session.0.act(bota_client_core::Intent::ToggleReady);
            }
            true
        }
        Step::Say(line) => {
            said.0.push_back(line.clone());
            true
        }
    };
    if done {
        println!("script: {line}");
        script.at += 1;
        script.began = None;
        script.shot = None;
    } else if now - began > PATIENCE {
        println!("script: timed out at {line}");
        exit.write(AppExit::from_code(2));
    }
}

/// Presses the button the step names, after the pointer's own pass, as a
/// click on it would.
fn press(mut script: ResMut<Script>, mut buttons: Query<(&Name, &mut Interaction), With<Button>>) {
    let Some(name) = script.click.take() else {
        return;
    };
    if let Some((_, mut interaction)) = buttons.iter_mut().find(|(n, _)| n.as_str() == name) {
        *interaction = Interaction::Pressed;
    }
}
