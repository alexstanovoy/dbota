//! Where replays are kept, apart from the rules of what is kept: a folder on a desktop, the
//! browser's private storage in a page. A backend does what it is told in the order it is told
//! and answers on a channel the library polls, so nothing waits on a disk.

use std::sync::mpsc::Sender;

use super::index::Record;

/// What the library asks of a store.
pub enum Op {
    /// Lists the files and reads the index.
    Load,
    /// Keeps a replay.
    Put {
        record: Record,
        bytes: Vec<u8>,
    },
    /// Keeps a replay a game server wrote into a file of its own, which then belongs to the
    /// library (a desktop's local games).
    #[cfg(not(target_arch = "wasm32"))]
    Adopt {
        record: Record,
        from: std::path::PathBuf,
    },
    /// A replay's bytes (a browser reads them; a desktop opens the file).
    #[cfg(target_arch = "wasm32")]
    Get {
        name: String,
    },
    Remove {
        names: Vec<String>,
    },
    /// Replaces the index.
    Index {
        text: String,
    },
    /// Hands a kept replay to the player as a file download.
    #[cfg(target_arch = "wasm32")]
    Export {
        name: String,
    },
    /// Asks the player for a replay file.
    #[cfg(target_arch = "wasm32")]
    PickFile,
}

/// What a store answers.
pub enum Done {
    /// The files `(name, bytes, modified)` and the index text, if there is one.
    Loaded {
        index: Option<String>,
        files: Vec<(String, u64, u64)>,
    },
    /// A replay kept, with its size as stored; or why not.
    Put {
        record: Record,
        /// Why it was not kept.
        error: Option<String>,
    },
    #[cfg(target_arch = "wasm32")]
    Got {
        name: String,
        result: Result<Vec<u8>, String>,
    },
    /// A replay file the player picked.
    #[cfg(target_arch = "wasm32")]
    Picked { name: String, bytes: Vec<u8> },
    #[cfg(target_arch = "wasm32")]
    PickFailed(String),
    /// A removal or an index that failed (the library goes on with the index it has).
    Failed(String),
    /// The store cannot be used at all.
    Unavailable(String),
}

/// A place replays are kept.
pub trait Backend {
    /// Does `op` after the ones before it, and answers on `reply`.
    fn run(&self, op: Op, reply: Sender<Done>);

    /// The file of a kept replay, where replays are files.
    #[cfg(not(target_arch = "wasm32"))]
    fn path(&self, _name: &str) -> Option<std::path::PathBuf> {
        None
    }

    /// The folder replays are kept in, where they are files.
    fn dir(&self) -> Option<std::path::PathBuf> {
        None
    }
}
