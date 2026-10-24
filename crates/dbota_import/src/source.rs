//! Where the importer reads an install: its `game/dota` folder, and its
//! addons' loose files beside it (`game/dota_addons`, as Dota's own Demo
//! Hero), named under [`ADDONS`].

/// Where a path names a loose file of one of the install's addons rather
/// than a file under `game/dota`: `addons/hero_demo/...`.
pub const ADDONS: &str = "addons/";

#[cfg(any(unix, windows))]
pub use native::InstallSource;

#[cfg(any(unix, windows))]
mod native {
    use std::path::Path;

    use heatel::{DirSource, Result, Source};

    /// A Dota install: `game/dota`, and its addons' loose files under
    /// [`super::ADDONS`] (from `game/dota_addons`).
    pub struct InstallSource {
        game: DirSource,
        addons: DirSource,
    }

    impl InstallSource {
        /// The install whose `game/dota` folder is `game`.
        pub fn new(game: &Path) -> Self {
            let addons = game
                .parent()
                .map_or_else(|| game.join("dota_addons"), |up| up.join("dota_addons"));
            Self {
                game: DirSource::new(game),
                addons: DirSource::new(addons),
            }
        }

        fn which<'a>(&self, path: &'a str) -> (&DirSource, &'a str) {
            match path.strip_prefix(super::ADDONS) {
                Some(rest) => (&self.addons, rest),
                None => (&self.game, path),
            }
        }
    }

    impl Source for InstallSource {
        fn size(&self, path: &str) -> Result<u64> {
            let (dir, path) = self.which(path);
            dir.size(path)
        }

        fn read_at(&self, path: &str, offset: u64, len: usize) -> Result<Vec<u8>> {
            let (dir, path) = self.which(path);
            dir.read_at(path, offset, len)
        }
    }
}
