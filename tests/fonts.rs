//! The client starts on a machine whose fontconfig lists no font at all: it
//! says so once and goes on with the shipped fonts instead of panicking.

use std::process::Command;

#[test]
fn a_system_without_fonts_is_not_a_panic() {
    let config = std::env::temp_dir().join(format!("dbota-no-fonts-{}.conf", std::process::id()));
    std::fs::write(
        &config,
        "<?xml version=\"1.0\"?>\n<fontconfig></fontconfig>\n",
    )
    .expect("config");
    // The connection to port 1 of 0.0.0.0 fails at once, after the fonts are read.
    let output = Command::new(env!("CARGO_BIN_EXE_dbota"))
        .args(["--addr", "0.0.0.0:1"])
        .env("FONTCONFIG_FILE", &config)
        .env_remove("BOTA_SEAT_KEY")
        .output()
        .expect("dbota runs");
    let _ = std::fs::remove_file(&config);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"), "{stderr}");
    assert_eq!(
        stderr.matches("using the shipped fonts only").count(),
        1,
        "{stderr}"
    );
    assert!(
        stderr.contains("refused"),
        "the run goes on to dial: {stderr}"
    );
}
