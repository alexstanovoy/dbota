//! Test client construction stays behind the isolation wrappers.
mod common;

use std::path::{Path, PathBuf};

#[cfg(unix)]
#[test]
fn shell_test_client_uses_and_cleans_a_private_home() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = common::prepared::Scratch::new("shell-client");
    let tools = scratch.path("tools");
    let target = scratch.path("target");
    let release = target.join("release");
    let cache = scratch.path("cache");
    std::fs::create_dir_all(&tools).expect("tools directory");
    std::fs::create_dir_all(&release).expect("binary directory");
    std::fs::create_dir_all(&cache).expect("cache directory");
    std::fs::write(cache.join("index.txt"), "").expect("cache index");
    let wrapper = tools.join("test_client.sh");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/test_client.sh"),
        &wrapper,
    )
    .expect("wrapper copy");
    let binary = release.join("dbota");
    std::fs::write(&binary, "#!/bin/sh\nprintf '%s\\n' \"$HOME\" \"$XDG_CONFIG_HOME\" \"$XDG_DATA_HOME\" \"$XDG_CACHE_HOME\" \"${BOTA_SERVER-unset}\"\n").expect("probe");
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
        .expect("probe executable");
    let output = std::process::Command::new("bash")
        .arg(&wrapper)
        .env("HOME", scratch.path("real-home"))
        .env("BOTA_SERVER", "/untrusted/server")
        .env("DBOTA_ASSETS", &cache)
        .env_remove("CARGO_TARGET_DIR")
        .output()
        .expect("wrapper starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).expect("probe output");
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(lines.len(), 5, "{lines:?}");
    let home = Path::new(lines[0]);
    assert!(home.starts_with(&target));
    assert_eq!(Path::new(lines[1]), home.join("config"));
    assert_eq!(Path::new(lines[2]), home.join("data"));
    assert_eq!(Path::new(lines[3]), target.join("tmp/gpu-cache"));
    assert_eq!(lines[4], "unset");
    assert!(!home.exists(), "private home remains");
}

#[test]
fn shell_test_client_refuses_to_run_without_an_asset_cache() {
    let output = std::process::Command::new(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/test_client.sh"),
    )
    .arg("--version")
    .env_remove("DBOTA_ASSETS")
    .output()
    .expect("test client wrapper starts");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("DBOTA_ASSETS must name a test asset cache")
    );
}

fn compact(source: &str) -> String {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//") && !line.trim_start().starts_with('#'))
        .flat_map(str::chars)
        .filter(|character| character.is_ascii_alphanumeric() || *character == '_')
        .collect()
}

fn forbidden(source: &str) -> bool {
    let text = compact(source);
    let binary = ["CARGO", "BIN", "EXE", "dbota"].join("_");
    let prefix = ["target", "release"].join("");
    let path = [prefix.as_str(), "dbota"].join("");
    let getter = ["get", "program"].join("_");
    text.contains(&binary) || text.contains(&path) || text.contains(&getter)
}

#[test]
fn guard_detects_direct_concatenated_wrapped_and_hard_coded_client_paths() {
    let binary = ["CARGO", "BIN", "EXE", "dbota"].join("_");
    let split = format!(
        "concat!(\"{}\", \"{}\")",
        &binary[..binary.len() - 5],
        "dbota"
    );
    let getter = ["get", "program"].join("_");
    let prefix = ["target", "release"].join("/");
    let path = [prefix.as_str(), "dbota"].join("/");
    for source in [
        binary,
        split,
        format!("Command::new(common::client().{getter}())"),
        path,
    ] {
        assert!(forbidden(&source), "the guard missed {source}");
    }
}

#[test]
fn client_launches_use_the_isolation_wrappers() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut directories = vec![
        root.join("tests"),
        root.join("src"),
        root.join("tools"),
        root.join("crates"),
    ];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).expect("source directory") {
            let entry = entry.expect("source entry");
            let path = entry.path();
            if entry.file_type().expect("entry type").is_dir() {
                directories.push(path);
                continue;
            }
            if !matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("rs" | "sh" | "py")
            ) {
                continue;
            }
            let mut source = std::fs::read_to_string(&path).expect("source text");
            if path == root.join("tests/common/mod.rs") {
                let start = source
                    .find("fn isolated_client(")
                    .expect("client constructor");
                let end = source[start..]
                    .find("pub fn asset_cache()")
                    .expect("constructor end")
                    + start;
                let chunk = &source[start..end];
                let binary = ["CARGO", "BIN", "EXE", "dbota"].join("_");
                assert_eq!(chunk.matches(&binary).count(), 3);
                let approved = chunk.replace(&binary, "APPROVED_BINARY");
                source.replace_range(start..end, &approved);
            }
            if path == root.join("tools/test_client.sh") {
                let prefix = ["target", "release"].join("/");
                let allowed = [prefix.as_str(), "dbota"].join("/");
                assert_eq!(source.matches(&allowed).count(), 1);
                source = source.replace(&allowed, "approved-binary");
            }
            assert!(
                !forbidden(&source),
                "{} bypasses client isolation",
                path.display()
            );
        }
    }
}

#[test]
fn environment_reads_are_accounted_for_by_the_client_wrapper() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut directories = vec![root.clone()];
    let mut discovered = std::collections::BTreeSet::new();
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).expect("source directory") {
            let entry = entry.expect("source entry");
            let path = entry.path();
            if entry.file_type().expect("entry type").is_dir() {
                if path != root.join("e2e") {
                    directories.push(path);
                }
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
                || path.file_name().is_some_and(|name| name == "tests.rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("source");
            let production = source
                .split("#[cfg(test)]\nmod tests")
                .next()
                .unwrap_or(&source)
                .split("#[cfg(all(test")
                .next()
                .unwrap_or(&source);
            for (at, _) in production.match_indices("env::var") {
                let tail = &production[at..];
                let Some(argument) = tail.split_once('(').map(|(_, rest)| rest) else {
                    continue;
                };
                let name = if let Some(quoted) = argument.strip_prefix('"') {
                    quoted.split('"').next().unwrap_or("")
                } else if argument.starts_with("SEAT_KEY_VAR") {
                    "BOTA_SEAT_KEY"
                } else {
                    continue;
                };
                if (name.starts_with("BOTA_") || name.starts_with("DBOTA_"))
                    && name != "DBOTA_ASSETS"
                {
                    discovered.insert(name.to_owned());
                    assert!(
                        common::CLIENT_ENVIRONMENT.contains(&name),
                        "{} reads unhandled {name}",
                        path.display()
                    );
                }
            }
            for (at, _) in production.match_indices("env = \"DBOTA_") {
                let name = production[at..]
                    .trim_start_matches("env = \"")
                    .split('"')
                    .next()
                    .unwrap_or("");
                discovered.insert(name.to_owned());
                assert!(
                    common::CLIENT_ENVIRONMENT.contains(&name),
                    "{} reads unhandled {name}",
                    path.display()
                );
            }
        }
    }
    for expected in [
        "BOTA_SERVER",
        "BOTA_SEAT_KEY",
        "DBOTA_SERVICE",
        "DBOTA_DRILL",
        "DBOTA_DRILL_SHOTS",
    ] {
        assert!(
            discovered.contains(expected),
            "environment audit did not find {expected}"
        );
    }
}
