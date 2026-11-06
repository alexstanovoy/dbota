//! The variables that steer a client, or the game server it starts, from outside: the list in
//! `tools/client_environment.txt`, which the shell tools read too. This file is shared by path:
//! the client's own server launch, the in-process test harness and `tests/common` all take their
//! list and their scrubbed commands from here, so there is one list and one way to scrub.

use std::ffi::OsStr;
use std::process::Command;

/// The names, one per line of the list, comments and blank lines dropped.
pub fn variables() -> impl Iterator<Item = &'static str> {
    include_str!("../tools/client_environment.txt")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
}

/// A command that starts `program` without those variables: a seat list or referee key in a
/// developer's shell must not decide who sits in a game this process hosts or a test starts.
pub fn scrubbed_command(program: impl AsRef<OsStr>) -> Command {
    let mut command = Command::new(program);
    for name in variables() {
        command.env_remove(name);
    }
    command
}
