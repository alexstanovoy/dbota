//! Whatever starts, drives or measures the client stays behind the isolation wrappers: it never
//! reads or writes the user's HOME, XDG folders, replays (the library prunes the oldest beyond
//! its limits) or managed asset cache, and never inherits a variable that steers the client.
mod common;

use common::source_scan::{environment_violations, launch_lines, names_the_managed_cache};
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

/// A cache in the shape of the managed one (`~/.cache/dbota/<fingerprint>`) under `scratch`, with an index.
#[cfg(unix)]
fn managed_looking_cache(scratch: &common::prepared::Scratch) -> PathBuf {
    let cache = scratch.path("home/.cache/dbota/v9");
    std::fs::create_dir_all(&cache).expect("cache directory");
    std::fs::write(cache.join("index.txt"), "").expect("cache index");
    cache
}

#[cfg(unix)]
#[test]
fn shell_test_client_refuses_the_managed_asset_cache() {
    let scratch = common::prepared::Scratch::new("shell-managed-cache");
    let tools = copy_tools(&scratch, &["test_client.sh"]);
    let output = std::process::Command::new("bash")
        .arg(tools.join("test_client.sh"))
        .env("DBOTA_ASSETS", managed_looking_cache(&scratch))
        .output()
        .expect("wrapper starts");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("DBOTA_ASSETS names the managed asset cache"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn bench_refuses_the_managed_asset_cache() {
    let scratch = common::prepared::Scratch::new("bench-managed-cache");
    let output = bench(&scratch, Some(&managed_looking_cache(&scratch)));
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("DBOTA_ASSETS names the managed asset cache"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!scratch.path("stub.sh.env").exists(), "a client ran");
}

#[test]
fn the_harness_refuses_the_managed_asset_cache() {
    if std::env::var_os("DBOTA_MANAGED_CACHE_CHILD").is_some() {
        common::asset_cache();
        return;
    }
    let scratch = common::prepared::Scratch::new("harness-managed-cache");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "the_harness_refuses_the_managed_asset_cache",
            "--nocapture",
        ])
        .env("DBOTA_MANAGED_CACHE_CHILD", "1")
        .env("DBOTA_ASSETS", scratch.path("home/.cache/dbota/v9"))
        .output()
        .expect("child test starts");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("DBOTA_ASSETS names the managed asset cache"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The environment `in_private_environment` makes, printed by a shell command, one value a line.
#[cfg(unix)]
const PRIVATE_ENVIRONMENT_PROBE: &str = "printf \"%s\\n\" \"$HOME\" \"$CARGO_HOME\" \"$RUSTUP_HOME\" \"$SCCACHE_DIR\" \"$DBOTA_DOTA_PATH\" \"$XDG_CACHE_HOME\" \"${BOTA_SEATS-unset}\"";

#[cfg(unix)]
#[test]
fn a_private_environment_hides_the_home_and_keeps_the_toolchain_and_the_dota_install() {
    let scratch = common::prepared::Scratch::new("private-environment");
    let tools = copy_tools(&scratch, &[]);
    let home = scratch.path("home");
    let install = home.join(".local/share/Steam/steamapps/common/dota 2 beta");
    std::fs::create_dir_all(install.join("game/dota")).expect("install");
    std::fs::write(install.join("game/dota/pak01_dir.vpk"), "").expect("package");
    let run = |script: &str| {
        std::process::Command::new("bash")
            .args([
                "-c",
                &format!(". '{}'; {script}", tools.join("client_env.sh").display()),
            ])
            .env("HOME", &home)
            .env("CARGO_TARGET_DIR", scratch.path("target"))
            .env("BOTA_SEATS", "untrusted")
            .env_remove("CARGO_HOME")
            .env_remove("RUSTUP_HOME")
            .env_remove("SCCACHE_DIR")
            .env_remove("XDG_CACHE_HOME")
            .env_remove("DBOTA_DOTA_PATH")
            .output()
            .expect("bash starts")
    };
    let output = run(&format!(
        "in_private_environment sh -c '{PRIVATE_ENVIRONMENT_PROBE}'"
    ));
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    let values: Vec<_> = text.lines().collect();
    assert_eq!(values.len(), 7, "{values:?}");
    assert!(
        Path::new(values[0]).starts_with(scratch.path("target")),
        "{values:?}"
    );
    assert_eq!(Path::new(values[1]), home.join(".cargo"));
    assert_eq!(Path::new(values[2]), home.join(".rustup"));
    assert_eq!(Path::new(values[3]), home.join(".cache/sccache"));
    assert_eq!(Path::new(values[4]), install);
    assert!(
        Path::new(values[5]).starts_with(scratch.path("target")),
        "{values:?}"
    );
    assert_eq!(
        values[6], "untrusted",
        "a steering variable is the harness's to scrub"
    );
    assert!(!Path::new(values[0]).exists(), "the private home remains");
    let failing = run("in_private_environment sh -c 'exit 3'");
    assert_eq!(
        failing.status.code(),
        Some(3),
        "the command's status is the wrapper's"
    );
}

#[cfg(unix)]
#[test]
fn headless_runs_its_command_on_a_private_home() {
    let scratch = common::prepared::Scratch::new("headless-private");
    let tools = copy_tools(&scratch, &["headless.sh"]);
    // A compositor that only runs what comes after `--`, as weston does.
    let weston = scratch.path("weston");
    executable(
        &weston,
        "#!/bin/sh\nwhile [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n",
    );
    let output = std::process::Command::new("bash")
        .arg(tools.join("headless.sh"))
        .args(["sh", "-c", PRIVATE_ENVIRONMENT_PROBE])
        .env("DBOTA_WESTON", &weston)
        .env("HOME", scratch.path("home"))
        .env("CARGO_TARGET_DIR", scratch.path("target"))
        .env("BOTA_SEATS", "untrusted")
        .env_remove("DBOTA_HEADLESS")
        .env_remove("DBOTA_SHOW")
        .output()
        .expect("headless starts");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).expect("UTF-8");
    let values: Vec<_> = text.lines().collect();
    assert_eq!(values.len(), 7, "{values:?}");
    assert!(
        Path::new(values[0]).starts_with(scratch.path("target")),
        "{values:?}"
    );
    assert_eq!(values[6], "untrusted");
}

#[test]
fn precheck_and_gate_run_the_tests_on_a_private_home() {
    let read =
        |name: &str| std::fs::read_to_string(manifest().join("tools").join(name)).expect("tool");
    let precheck = read("precheck.sh");
    assert!(precheck.contains("step test in_private_environment run_tests"));
    assert!(precheck.contains("step \"test ($c)\" in_private_environment bash -c \"$e\""));
    let gate = read("gate.sh");
    for step in [
        "step test in_private_environment cargo test --profile test-release",
        "step install-tests in_private_environment cargo test --release -p dbota_import -- --ignored",
        "step tip-content in_private_environment cargo test --release -p dbota tip_content -- --ignored",
    ] {
        assert!(gate.contains(step), "tools/gate.sh does not run `{step}`");
    }
}

/// Starts the stub server `common::serve` finds through BOTA_SERVER_BIN and returns what the
/// stub saw of the variables that steer a server.
#[test]
fn a_spawned_game_server_does_not_inherit_the_variables_that_steer_it() {
    const MARK: &str = "DBOTA_SERVER_SCRUB_CHILD";
    if let Some(seen) = std::env::var_os(MARK) {
        let _server = common::serve(&[]);
        let seen = std::fs::read_to_string(seen).expect("the stub ran");
        let lines: Vec<_> = seen.lines().collect();
        assert_eq!(
            lines.len(),
            common::client_environment().count(),
            "{lines:?}"
        );
        assert!(
            lines.iter().all(|line| *line == "unset"),
            "a server inherited {lines:?}"
        );
        return;
    }
    let scratch = common::prepared::Scratch::new("server-scrub");
    let seen = scratch.path("seen.txt");
    let stub = scratch.path("bota_server");
    let probe: String = common::client_environment()
        .map(|name| format!(" \"${{{name}-unset}}\""))
        .collect();
    executable(
        &stub,
        &format!(
            "#!/bin/sh\nprintf '%s\\n'{probe} > '{}'\necho 'bota_server listening on 127.0.0.1:1'\nsleep 5\n",
            seen.display()
        ),
    );
    let mut parent = std::process::Command::new(std::env::current_exe().expect("test binary"));
    parent
        .args([
            "--exact",
            "a_spawned_game_server_does_not_inherit_the_variables_that_steer_it",
            "--nocapture",
        ])
        .env(MARK, &seen)
        .env("BOTA_SERVER_BIN", &stub);
    untrusted_environment(&mut parent);
    let output = parent.output().expect("child test starts");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn the_isolated_client_refuses_to_be_given_the_variables_it_isolates() {
    let mut client = common::binary(Path::new("unused"));
    for name in [
        "HOME",
        "XDG_DATA_HOME",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "BOTA_SERVER",
        "BOTA_SEATS",
        "DBOTA_SERVICE",
    ] {
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client.env(name, "/untrusted");
        }));
        let panic = refused.expect_err(name);
        let message = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
            .unwrap_or_default();
        assert!(message.contains("isolated client"), "{name}: {message}");
    }
}

/// Every Rust, shell and Python file of the repository, by path relative to it: all of them, so
/// that a new folder is not a hole. Build output and data folders hold no source.
fn repository_sources() -> Vec<(String, String)> {
    let root = manifest();
    let mut directories = vec![root.clone()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(&directory).expect("source directory") {
            let entry = entry.expect("source entry");
            let path = entry.path();
            if entry.file_type().expect("entry type").is_dir() {
                if !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some(
                        "target"
                            | ".git"
                            | "node_modules"
                            | "pkg"
                            | "dist"
                            | "out"
                            | "temp"
                            | "assets"
                            | "assets-manifest"
                    )
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

/// The exact lines the guard finds that start the client or a game server as it is, with why each
/// is no problem: it is the isolating wrapper itself, or only a name or a message to the guard's
/// squeezed reading, or it needs the user's real home and writes only an explicit folder.
const APPROVED_LAUNCH_LINES: &[(&str, &str, &str)] = &[
    (
        "tests/common/mod.rs",
        "prefix.arg(env!(\"CARGO_BIN_EXE_dbota\"));",
        "the isolated client: private HOME and XDG folders, scrubbed variables",
    ),
    (
        "tests/common/mod.rs",
        "command.arg(env!(\"CARGO_BIN_EXE_dbota\"));",
        "the isolated client: private HOME and XDG folders, scrubbed variables",
    ),
    (
        "tests/common/mod.rs",
        "None => Command::new(env!(\"CARGO_BIN_EXE_dbota\")),",
        "the isolated client: private HOME and XDG folders, scrubbed variables",
    ),
    (
        "tools/client_env.sh",
        "echo \"${DBOTA_BIN:-${CARGO_TARGET_DIR:-$_client_env_here/target}/${1:-release}/dbota}\"",
        "client_binary names the binary the isolating tools run",
    ),
    (
        "tools/precheck.sh",
        "(flock 9 && cargo run --release -q -- import) 9> \"$DBOTA_ASSETS.lock\" > /dev/null 2>&1 || echo \"   no asset cache (no usable install): the tests that need it fail\"",
        "`import` reads the user's Dota install and writes only the explicit DBOTA_ASSETS",
    ),
    (
        "tools/gate.sh",
        "if (flock 9 && cargo run --release -q -- import) 9> \"$DBOTA_ASSETS.lock\" > \"$logs/import.log\" 2>&1; then",
        "`import`, as above",
    ),
    (
        "tools/gate.sh",
        "if ! cargo run --release -q --manifest-path \"$here/../bota/Cargo.toml\" -p bota_server --example local_match \\",
        "bota's local_match example, not the client",
    ),
    (
        "tools/build.sh",
        "echo \"built $here/target/$profile/dbota and $here/target/$profile/bota_server\"",
        "an echo of the paths it built",
    ),
    (
        "web/build.sh",
        "\"target/wasm32-unknown-unknown/$profile/dbota.wasm\"",
        "dbota.wasm, not the native client",
    ),
    (
        "web/build.sh",
        "wasm-bindgen --target web --no-typescript --out-dir \"$out/import\" \"target/wasm32-unknown-unknown/$profile/dbota_import.wasm\"",
        "dbota_import.wasm, not the native client",
    ),
];

#[test]
fn client_and_server_launches_use_the_isolation_wrappers() {
    for (path, text) in repository_sources() {
        if path == "tests/client_guard.rs" || path == "tests/common/source_scan.rs" {
            continue; // they list the forms they look for
        }
        let found = launch_lines(&text);
        let approved: Vec<String> = APPROVED_LAUNCH_LINES
            .iter()
            .filter(|(file, ..)| *file == path)
            .map(|(_, line, _)| (*line).to_owned())
            .collect();
        for line in &found {
            assert!(
                approved.contains(line),
                "{path} starts the client or a game server outside the isolation wrappers: `{line}`"
            );
        }
        for line in &approved {
            assert!(
                found.contains(line),
                "{path}: the approved line `{line}` is gone (APPROVED_LAUNCH_LINES is stale)"
            );
        }
        // client_env.sh spells the managed cache to refuse it.
        if !path.starts_with("src/") && path != "tools/client_env.sh" {
            assert!(
                !names_the_managed_cache(&text),
                "{path} names the user's managed asset cache ~/.cache/dbota: take an explicit DBOTA_ASSETS"
            );
        }
    }
}

#[test]
fn the_launch_guard_sees_every_way_to_start_the_client_or_a_server() {
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
        "cargo   run".to_owned(),
        "cargo +nightly run".to_owned(),
        "\"$CARGO\" -q run".to_owned(),
        "${CARGO} run -p dbota".to_owned(),
        "Command::new(\"cargo\").arg(\"run\")".to_owned(),
        "Command::new(&cargo).args([\"run\", \"--release\"])".to_owned(),
        "Command::new(\"dbota\")".to_owned(),
        "let child = Command::new(server_binary())".to_owned(),
        "Command::new(common::server_binary())".to_owned(),
    ] {
        assert!(
            !launch_lines(&source).is_empty(),
            "the guard missed {source}"
        );
    }
    for source in [
        "// target/release/dbota",
        "# cargo run",
        "\"$here/tools/test_client.sh\" --replay x",
        "target/wasm32-unknown-unknown/web/dbota.wasm",
        "cargo build --release",
        "cargo test --release -p dbota_import -- --ignored",
        "cargo build -- run",
        "Command::new(&cargo).args([\"build\"])",
        "let command = common::server_command(server_binary());",
    ] {
        assert_eq!(
            launch_lines(source),
            Vec::<String>::new(),
            "the guard flags {source}"
        );
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
        "DBOTA_REFERENCE",
        "the importer's golden tests name their reference cache",
    ),
    (
        "DBOTA_IMPORTED",
        "the importer's tests name an imported cache to read",
    ),
    (
        "DBOTA_MODEL_CACHE",
        "the importer's tests name a copy of a cache to work in",
    ),
    (
        "DBOTA_WORLD_CACHE",
        "the importer's tests name a copy of a cache to work in",
    ),
];

/// Files that resolve a default location from the environment, and so have to be given explicit
/// paths by every harness (the in-process one passes them on the command line).
const LOCATION_RESOLVERS: &[&str] = &[
    "tests/common/mod.rs", // lists the folders its isolated client keeps to itself
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
        if !path.ends_with(".rs")
            || path == "tests/client_guard.rs"
            || path == "tests/common/source_scan.rs"
        {
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
    for expected in [
        "BOTA_SERVER",
        "DBOTA_SERVICE",
        "BOTA_SEAT_KEY",
        "DBOTA_DRILL",
        "DBOTA_DRILL_SHOTS",
    ] {
        assert!(
            discovered.contains(expected),
            "the audit never met {expected}, which the client reads: it is blind"
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
            "an escape",
            "fn f() { std::env::var(\"\\x42OTA_ESCAPED\"); }",
            "BOTA_ESCAPED",
        ),
        (
            "a unicode escape",
            "fn f() { std::env::var(\"\\u{44}BOTA_ESCAPED_TOO\"); }",
            "DBOTA_ESCAPED_TOO",
        ),
        (
            "the whole environment",
            "fn f() { for (k, v) in std::env::vars_os() {} }",
            "enumerates the whole environment",
        ),
        (
            "the whole environment, as text",
            "fn f() { std::env::vars().count(); }",
            "enumerates the whole environment",
        ),
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
