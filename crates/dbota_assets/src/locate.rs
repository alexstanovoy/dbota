use std::path::{Path, PathBuf};

/// The `game/dota` folder of a Dota 2 install: under `dota_path` (a `dota 2 beta` folder) when
/// given, else the first Steam library that has one (Flatpak Steam, `~/.steam/steam`,
/// `~/.local/share/Steam`).
pub fn find_game_dir(dota_path: Option<&Path>) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let steam_libraries = [
        ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ".steam/steam",
        ".local/share/Steam",
    ];
    let installs = steam_libraries.iter().filter_map(|library| {
        Some(
            home.as_ref()?
                .join(library)
                .join("steamapps/common/dota 2 beta"),
        )
    });
    dota_path
        .map(Path::to_path_buf)
        .into_iter()
        .chain(installs)
        .map(|install| install.join("game/dota"))
        .find(|game| game.join("pak01_dir.vpk").is_file())
}
