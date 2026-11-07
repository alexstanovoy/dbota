//! Asking a game server binary for its wire, without trusting it to answer.
//!
//! Std only: the integration tests include this file as their own module too.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

/// How long a server gets to print its wire. A hang guard, not a measure: a healthy binary
/// answers at once, under any load.
pub const WIRE_PROBE_PATIENCE: Duration = Duration::from_secs(10);

/// Most bytes of its answer a server is read for: a wire is sixteen hex digits and a newline.
const MAX_ANSWER_BYTES: usize = 64;

/// How much of a wrong answer an error quotes.
const QUOTED_BYTES: usize = 32;

/// The wire the server `name` speaks, from its own `--wire-fingerprint`. The caller gives the
/// command that starts it, so that the server is launched through the isolation wrappers
/// (scrubbed environment) like every other one. A server that has not answered within `patience`
/// is killed with everything it started (it runs in a process group of its own), whatever it
/// answered, and at most `MAX_ANSWER_BYTES` of its output are read.
pub fn wire_of(mut server: Command, name: &Path, patience: Duration) -> Result<u64, String> {
    let name = name.display();
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut server, 0);
    server
        .arg("--wire-fingerprint")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let deadline = Instant::now() + patience;
    let mut child = loop {
        match server.spawn() {
            Ok(child) => break child,
            // A binary just written is busy until every process forked while it was open has
            // moved on: transient by nature.
            Err(error)
                if error.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < deadline =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(format!("{name} does not start: {error}")),
        }
    };
    let mut stdout = child.stdout.take().expect("the output was piped");
    let (answer_sender, answer) = mpsc::channel();
    // Not joined: a read that outlives a failed kill must not hold the caller; it ends with the pipe.
    let reader = std::thread::Builder::new()
        .name("wire-probe".to_owned())
        .spawn(move || {
            let _ = answer_sender.send(read_answer(&mut stdout));
        });
    let heard = match reader {
        Ok(_) => answer.recv_timeout(deadline.saturating_duration_since(Instant::now())),
        Err(_) => Err(RecvTimeoutError::Disconnected),
    };
    stop(&mut child);
    let said = match heard {
        Ok(Ok(said)) => said,
        Ok(Err(error)) => return Err(format!("{name} said something unreadable: {error}")),
        Err(RecvTimeoutError::Timeout) => {
            return Err(format!(
                "{name} did not answer its wire probe within {}s and was killed",
                patience.as_secs()
            ));
        }
        Err(RecvTimeoutError::Disconnected) => {
            return Err(format!("{name} could not be listened to"));
        }
    };
    let line = said.split(|byte| *byte == b'\n').next().unwrap_or_default();
    if !said.contains(&b'\n') && said.len() >= MAX_ANSWER_BYTES {
        return Err(format!(
            "{name} did not say its wire in its first {MAX_ANSWER_BYTES} bytes: {}",
            quoted(line)
        ));
    }
    std::str::from_utf8(line)
        .ok()
        .and_then(|line| u64::from_str_radix(line.trim(), 16).ok())
        .ok_or_else(|| format!("{name} did not say its wire: {}", quoted(line)))
}

/// Up to `MAX_ANSWER_BYTES` of what `output` says, ending at the first newline.
fn read_answer(output: &mut impl Read) -> std::io::Result<Vec<u8>> {
    let mut said = Vec::with_capacity(MAX_ANSWER_BYTES);
    let mut chunk = [0_u8; MAX_ANSWER_BYTES];
    while said.len() < MAX_ANSWER_BYTES && !said.contains(&b'\n') {
        let read = output.read(&mut chunk[..MAX_ANSWER_BYTES - said.len()])?;
        if read == 0 {
            break;
        }
        said.extend_from_slice(&chunk[..read]);
    }
    Ok(said)
}

/// The start of `bytes`, as text that is safe to put in an error.
fn quoted(bytes: &[u8]) -> String {
    format!(
        "{:?}",
        String::from_utf8_lossy(&bytes[..bytes.len().min(QUOTED_BYTES)])
    )
}

/// Kills the server's whole process group and reaps the server.
fn stop(child: &mut Child) {
    #[cfg(unix)]
    {
        unsafe extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        const SIGKILL: i32 = 9;
        let group = i32::try_from(child.id()).expect("a process id fits the platform's pid_t");
        assert!(group > 1, "a child's group is never init's or the caller's");
        // SAFETY: kill(2) only sends a signal. The group is the child's own (`process_group(0)`
        // made it the leader) and the unreaped leader keeps the id from being reused.
        unsafe { kill(-group, SIGKILL) };
    }
    let _ = child.kill();
    let _ = child.wait();
}
