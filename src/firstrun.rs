//! The first run, before the sign-in screen. dbota draws Dota's art and plays its sounds from a
//! private copy of the player's own Dota 2 files (the asset cache) and from nothing else, so a
//! client without a copy of this build (none made yet, or one another dbota made) shows this
//! screen and nothing else: it finds Dota 2 in the computer's Steam libraries by itself (or the
//! player picks the folder in the system's dialog), makes the copy on worker threads while the
//! window keeps drawing (the count, the group at hand, CANCEL), and then starts dbota again on
//! the new copy, at the sign-in screen. dbota needs the player's own Dota 2 (free on Steam): its
//! files are never taken from anywhere else. The settings' RE-IMPORT GAME FILES brings the screen
//! back (`--reimport`). A run nobody watches (a capture, a check) has no screen to
//! show and ends with the reason (see `main`).
//!
//! Every import is made beside the cache and takes its place only when whole: a failed, stopped
//! or closed one leaves the cache that was there, never half of one.
//!
//! Why dbota starts again rather than going on in the same window: everything the client reads
//! from the cache (the manifest, the texts, the shop, the fonts, the map's lighting, the sounds)
//! is read once as it starts, by some forty systems at startup; starting again on the new copy is
//! the one path that reads all of it exactly as every later start does. On Linux and macOS it is
//! the same process (exec); on Windows a new one.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{Receiver, Sender, channel},
};

use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    picking::{events::PointerPress, hover::Hovered, pointer::PointerButton},
    prelude::*,
    ui_widgets::Button,
    window::Ime,
};
use dbota_import::import::Flow;
use heatel::locate::Install;

use crate::desktop::NoDialog;
use crate::import::{Imported, Need};
use crate::ui::{self, Fonts};

/// Whether the client starts at this screen: the player asked for a re-import, or the cache is
/// missing or another build's.
pub fn held(args: &crate::Args) -> Option<Need> {
    if args.reimport {
        Some(Need::Requested)
    } else {
        crate::import::need(&args.cache_dir)
    }
}

pub fn plugin(app: &mut App) {
    app.add_observer(reimport_pressed);
    let args = app.world().resource::<crate::Args>();
    let Some(need) = held(args) else {
        return;
    };
    let run = FirstRun::new(need, args);
    app.insert_resource(run)
        .add_observer(presses)
        .add_systems(Update, (typing, listen, restart, draw).chain());
}

/// What the player is doing on the screen.
#[derive(Clone, Debug, PartialEq)]
enum Screen {
    /// The Steam libraries are being searched.
    Looking,
    /// What was found is shown, with the ways to a copy.
    Ready,
    /// The system's folder dialog is open.
    Choosing,
    /// The system has no dialog: the path is typed.
    Typing,
    /// The copy is being made.
    Working(Work),
    /// CANCEL was pressed: the worker is ending, its half copy removed.
    Stopping,
    /// The copy is in place, some assets missing: CONTINUE or TRY AGAIN.
    Partial { files: usize, failed: usize },
    /// dbota starts again on the copy.
    Restarting,
}

/// How far the copy is.
#[derive(Clone, Debug, PartialEq)]
struct Work {
    done: usize,
    total: usize,
    item: String,
    bytes: u64,
}

/// What a worker thread tells the screen.
enum Told {
    Found(Vec<Install>),
    Picked(Result<Option<PathBuf>, NoDialog>),
    Progress(Work),
    Imported(Result<Imported, String>),
}

/// The screen's state and its workers.
#[derive(Resource)]
struct FirstRun {
    need: Need,
    screen: Screen,
    /// The install the import reads: the first found, or the folder the player picked.
    install: Option<Install>,
    /// A line under the text: why the last pick, import or copy did not work out.
    note: Option<String>,
    /// The path being typed (no system dialog).
    typed: String,
    tell: Sender<Told>,
    told: Mutex<Receiver<Told>>,
    /// Set to stop the copy under way; its worker checks it before each step.
    cancel: Arc<AtomicBool>,
    /// The worker making a copy, joined when the client ends (see `Drop`).
    worker: Option<std::thread::JoinHandle<()>>,
    /// Debug (`--dialog-answer`): the answers the dialogs give instead of opening.
    answers: Vec<PathBuf>,
    /// Debug (`--no-dialog`): the dialogs answer as on a system without one.
    no_dialog: bool,
    /// Debug (`--import-hold`): a copy waits before its first step until this file exists.
    hold: Option<PathBuf>,
    cache: PathBuf,
}

/// How long a client that is closed mid-copy waits for its worker to remove the half copy.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

impl Drop for FirstRun {
    /// A window closed mid-copy stops the worker and waits (a bounded while) for it to remove
    /// what it made: the cache that was there stays, no half copy beside it.
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        let Some(worker) = self.worker.take() else {
            return;
        };
        let ended = std::time::Instant::now();
        while !worker.is_finished() && ended.elapsed() < STOP_WAIT {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if worker.is_finished() {
            let _ = worker.join();
        }
    }
}

impl FirstRun {
    fn new(need: Need, args: &crate::Args) -> FirstRun {
        let (tell, told) = channel();
        let run = FirstRun {
            need,
            screen: Screen::Looking,
            install: None,
            note: None,
            typed: String::new(),
            tell,
            told: Mutex::new(told),
            cancel: Arc::new(AtomicBool::new(false)),
            worker: None,
            answers: args.dialog_answer.clone(),
            no_dialog: args.no_dialog,
            hold: args.import_hold.clone(),
            cache: args.cache_dir.clone(),
        };
        let (tell, steam) = (run.tell.clone(), args.steam_folder.clone());
        std::thread::spawn(move || {
            let found = if steam.is_empty() {
                crate::desktop::installs()
            } else {
                heatel::locate::find_installs(&steam)
            };
            let _ = tell.send(Told::Found(found));
        });
        run
    }

    /// Opens the system's folder dialog on a thread of its own (it blocks until closed).
    fn ask(&mut self) {
        self.note = None;
        self.screen = Screen::Choosing;
        let tell = self.tell.clone();
        if self.no_dialog {
            let _ = tell.send(Told::Picked(Err(NoDialog::Missing)));
            return;
        }
        if !self.answers.is_empty() {
            let answer = self.answers.remove(0);
            let _ = tell.send(Told::Picked(Ok(Some(answer))));
            return;
        }
        std::thread::spawn(move || {
            let answer = crate::desktop::ask_folder("Choose the Dota 2 folder (dota 2 beta)");
            let _ = tell.send(Told::Picked(answer));
        });
    }

    /// Takes the folder the player picked or typed.
    fn picked(&mut self, path: PathBuf) {
        self.screen = Screen::Ready;
        match heatel::locate::install_in(&path) {
            Some(install) => self.install = Some(install),
            None => self.note = Some(crate::import::not_dota(&path)),
        }
    }

    /// Starts `work` on a worker thread; it is told how far it is and whether to stop.
    fn start(&mut self, work: impl FnOnce(&Sender<Told>, &dyn Fn() -> bool) + Send + 'static) {
        self.note = None;
        self.cancel.store(false, Ordering::Relaxed);
        self.screen = Screen::Working(Work {
            done: 0,
            total: 0,
            item: String::new(),
            bytes: 0,
        });
        let (tell, cancel, hold) = (self.tell.clone(), self.cancel.clone(), self.hold.clone());
        self.worker = Some(std::thread::spawn(move || {
            let stopped = || cancel.load(Ordering::Relaxed);
            // Debug: the copy waits at its start until the test lifts the barrier (or CANCEL).
            while let Some(hold) = &hold
                && !hold.exists()
                && !stopped()
            {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            work(&tell, &stopped);
        }));
    }

    fn import(&mut self) {
        let Some(install) = self.install.clone() else {
            return;
        };
        let cache = self.cache.clone();
        self.start(move |tell, stopped| {
            let mut progress = |p: &dbota_import::import::Progress<'_>| {
                let _ = tell.send(Told::Progress(Work {
                    done: p.done,
                    total: p.total,
                    item: p.item.to_owned(),
                    bytes: p.bytes_written,
                }));
                if stopped() {
                    Flow::Stop
                } else {
                    Flow::Continue
                }
            };
            let made = if stopped() {
                Ok(Imported {
                    stopped: true,
                    ..Imported::default()
                })
            } else {
                crate::import::import_from(&install, &cache, true, &mut progress)
            };
            let _ = tell.send(Told::Imported(made));
        });
    }

    /// What a worker said since the last frame.
    fn heard(&mut self, told: Told) {
        match told {
            Told::Found(installs) => {
                if self.install.is_none() {
                    self.install = installs.into_iter().next();
                }
                if self.screen == Screen::Looking {
                    self.screen = Screen::Ready;
                }
            }
            Told::Picked(Ok(Some(path))) => self.picked(path),
            Told::Picked(Ok(None)) => self.screen = Screen::Ready,
            Told::Picked(Err(NoDialog::Missing)) => {
                self.typed.clear();
                self.screen = Screen::Typing;
            }
            Told::Picked(Err(NoDialog::Failed(why))) => {
                self.screen = Screen::Ready;
                self.note = Some(format!("The folder dialog failed: {why}"));
            }
            Told::Progress(work) => {
                if matches!(self.screen, Screen::Working(_)) {
                    self.screen = Screen::Working(work);
                }
            }
            Told::Imported(made) => {
                self.worker = None;
                self.screen = match made {
                    Ok(imported) if imported.stopped => self.ended("Import cancelled."),
                    Err(why) => {
                        warn!("dbota import: {why}");
                        self.ended(&format!("The import failed: {why}"))
                    }
                    Ok(imported) if imported.failed.is_empty() => Screen::Restarting,
                    Ok(imported) => Screen::Partial {
                        files: imported.files,
                        failed: imported.failed.len(),
                    },
                };
            }
        }
    }

    /// Back to the ways to a copy, `note` said under them.
    fn ended(&mut self, note: &str) -> Screen {
        self.note = Some(note.to_owned());
        Screen::Ready
    }
}

/// What a button does.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    Import,
    ChooseDota,
    /// Takes the path typed.
    Take,
    Back,
    Cancel,
    Continue,
    /// Leaves a requested re-import for dbota as it was.
    Keep,
    Quit,
}

/// The settings' button that brings this screen back.
#[derive(Component)]
pub struct ReimportButton;

/// The settings' RE-IMPORT GAME FILES: dbota starts again at this screen.
fn reimport_pressed(
    event: On<PointerPress>,
    buttons: Query<(), With<ReimportButton>>,
    script: Option<Res<crate::script::Script>>,
    mut exit: MessageWriter<AppExit>,
) {
    if event.button == PointerButton::Primary && buttons.contains(event.entity) {
        let mut args = restart_args(script.as_deref());
        args.push("--reimport".into());
        request_restart(args, &mut exit);
    }
}

/// The arguments dbota starts again with: this run's, without `--reimport`, and with the
/// script's steps not played yet in place of its `--script`.
fn restart_args(script: Option<&crate::script::Script>) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    let mut given = std::env::args_os().skip(1);
    while let Some(arg) = given.next() {
        if arg == "--reimport" {
            continue;
        }
        if arg == "--script" {
            given.next();
            continue;
        }
        if arg.to_str().is_some_and(|a| a.starts_with("--script=")) {
            continue;
        }
        args.push(arg);
    }
    if let Some(script) = script {
        args.push("--script".into());
        args.push(script.rest().into());
    }
    args
}

/// Ends this run so that `main` starts dbota again with `args` once the window is closed.
fn request_restart(args: Vec<OsString>, exit: &mut MessageWriter<AppExit>) {
    if crate::RESTART.set(args).is_ok() {
        exit.write(AppExit::Success);
    }
}

fn presses(
    event: On<PointerPress>,
    mut run: ResMut<FirstRun>,
    buttons: Query<&Press>,
    script: Option<Res<crate::script::Script>>,
    mut exit: MessageWriter<AppExit>,
) {
    if event.button != PointerButton::Primary {
        return;
    }
    let Ok(press) = buttons.get(event.entity) else {
        return;
    };
    match (*press, run.screen.clone()) {
        (Press::Import, Screen::Ready | Screen::Partial { .. }) => run.import(),
        (Press::ChooseDota, Screen::Ready) => run.ask(),
        (Press::Take, Screen::Typing) => take_typed(&mut run),
        (Press::Back, Screen::Typing) => run.screen = Screen::Ready,
        (Press::Cancel, Screen::Working(_)) => {
            run.cancel.store(true, Ordering::Relaxed);
            run.screen = Screen::Stopping;
        }
        (Press::Continue, Screen::Partial { .. }) => run.screen = Screen::Restarting,
        (Press::Keep, Screen::Ready) => request_restart(restart_args(script.as_deref()), &mut exit),
        (Press::Quit, _) => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// Takes the typed path.
fn take_typed(run: &mut FirstRun) {
    let typed = run.typed.trim().to_owned();
    if !typed.is_empty() {
        run.picked(expand(&typed));
    }
}

/// `path` with a leading `~/` meaning the home folder, as a shell would.
fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), crate::desktop::home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// Takes in what the workers said since the last frame.
fn listen(mut run: ResMut<FirstRun>) {
    let told: Vec<Told> = match run.told.lock() {
        Ok(told) => told.try_iter().collect(),
        Err(_) => Vec::new(),
    };
    for told in told {
        run.heard(told);
    }
}

/// Starts dbota again on the new copy, once the screen has said so.
fn restart(
    run: Res<FirstRun>,
    script: Option<Res<crate::script::Script>>,
    mut exit: MessageWriter<AppExit>,
    mut shown: Local<bool>,
) {
    if run.screen != Screen::Restarting {
        *shown = false;
        return;
    }
    // One frame shows "Starting dbota" first.
    if !std::mem::replace(&mut *shown, true) {
        return;
    }
    request_restart(restart_args(script.as_deref()), &mut exit);
}

/// The longest path typed or pasted, in characters.
const MAX_PATH: usize = 1024;
/// The most of a paste taken into the path, in characters.
const MAX_PASTE: usize = 4096;

/// Types the path while the system has no dialog to ask with: keys, text an input method or a
/// script commits, and a paste (Ctrl+V, or Shift+Insert); Enter takes it.
fn typing(
    mut run: ResMut<FirstRun>,
    mut events: MessageReader<KeyboardInput>,
    mut composing: MessageReader<Ime>,
    keys: Res<ButtonInput<KeyCode>>,
    mut clipboard: ResMut<Clipboard>,
    mut pasting: Local<Option<ClipboardRead>>,
) {
    if run.screen != Screen::Typing {
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
    for _ in 0..erase {
        run.typed.pop();
    }
    run.typed.extend(typed.chars().filter(|c| !c.is_control()));
    run.typed = run.typed.chars().take(MAX_PATH).collect();
    if enter {
        take_typed(&mut run);
    }
}

/// What dbota is, for a player who has just started it.
const WHAT: &str = "dbota is the client of bota, an open-source simulator of Dota 2 matches: play \
                    against bots and people with Dota 2's look.";

/// The promise the import keeps.
const PRIVACY: &str = "Your Dota 2 files never leave your computer: dbota reads them on this \
                       machine and keeps the converted copy in a folder of its own.";

/// Where a player finds the folder of their install.
const WHERE: &str = "In Steam, right-click Dota 2 → Manage → Browse local files: that is the \
                     \"dota 2 beta\" folder.";

/// What the screen shows: its title, its lines, a note of what went wrong, and its buttons.
struct Shown {
    title: String,
    lines: Vec<String>,
    note: Option<String>,
    /// The share of the work done, for the bar.
    bar: Option<f32>,
    buttons: Vec<(&'static str, Press)>,
}

const QUIT: (&str, Press) = ("QUIT", Press::Quit);

/// The title and the opening line for what the copy is needed for.
fn need_words(need: Need) -> (String, String) {
    match need {
        Need::Import => (
            "Welcome to dbota".to_owned(),
            "It needs your own Dota 2 install (free on Steam): it makes a private copy of \
             Dota's heroes, map and sounds once, on this computer, in about a minute."
                .to_owned(),
        ),
        Need::Reimport => (
            "The assets are out of date for this version: re-import them from your Dota 2 \
             folder"
                .to_owned(),
            format!(
                "They were imported by another dbota and may lack what this one draws (it \
                 needs assets {}). It takes about a minute.",
                dbota_import::import::fingerprint()
            ),
        ),
        Need::Requested => (
            "Re-import the game files".to_owned(),
            "The copy is made again from your Dota 2 folder (after a Dota update, or when \
             something looks wrong); the one in place stays until the new one is whole."
                .to_owned(),
        ),
    }
}

/// The buttons beside an install found or not: the import, another folder, back.
fn ready_buttons(run: &FirstRun) -> Vec<(&'static str, Press)> {
    let mut buttons = match run.install {
        Some(_) => vec![
            ("IMPORT FROM DOTA 2", Press::Import),
            ("CHOOSE ANOTHER FOLDER", Press::ChooseDota),
        ],
        None => vec![("CHOOSE THE DOTA 2 FOLDER", Press::ChooseDota)],
    };
    if run.need == Need::Requested {
        buttons.push(("BACK TO DBOTA", Press::Keep));
    }
    buttons.push(QUIT);
    buttons
}

/// What the screen says and offers now.
fn shown(run: &FirstRun) -> Shown {
    let (title, lead) = need_words(run.need);
    let mut lines = match run.need {
        Need::Import => vec![WHAT.to_owned(), lead, PRIVACY.to_owned()],
        Need::Reimport | Need::Requested => vec![lead, PRIVACY.to_owned()],
    };
    let mut bar = None;
    let buttons = match &run.screen {
        Screen::Looking => {
            lines.push("Looking for Dota 2 in this computer's Steam libraries...".to_owned());
            vec![QUIT]
        }
        Screen::Ready => {
            match &run.install {
                Some(install) => lines.push(format!("Dota 2 found: {}", install.folder.display())),
                None => lines.push(format!(
                    "Dota 2 was not found in this computer's Steam libraries. Install it with \
                     Steam (it is free) and start dbota again; if it is installed in a folder \
                     Steam does not list, choose that folder. {WHERE}"
                )),
            }
            ready_buttons(run)
        }
        Screen::Choosing => {
            lines.push("Choose the folder in the window that opened...".to_owned());
            vec![QUIT]
        }
        Screen::Typing => {
            lines.push(format!(
                "The path of the Dota 2 folder (dota 2 beta). Type or paste it, then Enter. {WHERE}"
            ));
            lines.push(format!("{}|", run.typed));
            vec![("USE THIS PATH", Press::Take), ("BACK", Press::Back)]
        }
        Screen::Working(work) => {
            let what = "Making your private copy";
            let group = work.item.split(['.', '/']).next().unwrap_or("");
            lines.push(match work.total {
                0 => format!("{what}: reading the files..."),
                total => format!("{what}: {} of {total}  ·  {group}", work.done),
            });
            lines.push(if work.bytes > 0 {
                format!("{} ({} MB written)", work.item, work.bytes >> 20)
            } else {
                work.item.clone()
            });
            bar = Some(work.done as f32 / work.total.max(1) as f32);
            vec![("CANCEL", Press::Cancel)]
        }
        Screen::Stopping => {
            lines.push("Stopping; what was made so far is removed...".to_owned());
            vec![]
        }
        Screen::Partial { files, failed } => {
            lines.push(format!(
                "Imported {files} files. {failed} assets could not be converted and are \
                 missing (the log names them); dbota plays without them."
            ));
            vec![
                ("CONTINUE", Press::Continue),
                ("IMPORT AGAIN", Press::Import),
                QUIT,
            ]
        }
        Screen::Restarting => {
            lines.push("Your Dota 2 files are ready. Starting dbota...".to_owned());
            vec![]
        }
    };
    Shown {
        title,
        lines,
        note: run.note.clone(),
        bar,
        buttons,
    }
}

/// The screen.
#[derive(Component)]
struct Root;

/// A button of the screen, named after its label for scripts (`click:quit dbota` for QUIT, which
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
            Hovered::default(),
            Node {
                height: px(54),
                padding: UiRect::axes(px(22), px(0)),
                flex_shrink: 0.0,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(match press {
                Press::Quit | Press::Back | Press::Cancel | Press::Keep => ui::PANEL_DEEP,
                _ => Color::srgb(0.12, 0.3, 0.14),
            }),
            Name::new(name),
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        ui::text(label, &fonts.bold, 21.0, ui::INK),
        Pickable::IGNORE,
        ChildOf(button),
    ));
}

/// The colour of what went wrong.
const NOTE: Color = Color::srgb(1.0, 0.62, 0.45);

/// The bar of the work done.
fn progress_bar(commands: &mut Commands, parent: Entity, share: f32) {
    let track = commands
        .spawn((
            Node {
                width: px(720),
                height: px(14),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.1, 0.11, 0.13)),
            BorderColor::all(Color::srgb(0.3, 0.32, 0.36)),
            Name::new("import progress"),
            Pickable::IGNORE,
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Node {
            width: percent(share.clamp(0.0, 1.0) * 100.0),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::srgb(0.25, 0.6, 0.3)),
        Pickable::IGNORE,
        ChildOf(track),
    ));
}

/// Draws the screen: over everything there is.
fn draw_screen(commands: &mut Commands, fonts: &Fonts, shown: &Shown) {
    let screen = commands
        .spawn((
            Root,
            crate::hud::Blocker,
            Hovered::default(),
            Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                row_gap: px(16),
                padding: UiRect::all(px(24)),
                ..ui::fill()
            },
            // Opaque: nothing of what it holds shows through.
            BackgroundColor(Color::srgb(0.02, 0.025, 0.035)),
            GlobalZIndex(90),
        ))
        .id();
    commands.spawn((
        ui::text(shown.title.clone(), &fonts.bold, 40.0, ui::GOLDEN),
        Node {
            max_width: px(1200),
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(screen),
    ));
    for line in &shown.lines {
        commands.spawn((
            ui::text(line.clone(), &fonts.regular, 25.0, ui::INK),
            Node {
                max_width: px(1080),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(screen),
        ));
    }
    if let Some(note) = &shown.note {
        commands.spawn((
            ui::text(note.clone(), &fonts.regular, 25.0, NOTE),
            Node {
                max_width: px(1080),
                ..default()
            },
            Pickable::IGNORE,
            ChildOf(screen),
        ));
    }
    if let Some(share) = shown.bar {
        progress_bar(commands, screen, share);
    }
    let row = commands
        .spawn((
            Node {
                column_gap: px(14),
                row_gap: px(10),
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ChildOf(screen),
        ))
        .id();
    for (label, press) in &shown.buttons {
        button(commands, fonts, row, label, *press);
    }
}

/// Draws the screen anew when what it shows changes.
fn draw(
    mut commands: Commands,
    run: Res<FirstRun>,
    fonts: Res<Fonts>,
    root: Query<Entity, With<Root>>,
    mut drawn: Local<Option<(String, Vec<String>, Option<String>, Vec<&'static str>)>>,
) {
    let shown = shown(&run);
    let now = (
        shown.title.clone(),
        shown.lines.clone(),
        shown.note.clone(),
        shown.buttons.iter().map(|(label, _)| *label).collect(),
    );
    if drawn.as_ref() == Some(&now) {
        return;
    }
    *drawn = Some(now);
    for entity in &root {
        commands.entity(entity).despawn();
    }
    draw_screen(&mut commands, &fonts, &shown);
}
