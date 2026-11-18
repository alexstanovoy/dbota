//! What the native client asks of the system it runs on: where its files go, where Steam keeps
//! Dota 2, the system's folder dialog, the free space of a drive, starting itself
//! again, and how a failure reaches a player who started it with a double-click (no terminal):
//! on Windows a log file and a message box.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use heatel::locate::{self, Install, Os};

/// An environment variable's value as a path, when it is set, not empty and absolute.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
}

/// The user's home folder (`HOME`; on Windows `USERPROFILE`).
pub fn home() -> Option<PathBuf> {
    env_path("HOME").or_else(|| cfg!(windows).then(|| env_path("USERPROFILE")).flatten())
}

/// The folder of this client's own files on Windows: `%LOCALAPPDATA%\dbota` (cache, settings,
/// replays and log under it). None elsewhere: there the system's usual places are taken.
pub fn windows_data() -> Option<PathBuf> {
    cfg!(windows)
        .then(|| env_path("LOCALAPPDATA"))
        .flatten()
        .map(|local| local.join("dbota"))
}

/// Every Dota 2 install in this machine's Steam libraries, the most likely first.
pub fn installs() -> Vec<Install> {
    let roots = locate::steam_roots(Os::current(), home().as_deref(), &registered_steam());
    locate::find_installs(&roots)
}

/// The Steam folders Windows names: the registry's, then the default under Program Files.
#[cfg(windows)]
fn registered_steam() -> Vec<PathBuf> {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ, RegGetValueW,
    };
    use windows::core::{HSTRING, PCWSTR};
    fn value(root: HKEY, key: &str, name: &str) -> Option<PathBuf> {
        // Steam's own values are short paths; a longer one is not taken.
        let mut buffer = [0u16; 1024];
        let mut bytes = (buffer.len() * 2) as u32;
        let (key, name) = (HSTRING::from(key), HSTRING::from(name));
        // SAFETY: the buffer and its size in bytes are passed together; the call writes within it.
        let read = unsafe {
            RegGetValueW(
                root,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut bytes),
            )
        };
        if read.is_err() {
            return None;
        }
        let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
        let path = PathBuf::from(String::from_utf16_lossy(&buffer[..len]));
        path.is_absolute().then_some(path)
    }
    [
        value(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath"),
        value(
            HKEY_LOCAL_MACHINE,
            r"SOFTWARE\WOW6432Node\Valve\Steam",
            "InstallPath",
        ),
        value(HKEY_LOCAL_MACHINE, r"SOFTWARE\Valve\Steam", "InstallPath"),
        env_path("ProgramFiles(x86)").map(|dir| dir.join("Steam")),
        env_path("ProgramFiles").map(|dir| dir.join("Steam")),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(not(windows))]
fn registered_steam() -> Vec<PathBuf> {
    Vec::new()
}

/// Why no dialog answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoDialog {
    /// The system has no dialog this client can open (a Linux desktop without zenity or
    /// kdialog): the path is typed instead.
    Missing,
    /// The dialog failed, with why.
    Failed(String),
}

/// Asks the player for a folder with the system's own dialog; blocks until it is closed, so it
/// runs on a thread of its own. `Ok(None)` when the player closed it unanswered.
pub fn ask_folder(title: &str) -> Result<Option<PathBuf>, NoDialog> {
    dialog(title)
}

#[cfg(windows)]
fn dialog(title: &str) -> Result<Option<PathBuf>, NoDialog> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::ERROR_CANCELLED;
    use windows::Win32::System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS, FileOpenDialog, IFileOpenDialog, SIGDN_FILESYSPATH,
    };
    use windows::core::{HRESULT, HSTRING};
    let failed = |why: windows::core::Error| NoDialog::Failed(why.message());
    // SAFETY: COM is initialised on this thread for the dialog's life and uninitialised after;
    // the dialog's strings outlive the calls that take them, and the path it returns is freed
    // with the allocator that made it.
    unsafe {
        let initialised = CoInitializeEx(None, COINIT_APARTMENTTHREADED).is_ok();
        let answer = (|| {
            let dialog: IFileOpenDialog =
                CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(dialog.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PICKFOLDERS)?;
            dialog.SetTitle(&HSTRING::from(title))?;
            if let Err(why) = dialog.Show(own_window()) {
                if why.code() == HRESULT::from_win32(ERROR_CANCELLED.0) {
                    return Ok(None);
                }
                return Err(why);
            }
            let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            // Wide characters as the system names the folder, also when they are not valid
            // UTF-16 (a name with an unpaired surrogate is a name all the same).
            let path = std::ffi::OsString::from_wide(name.as_wide());
            CoTaskMemFree(Some(name.0 as *const _));
            Ok(Some(PathBuf::from(path)))
        })();
        if initialised {
            CoUninitialize();
        }
        answer.map_err(failed)
    }
}

/// This process's window that is in front, which a dialog is made the child of: a dialog with
/// no owner can open behind a borderless fullscreen window, which is how the client starts.
/// None when the foreground window is another program's.
#[cfg(windows)]
fn own_window() -> Option<windows::Win32::Foundation::HWND> {
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    // SAFETY: plain queries; the handle is only passed on, and a stale one is refused by the
    // system.
    unsafe {
        let window = GetForegroundWindow();
        let mut process = 0;
        GetWindowThreadProcessId(window, Some(&mut process));
        (!window.is_invalid() && process == std::process::id()).then_some(window)
    }
}

#[cfg(target_os = "macos")]
fn dialog(title: &str) -> Result<Option<PathBuf>, NoDialog> {
    let quoted = title.replace(['"', '\\'], "");
    let script = format!("POSIX path of (choose folder with prompt \"{quoted}\")");
    run_dialog(std::process::Command::new("osascript").args(["-e", &script]))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn dialog(title: &str) -> Result<Option<PathBuf>, NoDialog> {
    let mut zenity = std::process::Command::new("zenity");
    zenity.args(["--file-selection", "--directory", "--title", title]);
    let mut kdialog = std::process::Command::new("kdialog");
    kdialog.args(["--title", title, "--getexistingdirectory", "."]);
    match run_dialog(&mut zenity) {
        Err(NoDialog::Missing) => run_dialog(&mut kdialog),
        answer => answer,
    }
}

/// The path a dialog program printed: its first line, at most [`MAX_ANSWER`] bytes; none when
/// it ended without one (closed unanswered).
#[cfg(unix)]
fn run_dialog(command: &mut std::process::Command) -> Result<Option<PathBuf>, NoDialog> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|why| match why.kind() {
            std::io::ErrorKind::NotFound => NoDialog::Missing,
            _ => NoDialog::Failed(why.to_string()),
        })?;
    let mut said = Vec::new();
    if let Some(out) = child.stdout.take() {
        let _ = out.take(MAX_ANSWER as u64 + 1).read_to_end(&mut said);
    }
    let ended = child
        .wait()
        .map_err(|why| NoDialog::Failed(why.to_string()))?;
    if !ended.success() || said.len() > MAX_ANSWER {
        return Ok(None);
    }
    // The bytes as the program printed them: a folder whose name is not UTF-8 is still that
    // folder.
    use std::os::unix::ffi::OsStrExt;
    let line = said.split(|&byte| byte == b'\n').next().unwrap_or(&[]);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    Ok((!line.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(line))))
}

/// The longest path a dialog program may answer with.
#[cfg(unix)]
const MAX_ANSWER: usize = 8192;

/// Bytes free for this user on the drive that holds `path` (or, while it does not exist yet,
/// its nearest folder that does); none when the system does not tell.
pub fn free_space(path: &Path) -> Option<u64> {
    // A relative path's nearest folder that exists is found among its absolute ancestors: its
    // own ancestors end in an empty path that exists nowhere.
    let path = std::path::absolute(path).ok()?;
    let existing = path.ancestors().find(|p| p.exists())?;
    free_at(existing)
}

#[cfg(unix)]
fn free_at(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    // SAFETY: `name` is a C string and `stat` a statvfs the call fills in.
    let read = unsafe { libc::statvfs(name.as_ptr(), stat.as_mut_ptr()) };
    if read != 0 {
        return None;
    }
    // SAFETY: the call succeeded, so it wrote the whole struct.
    let stat = unsafe { stat.assume_init() };
    #[allow(
        clippy::unnecessary_cast,
        reason = "the field types differ between systems"
    )]
    Some((stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64))
}

#[cfg(windows)]
fn free_at(path: &Path) -> Option<u64> {
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    use windows::core::HSTRING;
    let name = HSTRING::from(path.as_os_str());
    let mut free = 0u64;
    // SAFETY: `name` is a wide string that outlives the call, `free` a u64 it writes.
    unsafe { GetDiskFreeSpaceExW(&name, Some(&mut free), None, None) }.ok()?;
    Some(free)
}

/// Ends this process and starts the client again with `args`: what an import or a re-import
/// asked for. On Unix the process becomes the new client (same process id, so whoever started
/// it waits for the new one); on Windows the new one is started and this one ends. Returns only
/// when that failed.
pub fn restart(args: &[OsString]) -> std::io::Error {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(why) => return why,
    };
    let mut command = std::process::Command::new(exe);
    command.args(args).env(CONTINUE_LOG, "1");
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.exec()
    }
    #[cfg(not(unix))]
    match command.spawn() {
        Ok(_) => std::process::exit(0),
        Err(why) => why,
    }
}

/// Where the log of a client started without a terminal goes: `$DBOTA_LOG_FILE` when it names a
/// full path (any system), else `%LOCALAPPDATA%\dbota\dbota.log` on Windows; none elsewhere
/// (the log goes to the terminal's stderr).
pub fn log_file() -> Option<PathBuf> {
    env_path("DBOTA_LOG_FILE").or_else(|| windows_data().map(|data| data.join("dbota.log")))
}

/// Set for a client that starts again (see [`restart`]): it goes on with the log of the run that
/// started it instead of keeping that one as the previous log, so that what an import said
/// before the restart (the assets that did not convert) stays in the file the screen named.
const CONTINUE_LOG: &str = "DBOTA_CONTINUE_LOG";

/// The largest log file kept; past it the log stops growing (a run that logs this much
/// repeats itself, and the beginning says why).
pub const MAX_LOG: u64 = 32 << 20;

/// A writer of the log file: the previous run's log is kept beside it as `dbota.previous.log`.
pub struct LogFile {
    file: Option<std::sync::Mutex<std::io::BufWriter<std::fs::File>>>,
    written: std::sync::atomic::AtomicU64,
}

impl LogFile {
    /// The log of this run, at `path`; one that cannot be made writes nowhere. A run that
    /// continues (`continued`) adds to the file that is there; any other keeps it as the
    /// previous log and starts a new one.
    pub fn open(path: Option<&Path>, continued: bool) -> LogFile {
        let mut kept = 0;
        let file = path.and_then(|path| {
            std::fs::create_dir_all(path.parent()?).ok()?;
            if continued {
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .ok()?;
                kept = file.metadata().map_or(0, |meta| meta.len());
                return Some(file);
            }
            let _ = std::fs::rename(path, path.with_file_name("dbota.previous.log"));
            std::fs::File::create(path).ok()
        });
        LogFile {
            file: file.map(|file| std::sync::Mutex::new(std::io::BufWriter::new(file))),
            written: std::sync::atomic::AtomicU64::new(kept),
        }
    }

    /// Adds `bytes` while the file is under [`MAX_LOG`]; errors and warnings are flushed at once.
    pub fn write(&self, bytes: &[u8], flush: bool) {
        use std::io::Write;
        use std::sync::atomic::Ordering;
        let Some(file) = &self.file else {
            return;
        };
        let before = self
            .written
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        if before >= MAX_LOG {
            return;
        }
        if let Ok(mut file) = file.lock() {
            let _ = file.write_all(bytes);
            if flush {
                let _ = file.flush();
            }
        }
    }
}

/// The process's log file, opened once.
pub fn log() -> &'static LogFile {
    static LOG: std::sync::OnceLock<LogFile> = std::sync::OnceLock::new();
    LOG.get_or_init(|| {
        let continued = std::env::var_os(CONTINUE_LOG).is_some();
        LogFile::open(log_file().as_deref(), continued)
    })
}

/// A failure the player must see: on stderr and in the log file, and on Windows (where a
/// double-clicked client has no terminal) in a message box. Blocks until the box is closed.
pub fn tell_failure(title: &str, text: &str) {
    eprintln!("dbota: {title}: {text}");
    log().write(format!("dbota: {title}: {text}\n").as_bytes(), true);
    message_box(title, text);
}

#[cfg(windows)]
fn message_box(title: &str, text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;
    let (title, text) = (HSTRING::from(title), HSTRING::from(text));
    // SAFETY: both strings outlive the call; no owner window.
    unsafe { MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR) };
}

#[cfg(not(windows))]
fn message_box(_: &str, _: &str) {}

/// What a panic's message means to a player: a GPU that cannot be drawn with is the machine's
/// graphics, not a bug in dbota.
pub fn panic_words(message: &str) -> (&'static str, String) {
    let graphics = [
        "Unable to find a GPU",
        "request_device",
        "RequestDeviceError",
    ];
    if graphics.iter().any(|sign| message.contains(sign)) {
        return (
            "dbota could not start its graphics",
            format!(
                "No graphics card it can draw with was found: dbota draws with DirectX 12 or \
                 Vulkan (Metal on a Mac). Install or update the graphics driver from the \
                 maker of the graphics card (NVIDIA, AMD or Intel) and start dbota again.\n\n\
                 Details: {message}"
            ),
        );
    }
    (
        "dbota stopped because of an error in it",
        format!(
            "This is a bug in dbota, not something you did. Starting it again usually gets you \
             back. The details below and the log ({}) help to fix it.\n\n{message}",
            log_file().map_or_else(|| "stderr".to_owned(), |p| p.display().to_string())
        ),
    )
}

/// Tells the first panic of the process to the player (see [`tell_failure`]), after the panic
/// hook in place (Bevy's, printing it to stderr).
pub fn report_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        static TOLD: std::sync::Once = std::sync::Once::new();
        TOLD.call_once(|| {
            let message = info
                .payload()
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| info.payload().downcast_ref::<String>().cloned())
                .unwrap_or_default();
            let place = info
                .location()
                .map(|l| format!(" ({}:{})", l.file(), l.line()))
                .unwrap_or_default();
            let (title, text) = panic_words(&format!("{message}{place}"));
            tell_failure(title, &text);
        });
    }));
}

/// On Windows, where dbota is a windowed program with no console of its own, a client started
/// from a terminal (`dbota import`, `dbota --help`) writes to that terminal.
pub fn attach_console() {
    #[cfg(windows)]
    // SAFETY: a plain call; it fails harmlessly when the parent has no console.
    unsafe {
        let _ = windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_graphics_card_is_told_as_the_machine_s_graphics_not_a_bug() {
        let (title, text) =
            panic_words("Unable to find a GPU! Make sure you have installed required drivers!");
        assert_eq!(title, "dbota could not start its graphics");
        assert!(text.contains("update the graphics driver"), "{text}");
        let (title, _) = panic_words("index out of bounds");
        assert_eq!(title, "dbota stopped because of an error in it");
    }

    #[test]
    fn the_log_file_stops_growing_at_its_bound_and_keeps_the_previous_run() {
        let dir = std::env::temp_dir().join(format!("dbota-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("dbota.log");
        LogFile::open(Some(&path), false).write(b"first run\n", true);
        let log = LogFile::open(Some(&path), false);
        let line = vec![b'x'; 1 << 20];
        for _ in 0..(MAX_LOG >> 20) + 4 {
            log.write(&line, false);
        }
        drop(log);
        let size = std::fs::metadata(&path).expect("the log").len();
        assert!(size <= MAX_LOG, "{size}");
        let previous = std::fs::read(dir.join("dbota.previous.log")).expect("the previous log");
        assert_eq!(previous, b"first run\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A client that starts again after an import goes on with the log it was started with:
    /// what the import said stays in the file the screen named, and the previous run's log is
    /// not pushed out by the restart.
    #[test]
    fn a_restarted_client_adds_to_the_log_of_the_run_that_restarted_it() {
        let dir = std::env::temp_dir().join(format!("dbota-log-continued-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("dbota.log");
        LogFile::open(Some(&path), false).write(b"older run\n", true);
        LogFile::open(Some(&path), false).write(b"import said: asset a did not convert\n", true);
        LogFile::open(Some(&path), true).write(b"started again\n", true);
        let log = std::fs::read_to_string(&path).expect("the log");
        assert_eq!(log, "import said: asset a did not convert\nstarted again\n");
        let previous = std::fs::read_to_string(dir.join("dbota.previous.log")).expect("previous");
        assert_eq!(previous, "older run\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The space check of a cache folder given as a relative name that does not exist yet
    /// (`--cache-dir assets`) looks at the drive of the working folder instead of giving none.
    #[test]
    fn a_relative_cache_folder_that_does_not_exist_yet_has_free_space_known() {
        assert!(free_space(Path::new("dbota-no-such-folder-for-space")).is_some());
    }
}
