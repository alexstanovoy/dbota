//! The browser's copy of the assets: where it lives in the page's private storage (the Origin
//! Private File System) and which files of a folder the player picked the importer reads. Plain
//! logic shared by the client, which reads the copy, and the import worker, which writes it
//! (`dbota_import_web`); the browser's own calls are theirs.
//!
//! The storage holds [`ROOT`] with the copies in folders of their own and [`CURRENT`] naming the
//! finished one. An import writes a new folder and names it in [`CURRENT`] only once its index and
//! stamp are written, so a copy that was stopped, failed or cut off by a closed tab is never the
//! one read: the one before stays until the new one is whole, and what is left of the other is
//! removed when the client next starts.

/// The folder of the copies in the page's private storage.
pub const ROOT: &str = "dbota";
/// The file in [`ROOT`] naming the finished copy's folder.
pub const CURRENT: &str = "current.txt";
/// The lock an import holds (the Web Locks of the site's tabs and workers): one import at a time,
/// and a client that starts while one runs leaves its unfinished copy alone.
pub const IMPORT_LOCK: &str = "dbota-import";
/// Most bytes of [`CURRENT`].
pub const MAX_CURRENT: u64 = 64;
/// What the name of a copy's folder starts with; the rest is the time the import began.
const COPY_PREFIX: &str = "copy-";

/// The folder of a copy begun at `millis` (milliseconds of the worker's clock).
pub fn copy_name(millis: u64) -> String {
    format!("{COPY_PREFIX}{millis}")
}

/// The copy's folder [`CURRENT`] names: one [`copy_name`] made, on a line of its own.
pub fn current_copy(text: &str) -> Option<&str> {
    let name = text.strip_suffix('\n').unwrap_or(text);
    let digits = name.strip_prefix(COPY_PREFIX)?;
    let plain =
        !digits.is_empty() && digits.len() <= 20 && digits.bytes().all(|b| b.is_ascii_digit());
    plain.then_some(name)
}

/// The package directory every install has: what says a folder is Dota's.
pub const PACKAGE: &str = "pak01_dir.vpk";

/// Where `game/dota` may be below the folder a player picks with the browser's directory picker,
/// which can only go down: the folder itself, `game`, `dota 2 beta`, `common`, `steamapps` or the
/// Steam library holding them.
pub const LEVELS: [&[&str]; 6] = [
    &[],
    &["dota"],
    &["game", "dota"],
    &["dota 2 beta", "game", "dota"],
    &["common", "dota 2 beta", "game", "dota"],
    &["steamapps", "common", "dota 2 beta", "game", "dota"],
];

/// Most files of a picked folder the importer reads: Dota's has ~700 packages and typefaces.
pub const MAX_PICKED: usize = 4096;

/// The files the importer reads of a folder picked whole (`<input webkitdirectory>` lists every
/// file below it, by its path from the picked folder, `/`-separated): the index of each in `paths`
/// with its path relative to the `game/dota` folder, the one holding [`PACKAGE`] closest to the
/// top. Only what [`crate::source::picked`] names is kept, at most [`MAX_PICKED`]; nothing when no
/// folder below holds the package directory.
pub fn picked_files<'a>(paths: impl Iterator<Item = &'a str> + Clone) -> Vec<(usize, String)> {
    let base = paths
        .clone()
        .filter_map(|path| path.strip_suffix(PACKAGE))
        .filter(|dir| dir.is_empty() || dir.ends_with('/'))
        .min_by_key(|dir| (dir.matches('/').count(), dir.len()));
    let Some(base) = base else {
        return Vec::new();
    };
    paths
        .enumerate()
        .filter_map(|(at, path)| {
            let relative = path.strip_prefix(base)?;
            crate::source::picked(relative).then(|| (at, relative.to_owned()))
        })
        .take(MAX_PICKED)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picked(paths: &[&str]) -> Vec<String> {
        let mut found: Vec<String> = picked_files(paths.iter().copied())
            .into_iter()
            .map(|(at, relative)| format!("{at}:{relative}"))
            .collect();
        found.sort();
        found
    }

    #[test]
    fn a_picked_folder_gives_the_packages_and_typefaces_of_its_topmost_dota() {
        let got = picked(&[
            "dota 2 beta/game/dota/pak01_dir.vpk",
            "dota 2 beta/game/dota/pak01_000.vpk",
            "dota 2 beta/game/dota/maps/dota.vpk",
            "dota 2 beta/game/dota/maps/deep/x.vpk",
            "dota 2 beta/game/dota/addons/pak01_dir.vpk",
            "dota 2 beta/game/dota/cfg.txt",
            "dota 2 beta/game/bin/pak01_001.vpk",
            "dota 2 beta/game/dota/panorama/fonts/reaver-bold.otf",
            "dota 2 beta/game/dota/panorama/fonts/thai.uifont",
            "dota 2 beta/game/dota/panorama/fonts/deep/x.otf",
            "dota 2 beta/game/dota/panorama/x.otf",
        ]);
        assert_eq!(
            got,
            [
                "0:pak01_dir.vpk",
                "1:pak01_000.vpk",
                "2:maps/dota.vpk",
                "7:panorama/fonts/reaver-bold.otf"
            ]
        );
    }

    #[test]
    fn a_folder_without_the_package_directory_gives_nothing() {
        assert!(picked(&["dota 2 beta/readme.txt", "dota 2 beta/game/x_pak01_dir.vpk"]).is_empty());
    }

    #[test]
    fn the_picked_game_dota_folder_itself_is_read() {
        assert_eq!(
            picked(&["dota/pak01_dir.vpk", "dota/pak01_002.vpk"]),
            ["0:pak01_dir.vpk", "1:pak01_002.vpk"]
        );
        assert_eq!(picked(&["pak01_dir.vpk"]), ["0:pak01_dir.vpk"]);
    }

    #[test]
    fn a_picked_folder_gives_at_most_the_bound_of_files() {
        let names: Vec<String> = std::iter::once("d/pak01_dir.vpk".to_owned())
            .chain((0..MAX_PICKED + 10).map(|n| format!("d/pak01_{n:03}.vpk")))
            .collect();
        assert_eq!(
            picked_files(names.iter().map(String::as_str)).len(),
            MAX_PICKED
        );
    }

    #[test]
    fn only_a_copy_name_is_taken_from_the_current_file() {
        assert_eq!(
            current_copy("copy-1700000000000\n"),
            Some("copy-1700000000000")
        );
        assert_eq!(current_copy(&copy_name(5)), Some("copy-5"));
        for wrong in [
            "",
            "copy-",
            "copy-12a",
            "../copy-1",
            "copy-1/..",
            "copy-1\n\n",
            "stamp.txt",
        ] {
            assert_eq!(current_copy(wrong), None, "{wrong:?}");
        }
    }
}
