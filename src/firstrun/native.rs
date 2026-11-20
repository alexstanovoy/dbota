//! The first run on a desktop: Dota 2 looked for in the computer's Steam libraries, the system's
//! folder dialog (or a typed path where there is none), the copy made on a worker thread, and
//! dbota started again (`main`) once it is whole.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;

use bevy::{
    clipboard::{Clipboard, ClipboardRead},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    window::Ime,
};
use dbota_import::cache_dir::Gate;
use dbota_import::import::Flow;

use super::{FirstRun, Made, Press, Screen, Told, Work};
use crate::desktop::NoDialog;

/// Where a copy is made from: a Dota 2 install.
pub(super) type Source = heatel::locate::Install;
/// The folder picked in the dialog, nothing when it was closed, or why there was no dialog.
pub(super) type Picked = Result<Option<PathBuf>, NoDialog>;

pub(super) const FOUND: &str = "DOTA 2 FOUND";
pub(super) const NOT_FOUND: &str = "DOTA 2 NOT FOUND";
pub(super) const PRIVACY: &str = "Your Dota 2 files never leave your computer: dbota reads them on this \
                           machine and keeps the converted copy in a folder of its own.";

/// The line under [`NOT_FOUND`].
pub(super) fn not_found_line() -> String {
    format!(
        "It was not found in this computer's Steam libraries. Install it with Steam (it is free) \
         and start dbota again; if it is installed in a folder Steam does not list, choose that \
         folder. {}",
        super::WHERE
    )
}

/// An install as the screen shows it: its folder.
pub(super) fn shown_source(install: &Source) -> String {
    install.folder.display().to_string()
}

/// QUIT, which a desktop window has.
pub(super) fn quit_or_nothing() -> Vec<(&'static str, Press)> {
    vec![("QUIT", Press::Quit)]
}

/// The buttons while the dialog is open: it blocks until closed, so only QUIT.
pub(super) fn choosing_buttons() -> Vec<(&'static str, Press)> {
    quit_or_nothing()
}

/// What only the desktop keeps.
pub(super) struct Side {
    /// Ends the copy under way: its worker publishes nothing once it was cancelled, and a copy's
    /// cancel finds out whether it came too late. One per copy (see `import`).
    gate: Arc<Gate>,
    /// The worker making a copy, joined when the client ends (see `Drop`).
    worker: Option<std::thread::JoinHandle<()>>,
    /// Debug (`--dialog-answer`): the answers the dialogs give instead of opening.
    answers: Vec<PathBuf>,
    /// Debug (`--no-dialog`): the dialogs answer as on a system without one.
    no_dialog: bool,
    /// Debug (`--import-hold`): a copy waits before its first step until this file exists.
    hold: Option<PathBuf>,
    /// Debug (`--import-hold-at`): a copy waits at this step until it is cancelled.
    hold_at: Option<HoldAt>,
    cache: PathBuf,
}

/// Debug (`--import-hold-at`): the step of a copy at which it waits until CANCEL is pressed (or
/// the window closed), after the screen was told of the step and before it is made. The step is
/// then made all the same, and the cancel is looked at only after it: a cancel that arrives
/// while a step works, which a test cannot time otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldAt {
    /// The step with this number (the screen's count of steps done).
    Step(usize),
    /// The last step.
    Last,
}

impl std::str::FromStr for HoldAt {
    type Err = String;

    fn from_str(text: &str) -> Result<HoldAt, String> {
        match text {
            "last" => Ok(HoldAt::Last),
            number => number
                .parse()
                .map(HoldAt::Step)
                .map_err(|_| format!("'{text}' is neither a step number nor 'last'")),
        }
    }
}

impl HoldAt {
    fn is(self, step: usize, total: usize) -> bool {
        match self {
            HoldAt::Step(number) => step == number,
            HoldAt::Last => step + 1 == total,
        }
    }
}

/// How long a client that is closed mid-copy waits for its worker to remove the half copy.
const STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(10);

impl Drop for Side {
    /// A window closed mid-copy cancels the copy, which from then on publishes nothing even if
    /// the worker outlives this wait, and waits (a bounded while) for the worker to remove what
    /// it made: the cache that was there stays, no half copy beside it.
    fn drop(&mut self) {
        let _ = self.gate.cancel();
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

impl Side {
    pub(super) fn new(args: &crate::Args) -> Side {
        Side {
            gate: Arc::new(Gate::default()),
            worker: None,
            answers: args.dialog_answer.clone(),
            no_dialog: args.no_dialog,
            hold: args.import_hold.clone(),
            hold_at: args.import_hold_at,
            cache: args.cache_dir.clone(),
        }
    }
}

/// Looks for Dota 2 in the Steam libraries, on a thread of its own.
pub(super) fn begin(run: &mut FirstRun, args: &crate::Args) {
    let (tell, steam) = (run.tell.clone(), args.steam_folder.clone());
    std::thread::spawn(move || {
        let found = if steam.is_empty() {
            crate::desktop::installs()
        } else {
            heatel::locate::find_installs(&steam)
        };
        tell.send(Told::Found(found.into_iter().next()));
    });
}

/// Opens the system's folder dialog on a thread of its own (it blocks until closed).
pub(super) fn ask(run: &mut FirstRun) {
    let tell = run.tell.clone();
    if run.side.no_dialog {
        tell.send(Told::Picked(Err(NoDialog::Missing)));
        return;
    }
    if !run.side.answers.is_empty() {
        let answer = run.side.answers.remove(0);
        tell.send(Told::Picked(Ok(Some(answer))));
        return;
    }
    std::thread::spawn(move || {
        let answer = crate::desktop::ask_folder("Choose the Dota 2 folder (dota 2 beta)");
        tell.send(Told::Picked(answer));
    });
}

/// What the dialog answered.
pub(super) fn picked(run: &mut FirstRun, picked: Picked) {
    match picked {
        Ok(Some(path)) => take(run, path),
        Ok(None) => run.screen = Screen::Ready,
        Err(NoDialog::Missing) => run.screen = Screen::Typing,
        Err(NoDialog::Failed(why)) => {
            run.screen = Screen::Typing;
            run.trouble(format!(
                "The folder dialog could not open: {why}. Type or paste the Dota 2 folder below."
            ));
        }
    }
}

/// Takes the folder the player picked or typed.
fn take(run: &mut FirstRun, path: PathBuf) {
    run.screen = Screen::Ready;
    match heatel::locate::install_in(&path) {
        Some(install) => run.source = Some(install),
        None => run.trouble(crate::import::not_dota(&path)),
    }
}

/// Starts the copy on a worker thread; it is told how far it is and may be ended through its
/// gate. Each copy has a gate of its own: one that published stays published. `force` remakes a
/// copy of the same install.
pub(super) fn import(run: &mut FirstRun, force: bool) {
    let Some(install) = run.source.clone() else {
        return;
    };
    let side = &mut run.side;
    side.gate = Arc::new(Gate::default());
    let (tell, gate) = (run.tell.clone(), side.gate.clone());
    let (cache, hold, hold_at) = (side.cache.clone(), side.hold.clone(), side.hold_at);
    side.worker = Some(std::thread::spawn(move || {
        // Debug: the copy waits at its start until the test lifts the barrier (or CANCEL).
        while let Some(hold) = &hold
            && !hold.exists()
            && !gate.is_cancelled()
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let mut progress = |p: &dbota_import::import::Progress<'_>| {
            tell.send(Told::Progress(Work {
                done: p.done,
                total: p.total,
                item: p.item.to_owned(),
                bytes: p.bytes_written,
            }));
            if hold_at.is_some_and(|at| at.is(p.done, p.total)) {
                while !gate.is_cancelled() {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                return Flow::Continue;
            }
            if gate.is_cancelled() {
                Flow::Stop
            } else {
                Flow::Continue
            }
        };
        let mut told_waiting = false;
        let mut waiting = || {
            if !std::mem::replace(&mut told_waiting, true) {
                tell.send(Told::Waiting);
            }
        };
        let made = if gate.is_cancelled() {
            Ok(Made {
                stopped: true,
                ..Made::default()
            })
        } else {
            let mut watcher = crate::import::Watcher {
                gate: &gate,
                progress: &mut progress,
                waiting: &mut waiting,
            };
            crate::import::import_from(&install, &cache, force, &mut watcher)
                .map(|made| Made {
                    files: made.files,
                    failed: made.failed.len(),
                    stopped: made.stopped,
                })
                .map_err(|why| {
                    // A full drive or a folder in the way says what to do already.
                    if crate::import::says_what_to_do(&why) {
                        why
                    } else {
                        super::failed(&why)
                    }
                })
        };
        tell.send(Told::Imported(made));
    }));
}

/// Ends the copy under way: from now on it publishes nothing, and its worker removes what it
/// made. False when it came too late: the copy was published meanwhile.
pub(super) fn cancel(run: &mut FirstRun) -> bool {
    run.side.gate.cancel()
}

/// A cancel that took effect is final: the gate publishes nothing after it.
pub(super) const CANCEL_IS_FINAL: bool = true;

/// Where the assets that did not convert are named: the client's log.
pub(super) fn log_place() -> String {
    match crate::desktop::log_file() {
        Some(file) => format!("The log file {}", file.display()),
        None => "The output of this run (the terminal dbota was started from)".to_owned(),
    }
}

/// Nothing to look at each frame: the workers tell the screen.
pub(super) fn poll() {}

/// Ends this run so that `main` starts dbota again (at this screen when `reimport`) once the
/// window is closed: with this run's arguments, and the script's steps not played yet in place of
/// its `--script`.
pub(super) fn start_again(
    reimport: bool,
    script: Option<&crate::script::Script>,
    exit: &mut MessageWriter<AppExit>,
) {
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
    if reimport {
        args.push("--reimport".into());
    }
    if crate::RESTART.set(args).is_ok() {
        exit.write(AppExit::Success);
    }
}

/// Takes the typed path.
pub(super) fn take_typed(run: &mut FirstRun) {
    let typed = run.typed.trim().to_owned();
    if !typed.is_empty() {
        take(run, expand(&typed));
    }
}

/// `path` with a leading `~/` meaning the home folder, as a shell would.
fn expand(path: &str) -> PathBuf {
    match (path.strip_prefix("~/"), crate::desktop::home()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(path),
    }
}

/// Whether a character key types its character: not while Ctrl is held (a shortcut), except
/// the AltGr of a Windows layout, which arrives as Ctrl+Alt.
fn types_characters(control: bool, alt_gr: bool, windows: bool) -> bool {
    !control || (windows && alt_gr)
}

/// Whether a key press is the paste shortcut (Ctrl+V, Shift+Insert). AltGr is decided first: on
/// Windows it arrives as Ctrl+Alt, and AltGr+V types a character on some layouts (`@` on a
/// Croatian or Slovenian keyboard), which must not be taken for Ctrl+V.
fn is_paste(key: KeyCode, control: bool, alt_gr: bool, shift: bool, windows: bool) -> bool {
    let typing_altgr = windows && alt_gr;
    (control && !typing_altgr && key == KeyCode::KeyV) || (shift && key == KeyCode::Insert)
}

/// The longest path typed or pasted, in characters.
const MAX_PATH: usize = 1024;
/// The most of a paste taken into the path, in characters.
const MAX_PASTE: usize = 4096;

/// Types the path while the system has no dialog to ask with: keys, text an input method or a
/// script commits, and a paste (Ctrl+V, or Shift+Insert); Enter takes it.
pub(super) fn typing(
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
    // Windows reports AltGr, which its layouts type their extra characters with, as Ctrl+Alt.
    let alt_gr = keys.pressed(KeyCode::ControlLeft) && keys.pressed(KeyCode::AltRight);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut typed = String::new();
    let mut erase = 0;
    let mut enter = false;
    for event in events.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        if is_paste(event.key_code, control, alt_gr, shift, cfg!(windows)) {
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
            Key::Character(chars) if types_characters(control, alt_gr, cfg!(windows)) => {
                typed.push_str(chars)
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Windows types AltGr characters as Ctrl+Alt; a Ctrl shortcut elsewhere types nothing.
    #[test]
    fn altgr_characters_are_typed_on_windows_and_ctrl_shortcuts_are_not() {
        assert!(types_characters(false, false, false));
        assert!(!types_characters(true, false, true));
        assert!(types_characters(true, true, true));
        assert!(!types_characters(true, true, false));
    }

    /// AltGr+V types a character on Windows layouts that have one there; Ctrl+V and Shift+Insert
    /// paste everywhere, and Ctrl+Alt+V (AltGr) pastes only where AltGr is no Ctrl+Alt.
    #[test]
    fn altgr_v_on_windows_is_a_character_and_not_a_paste() {
        use KeyCode::{Insert, KeyV};
        assert!(is_paste(KeyV, true, false, false, true));
        assert!(is_paste(KeyV, true, false, false, false));
        assert!(is_paste(Insert, false, false, true, true));
        assert!(
            !is_paste(KeyV, true, true, false, true),
            "AltGr+V on Windows"
        );
        assert!(
            is_paste(KeyV, true, true, false, false),
            "Ctrl+Alt+V elsewhere"
        );
        assert!(!is_paste(KeyV, false, false, false, true));
    }

    #[test]
    fn the_step_to_hold_at_is_a_number_or_the_last() {
        assert_eq!("12".parse(), Ok(HoldAt::Step(12)));
        assert_eq!("last".parse(), Ok(HoldAt::Last));
        assert!("first".parse::<HoldAt>().is_err());
        assert!(HoldAt::Last.is(4, 5));
        assert!(!HoldAt::Last.is(3, 5));
    }
}
