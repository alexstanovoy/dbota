//! Whatever starts, drives or measures the client stays behind the isolation wrappers: it never
//! reads or writes the user's HOME, XDG folders, replays (the library prunes the oldest beyond
//! its limits) or managed asset cache, and never inherits a variable that steers the client.
mod common;

use common::source_scan::{client_launches, environment_violations, names_the_managed_cache};
use std::path::{Path, PathBuf};

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A copy of the shell tools into `scratch/tools`, to run them beside a target folder of the test's.
#[cfg(unix)]
fn copy_tools(scratch: &common::prepared::Scratch, names: &[&str]) -> PathBuf {
    let tools = scratch.path("tools");
    std::fs::create_dir_all(&tools).expect("tools directory");
    for name in names
        .iter()
        .chain(&["client_env.sh", "client_environment.txt"])
    {
        std::fs::copy(manifest().join("tools").join(name), tools.join(name)).expect("tool copy");
    }
    tools
}

#[cfg(unix)]
fn executable(path: &Path, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, script).expect("script");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("mode");
}

/// A shell command line that prints one line per variable of the client's environment.
#[cfg(unix)]
fn print_variables() -> String {
    let steering: String = common::client_environment()
        .map(|name| format!(" \"${{{name}-unset}}\""))
        .collect();
    format!(
        "printf '%s\\n' \"$HOME\" \"$XDG_CONFIG_HOME\" \"$XDG_DATA_HOME\" \"$XDG_CACHE_HOME\"{steering}"
    )
}

#[cfg(unix)]
fn untrusted_environment(command: &mut std::process::Command) {
    for name in common::client_environment() {
        command.env(name, "/untrusted");
    }
}

#[cfg(unix)]
#[test]
fn shell_test_client_uses_and_cleans_a_private_home() {
    let scratch = common::prepared::Scratch::new("shell-client");
    let tools = copy_tools(&scratch, &["test_client.sh"]);
    let target = scratch.path("target");
    let cache = scratch.path("cache");
    std::fs::create_dir_all(target.join("release")).expect("binary directory");
    std::fs::create_dir_all(&cache).expect("cache directory");
    std::fs::write(cache.join("index.txt"), "").expect("cache index");
    executable(
        &target.join("release/dbota"),
        &format!("#!/bin/sh\n{}\n", print_variables()),
    );
    let mut wrapper = std::process::Command::new("bash");
    wrapper
        .arg(tools.join("test_client.sh"))
        .env("HOME", scratch.path("real-home"))
        .env("DBOTA_ASSETS", &cache)
        .env_remove("CARGO_TARGET_DIR");
    untrusted_environment(&mut wrapper);
    let output = wrapper.output().expect("wrapper starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = String::from_utf8(output.stdout).expect("probe output");
    let lines: Vec<_> = output.lines().collect();
    assert_eq!(
        lines.len(),
        4 + common::client_environment().count(),
        "{lines:?}"
    );
    let home = Path::new(lines[0]);
    assert!(home.starts_with(&target));
    assert_eq!(Path::new(lines[1]), home.join("config"));
    assert_eq!(Path::new(lines[2]), home.join("data"));
    assert_eq!(Path::new(lines[3]), target.join("tmp/gpu-cache"));
    assert!(
        lines[4..].iter().all(|line| *line == "unset"),
        "a steering variable reached the client: {lines:?}"
    );
    assert!(!home.exists(), "private home remains");
}

#[cfg(unix)]
#[test]
fn shell_test_client_runs_the_binary_of_the_cargo_target_dir() {
    let scratch = common::prepared::Scratch::new("shell-client-target");
    let tools = copy_tools(&scratch, &["test_client.sh"]);
    let target = scratch.path("elsewhere");
    let cache = scratch.path("cache");
    std::fs::create_dir_all(target.join("release")).expect("binary directory");
    std::fs::create_dir_all(&cache).expect("cache directory");
    std::fs::write(cache.join("index.txt"), "").expect("cache index");
    executable(&target.join("release/dbota"), "#!/bin/sh\necho \"$HOME\"\n");
    let output = std::process::Command::new("bash")
        .arg(tools.join("test_client.sh"))
        .env("DBOTA_ASSETS", &cache)
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .expect("wrapper starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let home = String::from_utf8(output.stdout).expect("probe output");
    assert!(
        Path::new(home.trim()).starts_with(&target),
        "the private home is under the target folder that holds the binary: {home}"
    );
}

#[test]
fn shell_test_client_refuses_to_run_without_an_asset_cache() {
    let output = std::process::Command::new(manifest().join("tools/test_client.sh"))
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

/// Runs a copy of `tools/bench.sh` on a stub client: the real binary of the run is `true`, and
/// `BENCH_WRAP` swaps it for a script that prints the environment it got in a `bench:` line.
#[cfg(unix)]
fn bench(scratch: &common::prepared::Scratch, assets: Option<&Path>) -> std::process::Output {
    let tools = copy_tools(scratch, &["bench.sh", "headless.sh"]);
    let stub = scratch.path("stub.sh");
    executable(
        &stub,
        &format!(
            "#!/bin/sh\n{} > \"$0.env\"\necho 'bench: mean_ms=1 p99_ms=1 cpu_pct=1 rss_mb=1 peak_rss_mb=1 first_frame_s=1 ready_s=1'\n",
            print_variables()
        ),
    );
    let replay = scratch.path("replay.brp");
    std::fs::write(&replay, "").expect("replay");
    let mut command = std::process::Command::new("bash");
    command
        .arg(tools.join("bench.sh"))
        .arg(&replay)
        .current_dir(scratch.path(""))
        .env("DBOTA_HEADLESS", "1")
        .env("HOME", scratch.path("real-home"))
        .env("CARGO_TARGET_DIR", scratch.path("target"))
        .env("DBOTA_BIN", "/bin/true")
        .env("BENCH_BUILD", "0")
        .env("BENCH_RUNS", "1")
        .env("BENCH_VSYNC", "0")
        .env("BENCH_SERVER", "")
        .env("BENCH_WRAP", &stub)
        .env_remove("DBOTA_ASSETS");
    if let Some(assets) = assets {
        command.env("DBOTA_ASSETS", assets);
    }
    untrusted_environment(&mut command);
    command.output().expect("bench starts")
}

#[cfg(unix)]
#[test]
fn bench_runs_the_client_on_a_private_home_without_steering_variables() {
    let scratch = common::prepared::Scratch::new("bench-private");
    let cache = scratch.path("cache");
    std::fs::create_dir_all(&cache).expect("cache directory");
    std::fs::write(cache.join("index.txt"), "").expect("cache index");
    let output = bench(&scratch, Some(&cache));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let seen = std::fs::read_to_string(scratch.path("stub.sh.env")).expect("the client ran");
    let lines: Vec<_> = seen.lines().collect();
    let home = Path::new(lines[0]);
    assert!(home.starts_with(scratch.path("target")), "{lines:?}");
    assert_eq!(Path::new(lines[1]), home.join("config"));
    assert_eq!(Path::new(lines[2]), home.join("data"));
    assert!(
        lines[4..].iter().all(|line| *line == "unset"),
        "a steering variable reached the client: {lines:?}"
    );
    assert!(!home.exists(), "private home remains");
}

#[cfg(unix)]
#[test]
fn bench_refuses_to_run_without_an_asset_cache_instead_of_taking_the_managed_one() {
    let scratch = common::prepared::Scratch::new("bench-no-cache");
    let output = bench(&scratch, None);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("DBOTA_ASSETS must name a test asset cache"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!scratch.path("stub.sh.env").exists(), "a client ran");
}

/// Every Rust, shell and Python file of the repository's own tests and tools, and the
/// client's sources, by path relative to the repository.
fn repository_sources() -> Vec<(String, String)> {
    let root = manifest();
    let mut directories = vec![
        root.join("tests"),
        root.join("src"),
        root.join("tools"),
        root.join("crates"),
        root.join("web"),
    ];
    let mut files = vec![root.join("build.rs")];
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).expect("source directory") {
            let entry = entry.expect("source entry");
            let path = entry.path();
            if entry.file_type().expect("entry type").is_dir() {
                if !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some("target" | "node_modules" | "pkg" | "dist")
                ) {
                    directories.push(path);
                }
            } else if matches!(
                path.extension().and_then(|extension| extension.to_str()),
                Some("rs" | "sh" | "py")
            ) {
                files.push(path);
            }
        }
    }
    files.sort();
    files
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path).expect("source text");
            let relative = path.strip_prefix(&root).expect("inside the repository");
            (relative.to_string_lossy().into_owned(), text)
        })
        .collect()
}

/// Where the guard finds the client started as it is built, with how many times and why that is
/// no problem: either it is isolated, or the guard's squeezed reading takes a name for a launch.
const APPROVED_LAUNCHES: &[(&str, usize, &str)] = &[
    (
        "tests/common/mod.rs",
        3,
        "the isolated client: private HOME and XDG folders, scrubbed variables",
    ),
    (
        "tools/client_env.sh",
        1,
        "client_binary names the binary the isolating tools run",
    ),
    (
        "tools/precheck.sh",
        1,
        "`import` reads the user's Dota install and writes only the explicit DBOTA_ASSETS",
    ),
    (
        "tools/gate.sh",
        2,
        "`import` as above; bota's local_match example, not the client",
    ),
    ("tools/build.sh", 1, "an echo of the paths it built"),
    ("web/build.sh", 2, "dbota.wasm, not the native client"),
];

#[test]
fn client_launches_use_the_isolation_wrappers() {
    for (path, text) in repository_sources() {
        if path == "tests/client_guard.rs" || path == "tests/common/source_scan.rs" {
            continue; // they list the forms they look for
        }
        let approved = APPROVED_LAUNCHES
            .iter()
            .find(|(approved, ..)| *approved == path)
            .map_or(0, |(_, count, _)| *count);
        assert_eq!(
            client_launches(&text),
            approved,
            "{path} starts the client outside the isolation wrappers (or APPROVED_LAUNCHES is stale)"
        );
        if !path.starts_with("src/") {
            assert!(
                !names_the_managed_cache(&text),
                "{path} names the user's managed asset cache ~/.cache/dbota: take an explicit DBOTA_ASSETS"
            );
        }
    }
}

#[test]
fn the_launch_guard_sees_every_way_to_start_the_client() {
    let binary = ["CARGO", "BIN", "EXE", "dbota"].join("_");
    let split = format!("concat!(\"{}\", \"dbota\")", &binary[..binary.len() - 5]);
    let wrapped = format!("Command::new(common::client().{}())", "get_program");
    for source in [
        binary,
        split,
        wrapped,
        "target/release/dbota".to_owned(),
        "\"$here/target/$profile/dbota\" --mute".to_owned(),
        "\"$CARGO_TARGET_DIR/release/dbota\" import".to_owned(),
        "bin=${DBOTA_BIN:-${CARGO_TARGET_DIR:-$here/target}/${BENCH_PROFILE:-release}/dbota}"
            .to_owned(),
        "$here/target/dist/dbota".to_owned(),
        "root.join(\"target\").join(\"release\").join(\"dbota\")".to_owned(),
        "cargo run --release -q -- import".to_owned(),
        "Command::new(\"dbota\")".to_owned(),
    ] {
        assert!(client_launches(&source) > 0, "the guard missed {source}");
    }
    for source in [
        "// target/release/dbota",
        "# cargo run",
        "\"$here/tools/test_client.sh\" --replay x",
        "target/wasm32-unknown-unknown/web/dbota.wasm",
        "cargo build --release",
    ] {
        assert_eq!(client_launches(source), 0, "the guard flags {source}");
    }
}

#[test]
fn the_cache_guard_sees_the_managed_cache_and_not_the_gates() {
    for source in [
        "pathlib.Path(\"~/.cache/dbota\").expanduser()",
        "cache=$HOME/.cache/dbota/v6",
        "\"~/.cache/dbota\"",
    ] {
        assert!(names_the_managed_cache(source), "the guard missed {source}");
    }
    for source in [
        "${XDG_CACHE_HOME:-$HOME/.cache}/dbota-gate/pwlib",
        "/// cp -rs ~/.cache/dbota/v6/. /path/to/copy",
        "# ~/.cache/dbota/tools",
    ] {
        assert!(!names_the_managed_cache(source), "the guard flags {source}");
    }
}

/// Variables the client and its tests name that need no scrubbing, and why.
const KNOWN_VARIABLES: &[(&str, &str)] = &[
    (
        "DBOTA_ASSETS",
        "every harness names its test asset cache explicitly",
    ),
    (
        "DBOTA_DOTA_PATH",
        "a test names the Dota install it reads, which stays untouched",
    ),
    ("BOTA_SERVER_BIN", "a test names a ready game server binary"),
    (
        "DBOTA_SHOTS",
        "a test names a folder for screenshots to keep",
    ),
    (
        "DBOTA_QUIT_SHOTS",
        "a test names a folder for screenshots to keep",
    ),
    ("DBOTA_HEADLESS_SIZE", "the size of the private compositor"),
    (
        "DBOTA_WEB_REPLAY",
        "read when the browser build is compiled",
    ),
    ("DBOTA_WEB_ARGS", "read when the browser build is compiled"),
    ("DBOTA_BUILD", "read when the client is compiled"),
    (
        "BOTA_SEATS",
        "removed from the environment of a game server the client starts",
    ),
    (
        "BOTA_REFEREE_KEY",
        "removed from the environment of a game server the client starts",
    ),
];

/// Files that resolve a default location from the environment, and so have to be given explicit
/// paths by every harness (the in-process one passes them on the command line).
const LOCATION_RESOLVERS: &[&str] = &[
    "src/config.rs",
    "src/replays/disk.rs",
    "src/import.rs",
    "src/main.rs",
    "src/assets_notice.rs",
];

fn environment_problems(path: &str, source: &str) -> Vec<String> {
    let resolver = LOCATION_RESOLVERS.contains(&path);
    environment_violations(source, resolver, |name| {
        common::client_environment().any(|handled| handled == name)
            || KNOWN_VARIABLES.iter().any(|(known, _)| *known == name)
    })
    .into_iter()
    .map(|problem| format!("{path}: {problem}"))
    .collect()
}

#[test]
fn environment_names_are_accounted_for_by_the_client_wrapper() {
    let mut problems = Vec::new();
    let mut discovered = std::collections::BTreeSet::new();
    for (path, text) in repository_sources() {
        let inspected =
            path.ends_with(".rs") && (path.starts_with("src/") || path.starts_with("tests/"));
        if !inspected || path == "tests/client_guard.rs" || path == "tests/common/source_scan.rs" {
            continue; // these list the names they look for
        }
        problems.extend(environment_problems(&path, &text));
        discovered.extend(
            common::source_scan::literals(&text)
                .into_iter()
                .map(|l| l.text),
        );
    }
    assert!(problems.is_empty(), "{problems:#?}");
    for expected in common::client_environment() {
        assert!(
            discovered.contains(expected),
            "the audit never met {expected}: the list in tools/client_environment.txt is stale or the audit is blind"
        );
    }
}

#[test]
fn the_environment_audit_sees_every_way_a_source_can_name_a_variable() {
    let late_in_file =
        "#[cfg(test)]\nmod tests {\n    fn t() {}\n}\nfn later() { std::env::var(\"BOTA_LATE\"); }";
    let cases = [
        (
            "direct",
            "fn f() { std::env::var(\"BOTA_NEW\"); }",
            "BOTA_NEW",
        ),
        (
            "a constant",
            "const KEY: &str = \"DBOTA_OTHER\"; fn f() { std::env::var(KEY); }",
            "DBOTA_OTHER",
        ),
        (
            "a clap attribute",
            "struct A { #[arg(long, env = \"DBOTA_FLAG\")] flag: bool }",
            "DBOTA_FLAG",
        ),
        (
            "a closure",
            "fn f() { let env = |n| std::env::var_os(n); env(\"HOME\"); }",
            "HOME",
        ),
        (
            "a helper with another name",
            "fn f() { let set = |n| std::env::var_os(n).is_some(); set(\"XDG_DATA_HOME\"); }",
            "XDG_DATA_HOME",
        ),
        ("after a test module", late_in_file, "BOTA_LATE"),
        (
            "a raw string and a lifetime before it",
            "fn f<'a>(x: &'a str) { let c = '\"'; let q = r#\"he said \"hi\"\"#; std::env::var(\"LOCALAPPDATA\"); }",
            "LOCALAPPDATA",
        ),
    ];
    for (what, source, name) in cases {
        let problems = environment_problems("src/elsewhere.rs", source);
        assert!(
            problems.iter().any(|problem| problem.contains(name)),
            "the audit missed {what}: {problems:?}"
        );
    }
    for source in [
        "// std::env::var(\"BOTA_COMMENTED\")",
        "/* \"BOTA_BLOCK\" /* nested */ \"HOME\" */ fn f() {}",
        "fn f() { command.env(\"HOME\", \"x\").env_remove(\"BOTA_SERVER\"); }",
        "fn f() { std::env::var(\"DBOTA_ASSETS\"); std::env::var(\"DBOTA_ANYTHING_CHILD\"); }",
        "fn f() { std::env::var(\"PATH\"); }",
    ] {
        assert_eq!(
            environment_problems("src/elsewhere.rs", source),
            Vec::<String>::new(),
            "{source}"
        );
    }
    assert!(
        environment_problems("src/config.rs", "fn f() { std::env::var_os(\"HOME\"); }").is_empty(),
        "a resolver may read a location"
    );
}
