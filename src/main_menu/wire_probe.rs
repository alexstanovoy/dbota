//! Asking a game server binary for its wire, without trusting it to answer.
//!
//! Std only: the integration tests include this file as their own module too.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a server gets to print its wire. A hang guard, not a measure: a healthy binary
/// answers at once, under any load.
pub const WIRE_PROBE_PATIENCE: Duration = Duration::from_secs(10);

/// The wire the server `name` speaks, from its own `--wire-fingerprint`. The caller gives the
/// command that starts it, so that the server is launched through the isolation wrappers
/// (scrubbed environment) like every other one. A server that has not answered within `patience`
/// is killed.
pub fn wire_of(mut server: Command, name: &Path, patience: Duration) -> Result<u64, String> {
    let name = name.display();
    let mut child = server
        .arg("--wire-fingerprint")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("{name} does not start: {error}"))?;
    let deadline = Instant::now() + patience;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "{name} did not answer its wire probe within {}s and was killed",
                    patience.as_secs()
                ));
            }
            Err(error) => return Err(format!("{name} cannot be waited for: {error}")),
        }
    };
    let mut said = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        // The child has ended, so this reads what it left in the pipe and stops.
        std::io::Read::read_to_string(&mut stdout, &mut said)
            .map_err(|error| format!("{name} said something unreadable: {error}"))?;
    }
    u64::from_str_radix(said.trim(), 16)
        .map_err(|_| format!("{name} did not say its wire (exit {status}): {said:?}"))
}
