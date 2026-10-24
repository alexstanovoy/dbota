//! What the tests that draw share.

use std::process::Command;

/// The client, to be given its arguments, drawing in a headless compositor of its own on the GPU
/// (tools/headless.sh, or the one the run is already in): no window shows on the desktop, unless
/// `DBOTA_SHOW=1` asks for it.
pub fn client() -> Command {
    let mut command = Command::new(concat!(env!("CARGO_MANIFEST_DIR"), "/tools/headless.sh"));
    command.arg(env!("CARGO_BIN_EXE_dbota"));
    command
}
