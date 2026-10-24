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
//! - `type:TEXT`: the text typed into the focused field (`Tab` moves on);
//! - `await:WHAT`: until `menu` covers the screen, a `game` is on it, a
//!   `popup` (a question or the match-found offer) is up or `nopopup` is not, or a button named `button:NAME`
//!   shows;
//! - `shot:FILE`: the screen saved under `--screenshot-dir`;
//! - `wait:SECONDS`.

use bevy::{
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
    window::PrimaryWindow,
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
    Type(String),
    Await(Awaited),
    Shot(String),
    Wait(f32),
}

#[derive(Clone, Debug, PartialEq)]
enum Awaited {
    Menu,
    Game,
    Popup,
    NoPopup,
    Button(String),
}

/// Where the script is.
#[derive(Resource, Default)]
struct Script {
    steps: Vec<(String, Step)>,
    at: usize,
    /// When the step under way began.
    began: Option<f32>,
    /// A key pressed last frame, to let go of now.
    held: Option<KeyCode>,
    /// The button to press after the pointer's own pass this frame.
    click: Option<String>,
    /// A screenshot asked for, until its file is there.
    shot: Option<std::path::PathBuf>,
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
                "type" => Step::Type(arg.to_owned()),
                "await" => Step::Await(match arg {
                    "menu" => Awaited::Menu,
                    "game" => Awaited::Game,
                    "popup" => Awaited::Popup,
                    "nopopup" => Awaited::NoPopup,
                    other => match other.strip_prefix("button:") {
                        Some(name) => Awaited::Button(name.to_owned()),
                        None => return Err(format!("cannot await '{other}'")),
                    },
                }),
                "shot" => Step::Shot(arg.to_owned()),
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

/// What the screen shows now, for the awaited steps.
fn holds(
    awaited: &Awaited,
    menu: Option<&MenuCore>,
    session: &Session,
    buttons: &Query<&Name, With<Button>>,
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
        Awaited::Button(name) => buttons.iter().any(|n| n.as_str() == name),
    }
}

/// Plays the step due: keys before Bevy reads the keyboard this frame.
#[allow(clippy::too_many_arguments)]
fn play(
    mut script: ResMut<Script>,
    time: Res<Time>,
    menu: Option<NonSend<MenuCore>>,
    session: NonSend<Session>,
    buttons: Query<&Name, With<Button>>,
    disabled: Query<&Name, With<crate::main_menu::Disabled>>,
    window: Query<Entity, With<PrimaryWindow>>,
    mut keys: MessageWriter<KeyboardInput>,
    mut commands: Commands,
    args: Res<crate::Args>,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(window) = window.single() else {
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
    if let Some(key) = script.held.take() {
        keys.write(typed(key, ButtonState::Released));
        return;
    }
    let now = time.elapsed_secs();
    let Some((line, step)) = script.steps.get(script.at).cloned() else {
        return;
    };
    let began = *script.began.get_or_insert(now);
    let done = match &step {
        Step::Key(key) => {
            keys.write(typed(*key, ButtonState::Pressed));
            script.held = Some(*key);
            true
        }
        // Once the button shows.
        Step::Click(name) if buttons.iter().any(|n| n.as_str() == name) => {
            script.click = Some(name.clone());
            true
        }
        Step::Type(text) => {
            for c in text.chars() {
                keys.write(KeyboardInput {
                    key_code: KeyCode::Unidentified(
                        bevy::input::keyboard::NativeKeyCode::Unidentified,
                    ),
                    logical_key: Key::Character(c.to_string().into()),
                    state: ButtonState::Pressed,
                    text: None,
                    repeat: false,
                    window,
                });
            }
            true
        }
        Step::Click(name) if disabled.iter().any(|n| n.as_str() == name) => {
            println!("script: {line} is disabled");
            exit.write(AppExit::from_code(2));
            true
        }
        Step::Click(_) => false,
        Step::Await(awaited) => holds(awaited, menu.as_deref(), &session, &buttons),
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
        Step::Wait(secs) => now - began >= *secs,
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
