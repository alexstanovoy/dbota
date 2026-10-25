//! The build's commit, for the version line the settings show (`DBOTA_BUILD`): the checkout's
//! short commit hash, `+` when its files differ from it; "unknown" outside a git checkout.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    // A new commit or a switched branch moves HEAD or the branch it names; the index changes
    // with every staged edit, which is what the `+` follows.
    for path in ["HEAD", "index"] {
        if let Some(file) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={file}");
        }
    }
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(file) = git(&["rev-parse", "--git-path", &branch])
    {
        println!("cargo:rerun-if-changed={file}");
    }
    let build = match git(&["rev-parse", "--short=12", "HEAD"]) {
        Some(hash) => {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"])
                .is_some_and(|s| !s.is_empty());
            format!("{hash}{}", if dirty { "+" } else { "" })
        }
        None => "unknown".to_owned(),
    };
    println!("cargo:rustc-env=DBOTA_BUILD={build}");
}
