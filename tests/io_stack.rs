//! The client loads a match's models on a machine with one core without overflowing a loader
//! thread's stack. Bevy's glTF loader waits for its textures by running the pool's other tasks
//! on the loading thread, so models being loaded nest on one stack, deeper the fewer loader
//! threads there are (one per four cores): on one core the nesting of a match's models needs
//! more than the 2 MiB a thread gets by default.
//!
//! It draws on the GPU (in a headless compositor, tools/headless.sh), so it is left out of a
//! plain `cargo test`; `tools/visual_check.sh` runs it with `VISUAL_REPLAY`.

mod common;

use std::path::PathBuf;
use std::process::Stdio;

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_match_loads_on_one_core() {
    let replay = std::env::var_os("VISUAL_REPLAY")
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .expect("VISUAL_REPLAY names a replay of the current wire");
    let shots =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("io-stack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&shots);
    std::fs::create_dir_all(&shots).expect("a scratch folder");
    // The client, and the compositor it draws in, on the first core this test may use.
    let mut pinned = std::process::Command::new("sh");
    pinned.args([
        "-c",
        r#"cpu=$(taskset -pc $$ | sed 's/.*: //' | cut -d, -f1 | cut -d- -f1); exec taskset -c "$cpu" "$@""#,
        "pinned",
    ]);
    let client = common::client();
    pinned.arg(client.get_program()).args(client.get_args());
    let output = pinned
        .args(["--replay"])
        .arg(&replay)
        .args(["--pov", "0", "--screenshot-at", "300", "--screenshot-dir"])
        .arg(&shots)
        .args(["--mute", "--no-vsync", "--display-mode", "windowed"])
        .stdout(Stdio::null())
        .output()
        .expect("dbota runs");
    let said = String::from_utf8_lossy(&output.stderr);
    let _ = std::fs::remove_dir_all(&shots);
    assert!(
        output.status.success() && !said.contains("overflowed its stack"),
        "exit {:?}:\n{}",
        output.status.code(),
        said.lines().rev().take(20).collect::<Vec<_>>().join("\n")
    );
}
