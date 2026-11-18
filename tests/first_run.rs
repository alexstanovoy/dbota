//! The first run, end to end in the real client (`--script`, drawn in a headless compositor): a
//! client without an asset cache of its build finds Dota 2 by itself in the libraries of a Steam
//! folder (`--steam-folder`: the harness gives the client a private home), imports it on the press of a button while its screen keeps drawing, and starts again
//! at the sign-in screen; CANCEL leaves no copy; a cache another build made is imported again;
//! without Dota 2 in Steam the player picks the folder; the settings' RE-IMPORT GAME FILES brings
//! the screen back. The Dota install is a tiny synthetic one (a package directory with nothing
//! in it): every asset fails to convert, which the screen tells, and the client goes on.
//!
//! The runs draw on the GPU, so they are `#[ignore]`d in a plain `cargo test`;
//! `tools/visual_check.sh` runs them. `DBOTA_FIRST_RUN_SHOTS` names a folder to keep their
//! screenshots in.

mod common;

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// Seconds a run may take: a hang guard only.
const PATIENCE: Duration = Duration::from_secs(300);

/// A VPK directory file (version 1) with no file in it.
const EMPTY_PACKAGE: [u8; 13] = [0x34, 0x12, 0xaa, 0x55, 1, 0, 0, 0, 1, 0, 0, 0, 0];

/// What a run leaves: its folder, the cache it imports into, and where its screenshots go.
struct Scratch {
    root: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Scratch {
        let root =
            std::env::temp_dir().join(format!("dbota-first-run-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a scratch folder");
        Scratch { root }
    }

    fn cache(&self) -> PathBuf {
        self.root.join("assets")
    }

    fn shots(&self) -> PathBuf {
        std::env::var_os("DBOTA_FIRST_RUN_SHOTS")
            .map_or_else(|| self.root.join("shots"), PathBuf::from)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A Dota install in the Steam library `library`: its `dota 2 beta` folder.
fn dota_in(library: &Path) -> PathBuf {
    let folder = library.join("steamapps/common/dota 2 beta");
    let game = folder.join("game/dota");
    std::fs::create_dir_all(&game).expect("an install");
    std::fs::write(game.join("pak01_dir.vpk"), EMPTY_PACKAGE).expect("a package directory");
    folder
}

/// A Steam folder (`steam`) whose library list names a second library, `games`, which holds
/// Dota 2: the install a player has on another drive. The client is told to search `steam`
/// (`--steam-folder`): its home is a private one the harness makes.
fn steam_with_dota(client: &mut common::ClientCommand, steam: &Path, games: &Path) -> PathBuf {
    client.arg("--steam-folder").arg(steam);
    std::fs::create_dir_all(steam.join("steamapps")).expect("a Steam folder");
    let vdf = format!(
        "\"libraryfolders\"\n{{\n\t\"0\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n\t\"1\"\n\t{{\n\t\t\"path\"\t\t\"{}\"\n\t}}\n}}\n",
        steam.display(),
        games.display()
    );
    std::fs::write(steam.join("steamapps/libraryfolders.vdf"), vdf).expect("a library list");
    dota_in(games)
}

/// The client of a first run: its home is the caller's to fill before [`run`].
fn first_run_client(scratch: &Scratch) -> common::ClientCommand {
    let mut client = common::client();
    client
        .arg("--cache-dir")
        .arg(scratch.cache())
        .arg("--config")
        .arg(scratch.root.join("settings.conf"))
        .arg("--screenshot-dir")
        .arg(scratch.shots())
        .args(["--mute", "--no-vsync", "--display-mode", "windowed"]);
    client
}

/// Plays `script` in `client` and asserts it ended with exit code 0 having played every step
/// (a step a started-again client plays counts once); returns what it printed.
fn played(what: &str, mut client: common::ClientCommand, script: &str) -> String {
    let log =
        std::env::temp_dir().join(format!("dbota-first-run-{what}-{}.log", std::process::id()));
    let mut child = client
        .args(["--script", script])
        .stdout(Stdio::piped())
        .stderr(std::fs::File::create(&log).expect("a log file"))
        .spawn()
        .expect("dbota runs");
    let mut stdout = child.stdout.take().expect("its stdout");
    let reader = std::thread::spawn(move || {
        let mut said = String::new();
        let _ = std::io::Read::read_to_string(&mut stdout, &mut said);
        said
    });
    let started = Instant::now();
    let code = loop {
        if let Some(status) = child.try_wait().expect("waited for") {
            break status.code();
        }
        if started.elapsed() > PATIENCE {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    let said = reader.join().expect("the client's output");
    let stderr = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    let steps = script.split(';').filter(|s| !s.trim().is_empty()).count();
    let done = said.lines().filter(|l| l.starts_with("script: ")).count();
    assert!(
        code == Some(0) && done == steps && !said.contains("timed out"),
        "{what}: exit {code:?}, {done} of {steps} steps:\n{said}\nstderr:\n{stderr}"
    );
    format!("{said}\n{stderr}")
}

/// The steps from a made copy to the sign-in screen and out: the import's failures are told
/// (nothing converts from an empty package), CONTINUE starts dbota again on the copy, and the
/// menu's quit question ends it.
const TO_SIGN_IN: &str = "await:button:continue;await:text:could not be converted;\
                          shot:first-run-imported.png;click:continue;await:button:sign in;\
                          shot:first-run-sign-in.png";
const QUIT_MENU: &str = "key:Escape;await:popup;click:ok";

/// A cache this run made: finished (index and stamp) and of this build.
fn assert_cache_made(cache: &Path) {
    assert!(
        cache.join("index.txt").is_file(),
        "no index in {}",
        cache.display()
    );
    let stamp = std::fs::read_to_string(cache.join("stamp.txt")).expect("a stamp");
    assert!(
        dbota_import::import::stamped_here(&stamp),
        "the copy is stamped {stamp}, not by this build"
    );
}

/// A first run finds Dota 2 in the second library of the player's Steam by itself, says where,
/// imports it at a click, and goes on to the sign-in screen; the settings' RE-IMPORT GAME FILES
/// brings the import screen back, and BACK TO DBOTA leaves it for the menu.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_first_run_finds_dota_imports_it_and_goes_on_to_sign_in() {
    let scratch = Scratch::new("found");
    let mut client = first_run_client(&scratch);
    let dota = steam_with_dota(
        &mut client,
        &scratch.root.join("Steam"),
        &scratch.root.join("games"),
    );
    let script = format!(
        "await:text:Welcome to dbota;await:text:free on Steam;await:text:never leave your computer;\
         await:text:Dota 2 found: {};shot:first-run-found.png;click:import from dota 2;\
         {TO_SIGN_IN};click:settings;click:options;click:re-import game files;\
         await:text:Re-import the game files;shot:first-run-reimport.png;click:back to dbota;\
         await:button:sign in;{QUIT_MENU}",
        dota.display()
    );
    played("found", client, &script);
    assert_cache_made(&scratch.cache());
}

/// CANCEL stops an import under way and nothing of it is left: no cache, no half copy beside
/// it. While the import works (held at its start here), the screen keeps drawing frames.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn cancelling_an_import_leaves_no_copy_and_the_screen_draws_while_it_works() {
    let scratch = Scratch::new("cancel");
    let mut client = first_run_client(&scratch);
    steam_with_dota(
        &mut client,
        &scratch.root.join("Steam"),
        &scratch.root.join("games"),
    );
    client
        .arg("--import-hold")
        .arg(scratch.root.join("never-lifted"));
    let script = "await:button:import from dota 2;click:import from dota 2;\
                  await:text:Making your private copy;frames:120;shot:first-run-importing.png;\
                  click:cancel;await:text:Import cancelled.;await:button:import from dota 2;\
                  click:quit dbota";
    played("cancel", client, script);
    let cache = scratch.cache();
    assert!(!cache.exists(), "{} was made", cache.display());
    let left: Vec<_> = std::fs::read_dir(&scratch.root)
        .expect("the scratch folder")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("assets"))
        .collect();
    assert!(left.is_empty(), "left beside the cache: {left:?}");
}

/// A cache another dbota made holds the client at the import screen, which says why, and the
/// import from the Steam library replaces it with this build's.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_cache_of_another_build_is_imported_again() {
    let scratch = Scratch::new("stale");
    let cache = scratch.cache();
    std::fs::create_dir_all(&cache).expect("a cache folder");
    std::fs::write(
        cache.join("index.txt"),
        format!("{}\n", dbota_import::index::CacheIndex::header()),
    )
    .expect("an index");
    std::fs::write(cache.join("stamp.txt"), "rev1.1/0000000000000000/1/2").expect("a stamp");
    let mut client = first_run_client(&scratch);
    steam_with_dota(
        &mut client,
        &scratch.root.join("Steam"),
        &scratch.root.join("games"),
    );
    let script = format!(
        "await:text:out of date for this version;await:text:imported by another dbota;\
         shot:first-run-stale.png;click:import from dota 2;{TO_SIGN_IN};{QUIT_MENU}"
    );
    played("stale", client, &script);
    assert_cache_made(&cache);
}

/// Without Dota 2 in the player's Steam the screen says so and asks for the folder: a folder
/// that holds no Dota is refused with why, the install's folder is taken and imported.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn without_dota_in_steam_the_player_picks_its_folder() {
    let scratch = Scratch::new("chooser");
    let elsewhere = dota_in(&scratch.root.join("elsewhere"));
    let nothing = scratch.root.join("nothing");
    std::fs::create_dir_all(&nothing).expect("a folder without Dota");
    let mut client = first_run_client(&scratch);
    client
        .arg("--dialog-answer")
        .arg(&nothing)
        .arg("--dialog-answer")
        .arg(&elsewhere);
    let script = format!(
        "await:text:was not found in this computer's Steam libraries;await:text:Browse local files;\
         shot:first-run-not-found.png;await:nobutton:import from dota 2;\
         click:choose the dota 2 folder;await:text:holds no Dota 2;\
         shot:first-run-not-dota.png;click:choose the dota 2 folder;\
         await:text:Dota 2 found: {};click:import from dota 2;{TO_SIGN_IN};{QUIT_MENU}",
        elsewhere.display()
    );
    played("chooser", client, &script);
    assert_cache_made(&scratch.cache());
}

/// A run nobody watches (a capture) has no import screen to show: without a cache of this build
/// it ends at once with the reason and the way to one, drawing nothing.
#[test]
fn an_unattended_run_without_assets_ends_with_the_reason() {
    let scratch = Scratch::new("unattended");
    let cache = scratch.cache();
    let output = common::binary(&cache)
        .arg("--cache-dir")
        .arg(&cache)
        .args(["--replay", "none.brp", "--screenshot-at", "10"])
        .env_remove("DBOTA_ASSETS")
        .output()
        .expect("dbota runs");
    let said = String::from_utf8_lossy(&output.stderr);
    assert_eq!(output.status.code(), Some(2), "{said}");
    assert!(said.contains("no asset cache at"), "{said}");
    assert!(said.contains("dbota import"), "{said}");
    assert!(!cache.exists());
}

/// On a system without a folder dialog (a Linux desktop without zenity or kdialog) the folder is
/// typed: Enter takes it, and the install in it is found.
#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn without_a_folder_dialog_the_path_is_typed() {
    let scratch = Scratch::new("typed");
    let elsewhere = dota_in(&scratch.root.join("elsewhere"));
    let mut client = first_run_client(&scratch);
    client.arg("--no-dialog");
    let script = format!(
        "click:choose the dota 2 folder;await:button:use this path;text:{};\
         shot:first-run-typed.png;key:Enter;await:text:Dota 2 found: {};click:quit dbota",
        elsewhere.display(),
        elsewhere.display()
    );
    played("typed", client, &script);
}
