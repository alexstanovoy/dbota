//! Where the client keeps what outlives a run: the settings file, the menu's
//! own choices, and the session token that logs the account in again. Files
//! under `~/.config/dbota` on the desktop (the token readable by its owner
//! only), the page's `localStorage` in a browser.

use bevy::prelude::Resource;

use crate::config::Settings;

/// Largest entry read back; a bigger one is ignored as damaged.
const MAX_ENTRY: usize = 64 * 1024;

/// One thing the store keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Entry {
    /// The settings file (`client.conf`).
    Settings,
    /// The menu's choices: the service, the last login, direct addresses.
    Menu,
    /// The token of the last login and the service it is for.
    Session,
}

#[cfg(target_arch = "wasm32")]
impl Entry {
    fn key(self) -> &'static str {
        match self {
            Entry::Settings => "dbota.client.conf",
            Entry::Menu => "dbota.menu",
            Entry::Session => "dbota.session",
        }
    }
}

/// The client's persistent store.
#[derive(Resource, Clone, Debug, Default)]
pub struct SettingsStore {
    /// The settings file; the other entries live beside it.
    #[cfg(not(target_arch = "wasm32"))]
    settings: Option<std::path::PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
impl SettingsStore {
    /// A store around the settings file `settings`, or none at all.
    pub fn new(settings: Option<std::path::PathBuf>) -> SettingsStore {
        SettingsStore { settings }
    }

    fn path(&self, entry: Entry) -> Option<std::path::PathBuf> {
        let settings = self.settings.as_ref()?;
        Some(match entry {
            Entry::Settings => settings.clone(),
            Entry::Menu => settings.with_file_name("menu.conf"),
            Entry::Session => settings.with_file_name("session"),
        })
    }

    pub fn read(&self, entry: Entry) -> Option<String> {
        use std::io::Read;
        let path = self.path(entry)?;
        let file = std::fs::File::open(&path).ok()?;
        let mut text = String::new();
        let read = file.take(MAX_ENTRY as u64 + 1).read_to_string(&mut text);
        (read.is_ok() && text.len() <= MAX_ENTRY).then_some(text)
    }

    /// Replaces an entry whole. The session token is written readable by
    /// its owner only, through a fresh file renamed into place.
    pub fn write(&self, entry: Entry, text: &str) -> std::io::Result<()> {
        let Some(path) = self.path(entry) else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let fresh = path.with_extension("new");
        let _ = std::fs::remove_file(&fresh);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if entry == Entry::Session {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        {
            use std::io::Write;
            let mut file = options.open(&fresh)?;
            file.write_all(text.as_bytes())?;
        }
        std::fs::rename(&fresh, &path)
    }

    pub fn remove(&self, entry: Entry) {
        if let Some(path) = self.path(entry) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl SettingsStore {
    /// The page's `localStorage`; there are no files.
    pub fn new(_settings: Option<std::path::PathBuf>) -> SettingsStore {
        SettingsStore {}
    }

    fn storage() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn read(&self, entry: Entry) -> Option<String> {
        let text = Self::storage()?.get_item(entry.key()).ok().flatten()?;
        (text.len() <= MAX_ENTRY).then_some(text)
    }

    pub fn write(&self, entry: Entry, text: &str) -> std::io::Result<()> {
        let storage = Self::storage().ok_or_else(|| std::io::Error::other("no localStorage"))?;
        storage
            .set_item(entry.key(), text)
            .map_err(|_| std::io::Error::other("localStorage refused the entry"))
    }

    pub fn remove(&self, entry: Entry) {
        if let Some(storage) = Self::storage() {
            let _ = storage.remove_item(entry.key());
        }
    }
}

impl SettingsStore {
    /// The settings, with every complaint about the file printed.
    pub fn load_settings(&self) -> Settings {
        let Some(text) = self.read(Entry::Settings) else {
            return Settings::default();
        };
        let (settings, complaints) = Settings::parse(&text);
        for complaint in complaints {
            eprintln!("dbota settings: {complaint}");
        }
        settings
    }

    pub fn save_settings(&self, settings: &Settings) -> std::io::Result<()> {
        self.write(Entry::Settings, &settings.render())
    }
}
