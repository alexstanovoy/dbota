//! The first run, before the sign-in screen, on the desktop and in a browser alike. dbota draws
//! Dota's art and plays its sounds from a private copy of the player's own Dota 2 files and from
//! nothing else, so a client without a copy of this build (none made yet, one another dbota made,
//! one a browser evicted) shows this screen and nothing else: where the copy is made from (on a
//! desktop Dota 2 found in the computer's Steam libraries, or the folder the player picks in the
//! system's dialog; in a browser the folder the player picks in the browser's), the copy being
//! made off the frame's thread while the screen keeps drawing (the count, what is read, CANCEL),
//! and then dbota starting again on the new copy, at the sign-in screen. dbota needs the player's
//! own Dota 2 (free on Steam): its files are never taken from anywhere else. The settings'
//! RE-IMPORT GAME FILES brings the screen back. A run nobody watches (a capture, a check) has no
//! screen to show and ends with the reason (see `main`).
//!
//! Every import is made beside the copy in place and takes its place only when whole: a failed,
//! stopped or closed one leaves the copy that was there, never half of one.
//!
//! Why dbota starts again rather than going on: everything the client reads from the copy (the
//! manifest, the texts, the shop, the fonts, the map's lighting, the sounds) is read once as it
//! starts, by some forty systems at startup; starting again on the new copy is the one path that
//! reads all of it exactly as every later start does. On Linux and macOS it is the same process
//! (exec), on Windows a new one, in a browser the page loaded again.
//!
//! What differs between the two lives in `native` and `browser`; what the screen says and how it
//! looks (`look`) is the same.

#[cfg(target_arch = "wasm32")]
pub(crate) mod browser;
mod look;
#[cfg(not(target_arch = "wasm32"))]
mod native;

use std::sync::{
    Mutex,
    mpsc::{Receiver, Sender, channel},
};

use bevy::{
    picking::{events::PointerPress, pointer::PointerButton},
    prelude::*,
};

#[cfg(target_arch = "wasm32")]
use browser as side;
#[cfg(not(target_arch = "wasm32"))]
use native as side;
#[cfg(not(target_arch = "wasm32"))]
pub use native::HoldAt;

/// What the copy in place lacks for this build.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// No finished copy is there.
    Import,
    /// A copy another build made is (its files may lack what the client now asks for), or one of
    /// this build's stamp whose files are of a layout older than this build reads.
    Reimport,
    /// The copy is this build's, and the player asked to make it again (the settings' RE-IMPORT
    /// GAME FILES): until a new one is whole, the one in place stays.
    Requested,
}

/// Why the client starts at this screen, and what the start found wrong with the storage when
/// something was.
#[derive(Resource, Clone, Debug)]
pub struct Held {
    pub need: Need,
    pub note: Option<String>,
}

/// Whether a desktop client starts at this screen: the player asked for a re-import, or the
/// cache is missing or another build's.
#[cfg(not(target_arch = "wasm32"))]
pub fn held(args: &crate::Args) -> Option<Need> {
    if args.reimport {
        Some(Need::Requested)
    } else {
        crate::import::need(&args.cache_dir)
    }
}

pub fn plugin(app: &mut App) {
    app.add_observer(reimport_pressed);
    let Some(held) = app.world().get_resource::<Held>().cloned() else {
        return;
    };
    let args = app.world().resource::<crate::Args>().clone();
    let run = FirstRun::new(held, &args);
    app.insert_resource(run).add_observer(presses).add_systems(
        Update,
        (
            side::typing,
            listen,
            side::poll,
            restart,
            look::draw,
            look::bar,
        )
            .chain(),
    );
}

/// What the player is doing on the screen.
#[derive(Clone, Debug, PartialEq)]
enum Screen {
    /// The Steam libraries are being searched.
    Looking,
    /// What was found is shown, with the ways to a copy.
    Ready,
    /// The folder dialog is open.
    Choosing,
    /// The system has no dialog: the path is typed.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(dead_code, reason = "a browser always has a folder dialog")
    )]
    Typing,
    /// The copy is being made.
    Working(Work),
    /// Another dbota is making its copy in the same folder: this one starts when it is done, or
    /// is cancelled.
    Waiting,
    /// CANCEL was pressed and nothing will be published: the worker is ending, its half copy
    /// removed. The screen leaves this for the ways to a copy, never for a restart.
    Stopping,
    /// The copy is in place, some assets missing: CONTINUE or IMPORT AGAIN.
    Partial { files: usize, failed: usize },
    /// dbota starts again on the copy.
    Restarting,
}

/// How far the copy is.
#[derive(Clone, Debug, Default, PartialEq)]
struct Work {
    done: usize,
    total: usize,
    item: String,
    bytes: u64,
}

/// How an import ended.
#[derive(Debug, Default)]
struct Made {
    /// Files written.
    files: usize,
    /// Assets that did not convert: what they draw is missing.
    failed: usize,
    /// The player stopped it: the copy in place stays.
    stopped: bool,
}

/// What the backend's workers tell the screen.
enum Told {
    /// Where the copy may be made from: an install found, or none.
    Found(Option<side::Source>),
    /// The folder the player picked in the dialog (nothing when it was closed), or why there was
    /// no dialog.
    Picked(side::Picked),
    /// The import waits for another's import of the same folder.
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            dead_code,
            reason = "a browser's imports take turns by a lock, and say so"
        )
    )]
    Waiting,
    Progress(Work),
    /// How the import ended, or what to tell the player when it failed.
    Imported(Result<Made, String>),
    /// A line to show while the work waits (a server too busy to send the importer at once).
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(dead_code, reason = "a desktop's importer is in the binary")
    )]
    Said(String),
}

/// The sending side of what the workers tell the screen: it also wakes the app, which draws an idle
/// screen only on demand.
#[derive(Clone)]
struct Tell(Sender<Told>, std::task::Waker);

impl Tell {
    fn send(&self, message: Told) {
        if self.0.send(message).is_ok() {
            self.1.wake_by_ref();
        }
    }
}

/// A line under the text: what went wrong and what to do, or what happened.
#[derive(Clone, Debug, PartialEq)]
enum Note {
    Trouble(String),
    Info(String),
}

impl Note {
    fn text(&self) -> &str {
        match self {
            Note::Trouble(text) | Note::Info(text) => text,
        }
    }
}

/// The screen's state and its workers.
#[derive(Resource)]
struct FirstRun {
    need: Need,
    screen: Screen,
    /// Where the copy is made from: the install found, or the folder the player picked.
    source: Option<side::Source>,
    /// A line under the text: why the last pick, import or copy did not work out.
    note: Option<Note>,
    /// The path being typed (a desktop without a folder dialog).
    typed: String,
    tell: Tell,
    told: Mutex<Receiver<Told>>,
    /// What only one backend keeps.
    side: side::Side,
}

impl FirstRun {
    fn new(held: Held, args: &crate::Args) -> FirstRun {
        let (tell, told) = channel();
        let tell = Tell(tell, crate::pacing::waker());
        let mut run = FirstRun {
            need: held.need,
            screen: Screen::Looking,
            source: None,
            note: held.note.map(Note::Trouble),
            typed: String::new(),
            side: side::Side::new(args),
            tell,
            told: Mutex::new(told),
        };
        side::begin(&mut run, args);
        run
    }

    /// Opens the folder dialog.
    fn ask(&mut self) {
        self.note = None;
        self.screen = Screen::Choosing;
        side::ask(self);
    }

    /// Starts the copy from the source at hand; `force` remakes a copy of the same install.
    fn import(&mut self, force: bool) {
        if self.source.is_none() {
            return;
        }
        self.note = None;
        self.screen = Screen::Working(Work::default());
        side::import(self, force);
    }

    /// What a worker said since the last frame.
    fn heard(&mut self, told: Told) {
        match told {
            Told::Found(found) => {
                if self.source.is_none() {
                    self.source = found;
                }
                if self.screen == Screen::Looking {
                    self.screen = Screen::Ready;
                }
            }
            Told::Picked(picked) => side::picked(self, picked),
            Told::Waiting => {
                if matches!(self.screen, Screen::Working(_)) {
                    self.screen = Screen::Waiting;
                }
            }
            Told::Progress(work) => {
                if matches!(self.screen, Screen::Working(_) | Screen::Waiting) {
                    self.screen = Screen::Working(work);
                }
            }
            Told::Said(line) => self.note = Some(Note::Info(line)),
            Told::Imported(made) => {
                // A desktop's cancel that took effect (see `Press::Cancel`) means nothing was
                // published, whatever the worker found out meanwhile.
                if self.screen == Screen::Stopping && side::CANCEL_IS_FINAL {
                    if let Err(why) = &made {
                        warn!("dbota import: ended by a cancel after: {why}");
                    }
                    self.screen = self.ended(Note::Info(CANCELLED.to_owned()));
                    return;
                }
                self.screen = match made {
                    Ok(made) if made.stopped => self.ended(Note::Info(CANCELLED.to_owned())),
                    Err(why) => {
                        warn!("dbota import: {why}");
                        self.ended(Note::Trouble(why))
                    }
                    Ok(made) if made.failed == 0 => Screen::Restarting,
                    Ok(made) => Screen::Partial {
                        files: made.files,
                        failed: made.failed,
                    },
                };
            }
        }
    }

    /// Back to the ways to a copy, `note` said under them.
    fn ended(&mut self, note: Note) -> Screen {
        self.note = Some(note);
        Screen::Ready
    }

    /// Says what went wrong, under the text.
    fn trouble(&mut self, why: impl Into<String>) {
        self.note = Some(Note::Trouble(why.into()));
    }
}

/// What the screen says of a cancelled import.
const CANCELLED: &str =
    "Import cancelled. Nothing of it was kept: import again when you are ready.";

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
        side::start_again(true, script.as_deref(), &mut exit);
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
        // Importing again from the screen of a partial copy remakes it; a first import over a copy
        // another dbota finished meanwhile finds it done (and a requested one is a remake).
        (Press::Import, Screen::Ready) => {
            let remake = run.need == Need::Requested;
            run.import(remake);
        }
        (Press::Import, Screen::Partial { .. }) => run.import(true),
        (Press::ChooseDota, Screen::Ready) => run.ask(),
        (Press::Take, Screen::Typing) => side::take_typed(&mut run),
        (Press::Back, Screen::Typing | Screen::Choosing) => run.screen = Screen::Ready,
        // Too late when the copy was published meanwhile: the screen then goes on as it does
        // for a finished copy, when the worker says so.
        (Press::Cancel, Screen::Working(_) | Screen::Waiting) => {
            if side::cancel(&mut run) {
                run.screen = Screen::Stopping;
            }
        }
        (Press::Continue, Screen::Partial { .. }) => run.screen = Screen::Restarting,
        (Press::Keep, Screen::Ready) => side::start_again(false, script.as_deref(), &mut exit),
        (Press::Quit, _) => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

/// Takes in what the workers said since the last frame.
fn listen(mut run: ResMut<FirstRun>) {
    for _ in 0..128 {
        let Some(told) = run.told.lock().ok().and_then(|told| told.try_recv().ok()) else {
            break;
        };
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
    side::start_again(false, script.as_deref(), &mut exit);
}

/// What a player is told when an import failed for a reason dbota cannot name a remedy for.
fn failed(why: &str) -> String {
    format!(
        "The import failed: {why}. Choose another folder or try the import again; if it fails again, have Steam check Dota 2's files \
         (right-click Dota 2 → Properties → Installed Files → Verify integrity)."
    )
}

/// Where a player finds the folder of their install.
const WHERE: &str = "In Steam: right-click Dota 2 → Manage → Browse local files. That is the \
                     \"dota 2 beta\" folder.";

/// What the screen shows: its title, its lines, where the copy comes from, a note of what went
/// wrong, the work under way, and its buttons (the first the screen's main action).
#[derive(Clone, Debug, PartialEq)]
struct Shown {
    title: String,
    /// What dbota is: on the very first run only.
    pitch: Option<&'static str>,
    /// What the copy is and needs.
    lead: String,
    /// What the screen is waiting for or what was found: a heading and its line.
    state: Option<(String, String)>,
    /// What the player picked or dbota found, shown as a path the eye can skip.
    path: Option<String>,
    note: Option<Note>,
    working: bool,
    buttons: Vec<(&'static str, Press)>,
    /// The promise the import keeps.
    privacy: &'static str,
}

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
            "Your copy of the game files is out of date for this version".to_owned(),
            format!(
                "It was imported by another dbota and may lack what this one draws (it needs \
                 assets {}). Import it again from your Dota 2 folder: it takes about a minute.",
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

/// What the screen says and offers now.
fn shown(run: &FirstRun) -> Shown {
    let (title, lead) = need_words(run.need);
    let mut state = None;
    let mut path = None;
    let buttons = match &run.screen {
        Screen::Looking => {
            state = Some((
                "LOOKING FOR DOTA 2".to_owned(),
                "In this computer's Steam libraries...".to_owned(),
            ));
            side::quit_or_nothing()
        }
        Screen::Ready => {
            match &run.source {
                Some(source) => {
                    state = Some((side::FOUND.to_owned(), String::new()));
                    path = Some(side::shown_source(source));
                }
                None => state = Some((side::NOT_FOUND.to_owned(), side::not_found_line())),
            }
            ready_buttons(run)
        }
        Screen::Choosing => {
            state = Some((
                "CHOOSE THE FOLDER".to_owned(),
                format!("In the window that opened. {WHERE}"),
            ));
            side::choosing_buttons()
        }
        Screen::Typing => {
            state = Some((
                "TYPE THE PATH".to_owned(),
                format!(
                    "Of the Dota 2 folder (dota 2 beta): type or paste it, then Enter. {WHERE}"
                ),
            ));
            path = Some(format!("{}|", run.typed));
            vec![("USE THIS PATH", Press::Take), ("BACK", Press::Back)]
        }
        Screen::Working(_) => {
            path = run.source.as_ref().map(side::shown_source);
            vec![("CANCEL", Press::Cancel)]
        }
        Screen::Waiting => {
            state = Some((
                "WAITING".to_owned(),
                "Another dbota is making its copy in the same folder. This one starts when it is \
                 done, or press CANCEL."
                    .to_owned(),
            ));
            vec![("CANCEL", Press::Cancel)]
        }
        Screen::Stopping => {
            state = Some((
                "STOPPING".to_owned(),
                "What was made so far is removed...".to_owned(),
            ));
            vec![]
        }
        Screen::Partial { files, failed } => {
            state = Some((
                "IMPORTED, WITH GAPS".to_owned(),
                partial_words(*files, *failed),
            ));
            let mut buttons = vec![
                ("CONTINUE", Press::Continue),
                ("IMPORT AGAIN", Press::Import),
            ];
            buttons.extend(side::quit_or_nothing());
            buttons
        }
        Screen::Restarting => {
            state = Some((
                "READY".to_owned(),
                "Your Dota 2 files are ready. Starting dbota...".to_owned(),
            ));
            vec![]
        }
    };
    Shown {
        pitch: (run.need == Need::Import).then_some(
            "dbota is the client of bota, an open-source simulator of Dota 2 matches: play \
             against bots and people with Dota 2's look.",
        ),
        title,
        lead,
        state,
        path,
        note: run.note.clone(),
        working: matches!(run.screen, Screen::Working(_)),
        buttons,
        privacy: side::PRIVACY,
    }
}

/// What the screen says of a copy some assets of which did not convert, and where they are
/// named.
fn partial_words(files: usize, failed: usize) -> String {
    format!(
        "Imported {files} files. {failed} assets could not be converted and are missing; dbota \
         plays without them. {} names them.",
        side::log_place()
    )
}

/// The buttons beside an install found or not: the import, another folder, back.
fn ready_buttons(run: &FirstRun) -> Vec<(&'static str, Press)> {
    let mut buttons = match run.source {
        Some(_) => vec![
            ("IMPORT FROM DOTA 2", Press::Import),
            ("CHOOSE ANOTHER FOLDER", Press::ChooseDota),
        ],
        None => vec![("CHOOSE THE DOTA 2 FOLDER", Press::ChooseDota)],
    };
    if run.need == Need::Requested {
        buttons.push(("BACK TO DBOTA", Press::Keep));
    }
    buttons.extend(side::quit_or_nothing());
    buttons
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use clap::Parser;

    fn run() -> FirstRun {
        let held = Held {
            need: Need::Import,
            note: None,
        };
        let mut args = crate::Args::parse_from(["dbota"]);
        args.steam_folder = vec![crate::test_support::scratch_path("no-steam-library")];
        FirstRun::new(held, &args)
    }

    #[test]
    fn progress_updates_the_mounted_controls_without_allocating() {
        let mut run = run();
        run.screen = Screen::Working(Work {
            done: 0,
            total: 100,
            item: String::new(),
            bytes: 0,
        });
        let sender = run.tell.clone();
        let mut world = World::new();
        world.insert_resource(run);
        world.insert_resource(Fonts::for_layout_test());
        let mut schedule = Schedule::default();
        schedule.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        schedule.add_systems((listen, look::draw, look::bar).chain());
        for _ in 0..3 {
            schedule.run(&mut world);
        }
        let cancel_button = |world: &mut World| {
            world
                .query::<(Entity, &Press)>()
                .iter(world)
                .find_map(|(entity, press)| (*press == Press::Cancel).then_some(entity))
        };
        let cancel = cancel_button(&mut world).expect("CANCEL on the initial progress screen");
        sender.send(Told::Progress(Work {
            done: 50,
            total: 100,
            item: "model.hero".to_owned(),
            bytes: 2 << 20,
        }));
        let allocations = crate::test_support::count_allocations(|| schedule.run(&mut world));
        assert_eq!(
            cancel_button(&mut world),
            Some(cancel),
            "CANCEL was replaced on progress"
        );
        assert!(
            world
                .query::<&Text>()
                .iter(&world)
                .any(|text| text.0.contains("50 of 100"))
        );
        assert!(
            world
                .query::<&Text>()
                .iter(&world)
                .any(|text| text.0 == "2 MB written  ·  50 left")
        );
        assert!(
            world
                .query::<&Node>()
                .iter(&world)
                .any(|node| node.width == percent(50))
        );
        assert_eq!(allocations, 0, "allocations in one progress update");
    }

    /// A press of CANCEL that took effect means nothing is published: whatever the worker then
    /// reports (a clean end, even a copy it found done), the screen goes back to its choices and
    /// never on to a restart or to the partial-copy screen.
    #[test]
    fn a_cancelled_screen_never_leaves_stopping_for_a_restart() {
        for told in [
            Ok(Made::default()),
            Ok(Made {
                stopped: true,
                ..Made::default()
            }),
            Ok(Made {
                files: 3,
                failed: 1,
                stopped: false,
            }),
            Err("a file could not be written".to_owned()),
        ] {
            let mut run = run();
            run.screen = Screen::Stopping;
            run.heard(Told::Imported(told));
            assert_eq!(run.screen, Screen::Ready);
            assert_eq!(run.note, Some(Note::Info(CANCELLED.to_owned())));
        }
    }

    /// A copy that was published before the cancel came is told as finished, not as cancelled.
    #[test]
    fn a_copy_published_before_the_cancel_goes_on_to_the_restart() {
        let mut published = run();
        published.screen = Screen::Working(Work {
            done: 1,
            total: 1,
            item: String::new(),
            bytes: 0,
        });
        published.heard(Told::Imported(Ok(Made {
            files: 3,
            ..Made::default()
        })));
        assert_eq!(published.screen, Screen::Restarting);
    }

    /// A progress report after the wait ended turns the waiting screen into the working one.
    #[test]
    fn a_screen_that_waited_shows_the_work_when_it_starts() {
        let mut run = run();
        run.screen = Screen::Working(Work::default());
        run.heard(Told::Waiting);
        assert_eq!(run.screen, Screen::Waiting);
        let work = Work {
            done: 2,
            total: 9,
            item: "a".to_owned(),
            bytes: 0,
        };
        run.heard(Told::Progress(work.clone()));
        assert_eq!(run.screen, Screen::Working(work));
    }

    /// The partial-copy screen names where the failed assets are told.
    #[test]
    fn the_partial_copy_words_name_the_log() {
        let words = partial_words(10, 2);
        assert!(words.contains("2 assets could not be converted"), "{words}");
        assert!(words.contains("names them"), "{words}");
    }
}
