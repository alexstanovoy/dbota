//! Replays as files in a folder: `$XDG_DATA_HOME/dbota/replays` on Linux, the platform's
//! user data folder elsewhere. A thread of its own does the work in the order it is asked, so
//! the frame loop never waits on a disk; every file is written under a name of its own and
//! renamed into place, so a crash leaves a whole file or none.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

use super::backend::{Backend, Done, Op};
use super::index::{MAX_BYTES, MIN_BYTES, Record, mode_name, valid_name};

const INDEX: &str = "index.txt";
/// Files listed at most: a bound on a folder someone else filled.
const MAX_FILES: usize = 10_000;
/// A recording a game server left in the folder is taken for a crashed client's after this
/// long without a write.
const STALE_SECS: u64 = 3600;
/// The longest index read, in bytes.
const MAX_INDEX: u64 = 4 << 20;

/// Where replays are kept on this machine, if the platform has such a place.
pub fn default_dir() -> Option<PathBuf> {
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = || env("HOME");
    let base = if cfg!(windows) {
        env("LOCALAPPDATA").or_else(|| env("APPDATA"))?
    } else if cfg!(target_os = "macos") {
        home()?.join("Library/Application Support")
    } else {
        env("XDG_DATA_HOME").or_else(|| home().map(|h| h.join(".local/share")))?
    };
    Some(base.join("dbota").join("replays"))
}

/// The recording a game server is told to write: the library adopts it when the game ends.
pub fn recording_path(dir: &Path, mode: &str, secs: u64) -> PathBuf {
    dir.join(format!(".recording-{}-{secs}.part", mode_name(mode)))
}

pub struct Disk {
    dir: PathBuf,
    work: Sender<(Op, Sender<Done>)>,
}

impl Disk {
    pub fn new(dir: PathBuf) -> Disk {
        let (work, jobs) = channel();
        let folder = dir.clone();
        let spawned = std::thread::Builder::new()
            .name("dbota-replay-files".to_owned())
            .spawn(move || serve(&folder, &jobs));
        if let Err(err) = spawned {
            bevy::log::warn!("replays: no thread for the files: {err}");
        }
        Disk { dir, work }
    }
}

impl Backend for Disk {
    fn run(&self, op: Op, reply: Sender<Done>) {
        if self.work.send((op, reply.clone())).is_err() {
            let _ = reply.send(Done::Unavailable("the files' thread is gone".to_owned()));
        }
    }

    fn path(&self, name: &str) -> Option<PathBuf> {
        valid_name(name).then(|| self.dir.join(name))
    }

    fn dir(&self) -> Option<PathBuf> {
        Some(self.dir.clone())
    }
}

fn serve(dir: &Path, jobs: &Receiver<(Op, Sender<Done>)>) {
    for (op, reply) in jobs {
        let done = match op {
            Op::Load => load(dir),
            Op::Put { record, bytes } => {
                let error = write_whole(&dir.join(&record.name), &bytes)
                    .err()
                    .map(|err| format!("cannot keep the replay: {err}"));
                Some(Done::Put { record, error })
            }
            Op::Adopt { record, from } => Some(adopt(dir, record, &from)),
            Op::Remove { names } => {
                let failed =
                    names.iter().filter(|n| valid_name(n)).find_map(
                        |n| match std::fs::remove_file(dir.join(n)) {
                            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                                Some(format!("cannot remove {n}: {e}"))
                            }
                            _ => None,
                        },
                    );
                failed.map(Done::Failed)
            }
            Op::Index { text } => write_whole(&dir.join(INDEX), text.as_bytes())
                .err()
                .map(|e| Done::Failed(format!("cannot write the index: {e}"))),
        };
        if let Some(done) = done
            && reply.send(done).is_err()
        {
            return;
        }
    }
}

fn load(dir: &Path) -> Option<Done> {
    if let Err(err) = std::fs::create_dir_all(dir) {
        return Some(Done::Unavailable(format!("{}: {err}", dir.display())));
    }
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).ok()?.flatten().take(MAX_FILES) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(meta) = entry.metadata().ok().filter(std::fs::Metadata::is_file) else {
            continue;
        };
        let modified = secs(&meta);
        if valid_name(&name) {
            files.push((name, meta.len(), modified));
        } else if name.starts_with(".recording-") && name.ends_with(".part") {
            take_stale_recording(dir, &entry.path(), &name, &meta, &mut files);
        }
    }
    Some(Done::Loaded {
        index: read_text(&dir.join(INDEX)),
        files,
    })
}

/// A recording whose client is long gone: kept as a replay when it holds a match, else removed.
fn take_stale_recording(
    dir: &Path,
    path: &Path,
    name: &str,
    meta: &std::fs::Metadata,
    files: &mut Vec<(String, u64, u64)>,
) {
    let modified = secs(meta);
    if super::name::now().saturating_sub(modified) < STALE_SECS {
        return;
    }
    // `.recording-<mode>-<secs>.part`
    let mode = name
        .trim_start_matches(".recording-")
        .rsplit_once('-')
        .map_or("file", |(mode, _)| mode);
    let kept = super::name::file_name(modified, mode, None);
    if meta.len() >= MIN_BYTES
        && meta.len() <= MAX_BYTES
        && !dir.join(&kept).exists()
        && std::fs::rename(path, dir.join(&kept)).is_ok()
    {
        files.push((kept, meta.len(), modified));
    } else if meta.len() < MIN_BYTES {
        let _ = std::fs::remove_file(path);
    }
}

fn secs(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn read_text(path: &Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_INDEX)
        .read_to_string(&mut text)
        .ok()?;
    Some(text)
}

/// Writes `bytes` to a file of their own beside `path` and renames it over `path`.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let fresh = path.with_extension("tmp");
    let _ = std::fs::remove_file(&fresh);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&fresh)?;
    file.write_all(bytes)?;
    drop(file);
    std::fs::rename(&fresh, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&fresh);
    })
}

fn adopt(dir: &Path, record: Record, from: &Path) -> Done {
    let to = dir.join(&record.name);
    match std::fs::rename(from, &to).and_then(|()| std::fs::metadata(&to)) {
        Ok(meta) => Done::Put {
            record: Record {
                bytes: meta.len(),
                ..record
            },
            error: None,
        },
        Err(err) => Done::Put {
            record,
            error: Some(format!("cannot keep the recording: {err}")),
        },
    }
}
