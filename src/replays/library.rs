//! The replays this client keeps for its player: saved when a match ends, listed, opened,
//! pruned to the caps the player set. The rules live here; where the bytes go is a
//! [`Backend`]. Nothing here waits: a request goes to the backend and its answer is taken in
//! by [`Library::poll`] on a later frame.

use std::sync::mpsc::{Receiver, Sender, channel};

use super::backend::{Backend, Done, Op};
use super::index::{Index, MAX_BYTES, MAX_TEXT, MIN_BYTES, Policy, Record, Wanted, clean};
use super::name;

/// What a finished request tells the menu.
#[derive(Debug, PartialEq, Eq)]
pub enum Notice {
    /// A replay is kept.
    Saved(Record),
    /// A replay could not be kept: of which match, and why.
    NotSaved { match_id: Option<u64>, why: String },
    /// A replay's bytes, asked for with [`Library::fetch`].
    #[cfg(target_arch = "wasm32")]
    Loaded { name: String, bytes: Vec<u8> },
    #[cfg(target_arch = "wasm32")]
    LoadFailed { name: String, why: String },
    /// A replay file the player picked.
    #[cfg(target_arch = "wasm32")]
    Picked { name: String, bytes: Vec<u8> },
    #[cfg(target_arch = "wasm32")]
    PickFailed(String),
}

/// Where the library stands with its store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    /// The store is being read.
    Loading,
    Ready,
    /// There is no store (a browser that keeps nothing): nothing is kept.
    Unavailable,
}

/// A replay to keep.
pub struct Saving {
    /// The player asked for this replay by name: it is kept whatever the setting for the ones
    /// kept without asking.
    pub explicit: bool,
    pub mode: String,
    pub match_id: Option<u64>,
    pub players: String,
    pub outcome: String,
    pub bytes: Vec<u8>,
}

/// Why `bytes` are no replay worth keeping, if they are not.
fn unworthy(len: u64) -> Option<&'static str> {
    if len < MIN_BYTES {
        Some("its match never started")
    } else if len > MAX_BYTES {
        Some("it is longer than a replay may be")
    } else {
        None
    }
}

pub struct Library {
    backend: Box<dyn Backend>,
    reply: Sender<Done>,
    done: Receiver<Done>,
    index: Index,
    policy: Policy,
    standing: Standing,
    /// Saves asked for before the index was read.
    waiting: Vec<(Record, Vec<u8>)>,
    notices: Vec<Notice>,
    /// Whether the index changed in the library and the store has not been told.
    changed: bool,
}

impl Library {
    /// A library over `backend`, whose files and index are read now.
    pub fn new(backend: Box<dyn Backend>) -> Library {
        let (reply, done) = channel();
        backend.run(Op::Load, reply.clone());
        Library {
            backend,
            reply,
            done,
            index: Index::default(),
            policy: Policy::default(),
            standing: Standing::Loading,
            waiting: Vec::new(),
            notices: Vec::new(),
            changed: false,
        }
    }

    /// The library of this client: the user's data folder on a desktop, the page's private
    /// storage in a browser; none where the platform has no place to keep them.
    pub fn open_default() -> Library {
        #[cfg(not(target_arch = "wasm32"))]
        return match super::default_dir() {
            Some(dir) => Library::new(Box::new(super::Disk::new(dir))),
            None => Library::none(),
        };
        #[cfg(target_arch = "wasm32")]
        Library::new(super::open_browser())
    }

    /// A library with no store: it keeps nothing.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn none() -> Library {
        let mut library = Library::new(Box::new(Nowhere));
        library.standing = Standing::Unavailable;
        library
    }

    pub fn standing(&self) -> Standing {
        self.standing
    }

    pub fn records(&self) -> &[Record] {
        &self.index.records
    }

    pub fn wanted(&self) -> &[Wanted] {
        &self.index.wanted
    }

    pub fn policy(&self) -> Policy {
        self.policy
    }

    /// The caps apply to the next save, and to the start after this one: the replays already
    /// kept are not thrown away by a setting being turned.
    pub fn set_policy(&mut self, policy: Policy) {
        self.policy = policy;
    }

    /// The file of a kept replay, where replays are files.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn path(&self, name: &str) -> Option<std::path::PathBuf> {
        self.index
            .find(name)
            .and_then(|r| self.backend.path(&r.name))
    }

    /// The folder replays are kept in, where they are files.
    pub fn dir(&self) -> Option<std::path::PathBuf> {
        self.backend.dir()
    }

    pub fn kept(&self, match_id: u64) -> Option<&Record> {
        self.index.of_match(match_id)
    }

    /// Notes a match whose replay is to be fetched, even after a restart.
    pub fn want(&mut self, wanted: Wanted) {
        if self.standing == Standing::Unavailable || !self.policy.keep {
            return;
        }
        self.index.want(wanted);
        self.changed = true;
    }

    pub fn unwant(&mut self, match_id: u64) {
        self.index.unwant(match_id);
        self.changed = true;
    }

    /// Whether a replay is saved at all now.
    pub fn keeping(&self) -> bool {
        self.policy.keep && self.standing != Standing::Unavailable
    }

    /// Keeps a replay, unless the player turned saving off or it holds no match.
    pub fn save(&mut self, saving: Saving) {
        let match_id = saving.match_id;
        if self.standing == Standing::Unavailable || (!self.policy.keep && !saving.explicit) {
            self.notices.push(Notice::NotSaved {
                match_id,
                why: "saving replays is off".to_owned(),
            });
            return;
        }
        let len = saving.bytes.len() as u64;
        if let Some(why) = unworthy(len) {
            self.notices.push(Notice::NotSaved {
                match_id,
                why: why.to_owned(),
            });
            return;
        }
        // Replays saved in one second still have an order: the later is the newer.
        let when = name::now().max(self.index.records.first().map_or(0, |r| r.when + 1));
        let record = self.record(
            &saving.mode,
            match_id,
            when,
            len,
            &saving.players,
            &saving.outcome,
        );
        match self.standing {
            Standing::Ready => self.backend.run(
                Op::Put {
                    record,
                    bytes: saving.bytes,
                },
                self.reply.clone(),
            ),
            _ => self.waiting.push((record, saving.bytes)),
        }
    }

    /// Keeps the replay a game server wrote into `from`, which becomes the library's.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn adopt(&mut self, from: std::path::PathBuf, mode: &str) {
        let len = std::fs::metadata(&from).map_or(0, |m| m.len());
        if !self.keeping() || len < MIN_BYTES {
            // Nothing worth keeping: the file is a game that never began.
            let _ = std::fs::remove_file(&from);
            return;
        }
        let record = self.record(mode, None, name::now(), len, "", "");
        self.backend
            .run(Op::Adopt { record, from }, self.reply.clone());
    }

    fn record(
        &self,
        mode: &str,
        match_id: Option<u64>,
        when: u64,
        bytes: u64,
        players: &str,
        outcome: &str,
    ) -> Record {
        let file = name::unused(name::file_name(when, mode, match_id), |n| {
            self.index.find(n).is_some()
        });
        Record {
            name: file,
            when,
            bytes,
            mode: super::index::mode_name(mode),
            match_id,
            players: clean(players, MAX_TEXT),
            outcome: clean(outcome, MAX_TEXT),
        }
    }

    /// Asks for a kept replay's bytes; they come as [`Notice::Loaded`].
    #[cfg(target_arch = "wasm32")]
    pub fn fetch(&mut self, name: &str) {
        self.backend.run(
            Op::Get {
                name: name.to_owned(),
            },
            self.reply.clone(),
        );
    }

    /// Hands a kept replay to the player as a file download.
    #[cfg(target_arch = "wasm32")]
    pub fn export(&mut self, name: &str) {
        self.backend.run(
            Op::Export {
                name: name.to_owned(),
            },
            self.reply.clone(),
        );
    }

    /// Asks the player for a replay file; it comes as [`Notice::Picked`].
    #[cfg(target_arch = "wasm32")]
    pub fn pick_file(&mut self) {
        self.backend.run(Op::PickFile, self.reply.clone());
    }

    pub fn delete(&mut self, name: &str) {
        if self.index.remove(name) {
            self.backend.run(
                Op::Remove {
                    names: vec![name.to_owned()],
                },
                self.reply.clone(),
            );
            self.changed = true;
        }
    }

    /// Takes in what the store answered, and tells the store what changed in the index.
    pub fn poll(&mut self) -> Vec<Notice> {
        while let Ok(done) = self.done.try_recv() {
            self.took(done);
        }
        if std::mem::take(&mut self.changed) && self.standing == Standing::Ready {
            self.write_index();
        }
        std::mem::take(&mut self.notices)
    }

    fn write_index(&self) {
        self.backend.run(
            Op::Index {
                text: self.index.render(),
            },
            self.reply.clone(),
        );
    }

    fn took(&mut self, done: Done) {
        match done {
            Done::Loaded { index, files } => {
                // Matches noted while the index was being read stay noted.
                let early = std::mem::take(&mut self.index.wanted);
                self.index = index.map_or_else(Index::default, |t| Index::parse(&t));
                for wanted in early {
                    self.index.want(wanted);
                }
                self.index.reconcile(&files);
                self.index.expire(name::now());
                let dropped = self.index.prune(&self.policy);
                self.standing = Standing::Ready;
                bevy::log::info!("replays: {} kept", self.index.records.len());
                if !dropped.is_empty() {
                    self.backend
                        .run(Op::Remove { names: dropped }, self.reply.clone());
                }
                self.write_index();
                for (record, bytes) in std::mem::take(&mut self.waiting) {
                    self.backend
                        .run(Op::Put { record, bytes }, self.reply.clone());
                }
            }
            Done::Put {
                record,
                error: None,
            } => {
                let dropped = self.index.insert(record.clone(), &self.policy);
                bevy::log::info!("replays: {} kept", self.index.records.len());
                self.write_index();
                if !dropped.is_empty() {
                    self.backend
                        .run(Op::Remove { names: dropped }, self.reply.clone());
                }
                self.notices.push(Notice::Saved(record));
            }
            Done::Put {
                record,
                error: Some(why),
            } => self.notices.push(Notice::NotSaved {
                match_id: record.match_id,
                why,
            }),
            #[cfg(target_arch = "wasm32")]
            Done::Got { name, result } => self.notices.push(match result {
                Ok(bytes) => Notice::Loaded { name, bytes },
                Err(why) => Notice::LoadFailed { name, why },
            }),
            #[cfg(target_arch = "wasm32")]
            Done::Picked { name, bytes } => self.notices.push(Notice::Picked { name, bytes }),
            #[cfg(target_arch = "wasm32")]
            Done::PickFailed(why) => self.notices.push(Notice::PickFailed(why)),
            Done::Failed(why) => bevy::log::warn!("replays: {why}"),
            Done::Unavailable(why) => {
                bevy::log::warn!("replays are not kept: {why}");
                self.standing = Standing::Unavailable;
                self.waiting.clear();
            }
        }
    }
}

/// The store of a platform that has none.
#[cfg(not(target_arch = "wasm32"))]
struct Nowhere;

#[cfg(not(target_arch = "wasm32"))]
impl Backend for Nowhere {
    fn run(&self, _op: Op, reply: Sender<Done>) {
        let _ = reply.send(Done::Unavailable("this browser keeps nothing".to_owned()));
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::replays::Disk;

    fn folder(what: &str) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("target/replay-tests/{what}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Polls until the library has been read.
    fn ready(library: &mut Library) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while library.standing() == Standing::Loading {
            assert!(
                std::time::Instant::now() < deadline,
                "the folder was not read"
            );
            library.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Saves `bytes` and waits for the folder to have it.
    fn keep(library: &mut Library, id: u64, bytes: usize) {
        library.save(Saving {
            explicit: false,
            mode: "solo_mid".to_owned(),
            match_id: Some(id),
            players: "a vs b".to_owned(),
            outcome: "a won, 1:00".to_owned(),
            bytes: vec![id as u8; bytes],
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            assert!(
                std::time::Instant::now() < deadline,
                "the replay was not kept"
            );
            if library
                .poll()
                .iter()
                .any(|n| matches!(n, Notice::Saved(r) if r.match_id == Some(id)))
            {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }

    /// Files in the folder: the replays and nothing else but the index.
    fn files(dir: &std::path::Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .expect("the folder")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".brp"))
            .collect();
        names.sort();
        names
    }

    /// The caps hold on disk: the oldest replays go when a new one would pass the count or
    /// the bytes, saving off keeps nothing, and the index that comes back after a restart lists
    /// exactly the files left.
    #[test]
    fn the_folder_keeps_to_the_caps_and_the_index_follows_it() {
        let dir = folder("caps");
        let mut library = Library::new(Box::new(Disk::new(dir.clone())));
        ready(&mut library);
        library.set_policy(Policy {
            keep: true,
            max_count: 3,
            max_bytes: 10 << 20,
        });
        for id in 1..=5 {
            keep(&mut library, id, 2000);
        }
        let ids = |library: &Library| -> Vec<u64> {
            library
                .records()
                .iter()
                .filter_map(|r| r.match_id)
                .collect()
        };
        assert_eq!(
            ids(&library),
            [5, 4, 3],
            "newest first, the oldest two gone"
        );
        library.poll();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while files(&dir).len() != 3 {
            assert!(std::time::Instant::now() < deadline, "{:?}", files(&dir));
            library.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        // By bytes: a cap that holds two of these.
        library.set_policy(Policy {
            keep: true,
            max_count: 50,
            max_bytes: 5000,
        });
        keep(&mut library, 6, 2000);
        assert_eq!(ids(&library), [6, 5], "the byte cap keeps two of 2000");
        // Saving off keeps nothing, and says so.
        library.set_policy(Policy {
            keep: false,
            ..library.policy()
        });
        library.save(Saving {
            explicit: false,
            mode: "m".to_owned(),
            match_id: Some(7),
            players: String::new(),
            outcome: String::new(),
            bytes: vec![0; 2000],
        });
        assert!(matches!(
            library.poll().as_slice(),
            [Notice::NotSaved {
                match_id: Some(7),
                ..
            }]
        ));
        // A replay of no match is not kept either.
        library.set_policy(Policy::default());
        library.save(Saving {
            explicit: false,
            mode: "m".to_owned(),
            match_id: Some(8),
            players: String::new(),
            outcome: String::new(),
            bytes: vec![0; 10],
        });
        assert!(matches!(
            library.poll().as_slice(),
            [Notice::NotSaved {
                match_id: Some(8),
                ..
            }]
        ));
        // The folder and a new run agree.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while files(&dir).len() != 2 {
            assert!(std::time::Instant::now() < deadline, "{:?}", files(&dir));
            library.poll();
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        drop(library);
        let mut again = Library::new(Box::new(Disk::new(dir.clone())));
        ready(&mut again);
        assert_eq!(ids(&again), [6, 5]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
