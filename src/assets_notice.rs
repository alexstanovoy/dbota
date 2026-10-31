//! The screen of a client without Dota's assets. dbota draws Dota's art and plays its sounds
//! from the asset cache and from nothing else, so with no cache of this build (none made yet, or
//! one another dbota made: its stamp is not of this build's asset fingerprint) this screen is
//! all there is: no menu, no game, not even one the command line names. It imports the cache
//! from the local Dota 2 install with a click, the work `dbota import` does, its progress shown;
//! then a restart picks the new cache up. A player without Dota 2 uses a prepared cache instead
//! (a folder or a zip someone made with `dbota export-cache`): the button asks for its path and
//! `dbota import-cache`'s work follows. A run nobody watches (a capture, a check) has no screen
//! to show and ends with the reason (see `main`).

use std::sync::{
    Mutex,
    mpsc::{Receiver, channel},
};

use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::Ime,
};

use crate::import::Need;
use crate::ui::{self, Fonts};

/// What the work is: made from Dota 2's files, or taken from a prepared cache.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Way {
    Import,
    Load,
}

/// Where the import is.
#[derive(Clone, Debug, PartialEq)]
enum State {
    Needed(Need),
    /// The share done and the asset at hand.
    Working(Way, f32, String),
    Done(Way),
    Failed(Way, String),
}

/// What the import thread tells.
enum Told {
    Progress(f32, String),
    Finished(Result<bool, String>),
}

#[derive(Resource)]
struct Notice {
    state: State,
    told: Option<Mutex<Receiver<Told>>>,
    /// The path of a prepared cache being typed.
    entering: Option<String>,
}

/// Longest path typed or pasted, in characters.
const MAX_PATH: usize = 1024;

/// The screen.
#[derive(Component)]
struct Root;

/// What its button does.
#[derive(Component, Clone, Copy)]
enum Press {
    Import,
    /// Asks for the path of a prepared cache.
    Prepared,
    /// Takes the cache at the path typed.
    Load,
    Cancel,
    Restart,
    Quit,
}

/// Holds the client at this screen while its cache is missing or another build's.
pub fn plugin(app: &mut App) {
    let args = app.world().resource::<crate::Args>();
    let Some(need) = crate::import::need(&args.cache_dir) else {
        return;
    };
    app.insert_resource(Notice {
        state: State::Needed(need),
        told: None,
        entering: None,
    })
    .add_systems(Update, (typing, listen, presses, draw).chain());
}

/// Takes in what the import thread said since the last frame.
fn listen(mut notice: ResMut<Notice>) {
    let Some(told) = &notice.told else {
        return;
    };
    let way = match &notice.state {
        State::Working(way, ..) => *way,
        _ => Way::Import,
    };
    let mut last = None;
    if let Ok(told) = told.lock() {
        while let Ok(t) = told.try_recv() {
            last = Some(t);
        }
    }
    match last {
        Some(Told::Progress(done, item)) => notice.state = State::Working(way, done, item),
        Some(Told::Finished(result)) => {
            notice.told = None;
            notice.state = match result {
                Ok(true) => State::Done(way),
                Ok(false) => State::Failed(
                    way,
                    "imported, but some assets did not convert (the terminal names them); what \
                     they draw is missing"
                        .to_owned(),
                ),
                Err(why) => State::Failed(way, why),
            };
        }
        None => {}
    }
}

/// Starts `work` on a thread of its own; `notice` shows how far it is.
fn start(
    notice: &mut Notice,
    way: Way,
    work: impl FnOnce(&mut dyn FnMut(f32, &str)) -> Result<bool, String> + Send + 'static,
) {
    let (tell, told) = channel();
    std::thread::spawn(move || {
        let progress = tell.clone();
        let result = work(&mut |done, item| {
            let _ = progress.send(Told::Progress(done, item.to_owned()));
        });
        let _ = tell.send(Told::Finished(result));
    });
    notice.told = Some(Mutex::new(told));
    notice.state = State::Working(way, 0.0, String::new());
    notice.entering = None;
}

/// Starts taking the prepared cache at `path` (where `~/` is the home folder).
fn start_load(notice: &mut Notice, path: &str, cache: std::path::PathBuf) {
    let from = expand(path.trim());
    start(notice, Way::Load, move |progress| {
        crate::import::load_cache(&from, &cache, progress)
            .map(|()| true)
            .inspect_err(|why| eprintln!("dbota import-cache: {why}"))
    });
}

/// `path` with a leading `~/` meaning the home folder, as a shell would.
fn expand(path: &str) -> std::path::PathBuf {
    match (path.strip_prefix("~/"), std::env::var_os("HOME")) {
        (Some(rest), Some(home)) => std::path::PathBuf::from(home).join(rest),
        _ => std::path::PathBuf::from(path),
    }
}

fn presses(
    mut notice: ResMut<Notice>,
    buttons: Query<(&Interaction, &Press), Changed<Interaction>>,
    args: Res<crate::Args>,
    mut exit: MessageWriter<AppExit>,
) {
    for (interaction, press) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let busy = notice.told.is_some();
        match press {
            Press::Import if !busy => {
                let cache = args.cache_dir.clone();
                start(&mut notice, Way::Import, move |progress| {
                    crate::import::run_with(None, &cache, true, progress)
                });
            }
            Press::Prepared if !busy => notice.entering = Some(String::new()),
            Press::Load if !busy => {
                if let Some(path) = notice.entering.clone().filter(|p| !p.trim().is_empty()) {
                    start_load(&mut notice, &path, args.cache_dir.clone());
                }
            }
            Press::Import | Press::Prepared | Press::Load => {}
            Press::Cancel => notice.entering = None,
            Press::Quit => {
                exit.write(AppExit::Success);
            }
            Press::Restart => {
                let again = std::env::current_exe().and_then(|exe| {
                    std::process::Command::new(exe)
                        .args(std::env::args_os().skip(1))
                        .spawn()
                });
                match again {
                    Ok(_) => {
                        exit.write(AppExit::Success);
                    }
                    Err(why) => {
                        notice.state =
                            State::Failed(Way::Import, format!("cannot start dbota again: {why}"))
                    }
                }
            }
        }
    }
}

/// The most of a paste taken into the path, in characters.
const MAX_PASTE: usize = 4096;

/// Types the path of a prepared cache while it is asked for: keys, text an input method or a
/// script commits, and a paste (Ctrl+V, or Shift+Insert); Enter takes the cache there.
#[allow(clippy::too_many_arguments)]
fn typing(
    mut notice: ResMut<Notice>,
    mut events: MessageReader<KeyboardInput>,
    mut composing: MessageReader<Ime>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clipboard: ResMut<Clipboard>,
    mut pasting: Local<Option<ClipboardRead>>,
    args: Res<crate::Args>,
) {
    if notice.entering.is_none() || notice.told.is_some() {
        events.clear();
        composing.clear();
        *pasting = None;
        return;
    }
    let control = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut typed = String::new();
    let mut erase = 0;
    let mut enter = false;
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        if (control && event.key_code == KeyCode::KeyV)
            || (shift && event.key_code == KeyCode::Insert)
        {
            *pasting = Some(clipboard.fetch_text());
            continue;
        }
        match &event.logical_key {
            Key::Enter => enter = true,
            Key::Backspace => {
                if typed.pop().is_none() {
                    erase += 1;
                }
            }
            Key::Space => typed.push(' '),
            Key::Character(chars) if !control => typed.push_str(chars),
            _ => {}
        }
    }
    for event in composing.read() {
        if let Ime::Commit { value, .. } = event {
            typed.extend(value.chars().take(MAX_PASTE));
        }
    }
    if let Some(read) = pasting.as_mut()
        && let Some(result) = read.poll_result()
    {
        *pasting = None;
        if let Ok(text) = result {
            typed.extend(text.chars().take(MAX_PASTE));
        }
    }
    let Some(path) = notice.entering.as_mut() else {
        return;
    };
    for _ in 0..erase {
        path.pop();
    }
    path.extend(typed.chars().filter(|c| !c.is_control()));
    *path = path.chars().take(MAX_PATH).collect();
    if enter && !path.trim().is_empty() {
        let path = path.clone();
        start_load(&mut notice, &path, args.cache_dir.clone());
    }
}

/// The buttons that bring a cache, on a notice that asks for one.
const WAYS: [(&str, Press); 2] = [
    ("IMPORT FROM DOTA 2", Press::Import),
    ("USE A PREPARED CACHE", Press::Prepared),
];

/// The screen's title, what it says and its buttons.
fn held_words(state: &State) -> (&'static str, String, Vec<(&'static str, Press)>) {
    let quit = ("QUIT", Press::Quit);
    let fingerprint = dbota_import::import::fingerprint();
    match state {
        State::Working(Way::Import, done, item) => (
            "Importing from Dota 2",
            format!("{:.0} %  {item}", done * 100.0),
            vec![],
        ),
        State::Working(Way::Load, done, item) => (
            "Taking the prepared cache",
            format!("{:.0} %  {item}", done * 100.0),
            vec![],
        ),
        State::Done(Way::Import) => (
            "Dota 2 assets imported",
            "dbota reads them as it starts.".to_owned(),
            vec![("RESTART DBOTA", Press::Restart), quit],
        ),
        State::Done(Way::Load) => (
            "The prepared cache is in place",
            "dbota reads it as it starts.".to_owned(),
            vec![("RESTART DBOTA", Press::Restart), quit],
        ),
        State::Failed(Way::Import, why) => (
            "The import failed",
            why.clone(),
            vec![("TRY AGAIN", Press::Import), WAYS[1], quit],
        ),
        State::Failed(Way::Load, why) => (
            "The prepared cache was not taken",
            why.clone(),
            vec![WAYS[0], WAYS[1], quit],
        ),
        State::Needed(Need::Import) => (
            "No Dota 2 assets yet",
            "dbota draws Dota's heroes, map and sounds from your own Dota 2 install and from nothing \
             else. It converts them on this machine from your Dota 2 folder (about a minute). Without \
             Dota 2, use a cache prepared for this version by someone who has it."
                .to_owned(),
            vec![WAYS[0], WAYS[1], quit],
        ),
        State::Needed(Need::Reimport) => (
            "The assets are out of date for this version",
            format!(
                "They were imported by another dbota and may lack what this one draws \
                 (it needs assets {fingerprint}). Re-import them from your Dota 2 folder: \
                 about a minute, on this machine. Without Dota 2, use a cache prepared for \
                 this version."
            ),
            vec![("RE-IMPORT FROM DOTA 2", Press::Import), WAYS[1], quit],
        ),
    }
}

/// What is asked while the path is typed.
const ASK: &str = "The path of the prepared cache: a folder, or a .zip of one. Type or paste it.";
/// A button of the notice, named after its label for scripts (`click:quit dbota` for QUIT, which
/// the menu's own `quit` must not be taken for).
fn button(commands: &mut Commands, fonts: &Fonts, parent: Entity, label: &str, press: Press) {
    let name = match press {
        Press::Quit => "quit dbota".to_owned(),
        _ => label.to_lowercase(),
    };
    let button = commands
        .spawn((
            press,
            Button,
            Node {
                height: px(40),
                padding: UiRect::axes(px(18), px(0)),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(match press {
                Press::Quit => ui::PANEL_DEEP,
                _ => Color::srgb(0.12, 0.3, 0.14),
            }),
            Name::new(name),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        ui::text(label, &fonts.bold, 16.0, ui::INK),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// The title, text and buttons of the screen.
fn held_screen(notice: &Notice) -> (&'static str, String, Vec<(&'static str, Press)>) {
    if notice.entering.is_some() {
        return ("Use a prepared cache", ASK.to_owned(), ASKED.to_vec());
    }
    held_words(&notice.state)
}

/// The buttons of the question for a path.
const ASKED: [(&str, Press); 2] = [("LOAD", Press::Load), ("CANCEL", Press::Cancel)];

/// The box the path is typed in, with its caret.
fn path_field(commands: &mut Commands, fonts: &Fonts, parent: Entity, path: &str) {
    let field = commands
        .spawn((
            Node {
                width: percent(100),
                min_width: px(420),
                height: px(38),
                padding: UiRect::axes(px(10), px(0)),
                align_items: AlignItems::Center,
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(ui::PANEL_DEEP),
            BorderColor::all(ui::GOLDEN),
            Name::new("prepared cache path"),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        ui::text(format!("{path}|"), &fonts.regular, 17.0, ui::INK),
        Pickable::IGNORE,
        ChildOf(field),
    ));
}

/// Draws the screen: over everything, the menu, a game and the settings.
fn draw_held(commands: &mut Commands, fonts: &Fonts, notice: &Notice) {
    let (title, text, buttons) = held_screen(notice);
    let screen = commands
        .spawn((
            Root,
            crate::hud::Blocker,
            Interaction::default(),
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(18),
                ..ui::fill()
            },
            // Opaque: nothing of what it holds shows through.
            BackgroundColor(Color::srgb(0.02, 0.025, 0.035)),
            GlobalZIndex(90),
        ))
        .id();
    let title = commands
        .spawn((
            ui::text(title, &fonts.bold, 28.0, ui::GOLDEN),
            ChildOf(screen),
        ))
        .id();
    commands.entity(title).insert(Pickable::IGNORE);
    commands.spawn((
        ui::text(text, &fonts.regular, 17.0, ui::INK),
        Node {
            max_width: px(760),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(screen),
    ));
    if let Some(path) = &notice.entering {
        path_field(commands, fonts, screen, path);
    }
    let row = commands
        .spawn((
            Node {
                column_gap: px(14),
                ..default()
            },
            ChildOf(screen),
        ))
        .id();
    for (label, press) in buttons {
        button(commands, fonts, row, label, press);
    }
}

/// Draws the screen anew when what it says changes.
fn draw(
    mut commands: Commands,
    notice: Res<Notice>,
    fonts: Res<Fonts>,
    root: Query<Entity, With<Root>>,
    mut drawn: Local<Option<(State, Option<String>)>>,
) {
    let now = (notice.state.clone(), notice.entering.clone());
    if drawn.as_ref() == Some(&now) {
        return;
    }
    *drawn = Some(now);
    for entity in &root {
        commands.entity(entity).despawn();
    }
    draw_held(&mut commands, &fonts, &notice);
}
