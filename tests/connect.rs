//! What the client says and refuses as it starts: a notice for
//! plain text to another machine, and no seat key but the environment's.
//!
//! The client is started in the headless compositor (tools/headless.sh): it opens a window on the way.

mod common;

use std::process::Output;

fn dbota(args: &[&str], key: Option<&str>) -> Output {
    let mut command = common::client();
    command.args(args).env_remove("BOTA_SEAT_KEY");
    if let Some(key) = key {
        command.seat_key(key);
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

/// The match tests stop at once, naming both wires, when their server is of another wire than the
/// client (bota moved after the client was compiled).
#[cfg(unix)]
#[test]
fn a_server_of_another_wire_is_refused_before_any_match() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = common::prepared::Scratch::new("stale-server");
    let stale = scratch.path("bota_server");
    std::fs::write(&stale, "#!/bin/sh\necho 0000000000000001\n").expect("a script");
    std::fs::set_permissions(&stale, std::fs::Permissions::from_mode(0o755)).expect("mode");
    let said = common::check_wire(&stale, 0xabcd).expect_err("another wire");
    assert_eq!(
        said,
        format!(
            "{} speaks wire 0000000000000001 but this client was compiled for wire \
             000000000000abcd: rebuild the client against the current bota, or check out the \
             bota it was compiled with",
            stale.display()
        )
    );
}
