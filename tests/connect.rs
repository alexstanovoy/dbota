//! What the client says and refuses before it opens a window: a notice for
//! plain text to another machine, and no seat key but the environment's.

use std::process::{Command, Output};

fn dbota(args: &[&str], key: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dbota"));
    command.args(args).env_remove("BOTA_SEAT_KEY");
    if let Some(key) = key {
        command.env("BOTA_SEAT_KEY", key);
    }
    command.output().expect("dbota runs")
}

fn noticed(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stderr).contains("not safe across the internet")
}

#[test]
fn plain_text_to_another_machine_is_noticed_not_refused() {
    // 0.0.0.0 is not loopback, and dialing it fails at once: nobody listens on port 1.
    for addr in ["0.0.0.0:1", "tcp://0.0.0.0:1", "ws://0.0.0.0:1"] {
        let output = dbota(&["--addr", addr], None);
        assert!(noticed(&output), "{addr}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("refused"),
            "{addr} is dialed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    for addr in [
        "127.0.0.1:1",
        "localhost:1",
        "ws://[::1]:1",
        "wss://127.0.0.1:1",
    ] {
        assert!(!noticed(&dbota(&["--addr", addr], None)), "{addr}");
    }
}

#[test]
fn the_seat_key_comes_from_the_environment_only() {
    let key = "11".repeat(32);
    let argument = dbota(&["--addr", "127.0.0.1:1", "--seat-key", &key], None);
    assert!(
        String::from_utf8_lossy(&argument.stderr).contains("unexpected argument"),
        "{}",
        String::from_utf8_lossy(&argument.stderr)
    );
    let bad = dbota(&["--addr", "127.0.0.1:1"], Some("nothex"));
    assert!(
        String::from_utf8_lossy(&bad.stderr).contains("BOTA_SEAT_KEY"),
        "{}",
        String::from_utf8_lossy(&bad.stderr)
    );
}
