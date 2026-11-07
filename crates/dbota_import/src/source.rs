//! Where the importer reads an install: its `game/dota` folder, and its
//! addons' loose files beside it (`game/dota_addons`, as Dota's own Demo
//! Hero), named under [`ADDONS`].

/// Where a path names a loose file of one of the install's addons rather
/// than a file under `game/dota`: `addons/hero_demo/...`.
pub const ADDONS: &str = "addons/";

/// The folder of the install's typefaces, under `game/dota`.
const FONTS: &str = "panorama/fonts/";

/// Whether a browser hands the importer the install's file at `path` (relative to `game/dota`):
/// a package at the top or in `maps/`, or a typeface of `panorama/fonts/`. A browser's folder
/// grant reads nothing else of the player's folder.
pub fn picked(path: &str) -> bool {
    let depth = path.matches('/').count();
    (path.ends_with(".vpk") && (depth == 0 || (depth == 1 && path.starts_with("maps/"))))
        || (path.ends_with(".otf") && depth == 2 && path.starts_with(FONTS))
}

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

#[cfg(test)]
mod tests {
    /// The packages and the typefaces the manifest reads are taken, nothing else of the folder.
    #[test]
    fn a_browser_takes_the_packages_and_the_typefaces_only() {
        for taken in [
            "pak01_dir.vpk",
            "pak01_007.vpk",
            "maps/dota.vpk",
            "panorama/fonts/reaver-bold.otf",
        ] {
            assert!(super::picked(taken), "{taken}");
        }
        for left in [
            "maps/deep/x.vpk",
            "cfg/pak01_dir.vpk",
            "panorama/fonts/deep/x.otf",
            "panorama/fonts/thai.uifont",
            "panorama/x.otf",
            "fonts/x.otf",
            "gameinfo.gi",
        ] {
            assert!(!super::picked(left), "{left}");
        }
    }
}
