//! The picture checks of `tools/visual_check.sh`: ignored by default, as
//! they need a display, the Dota asset cache and bota next to dbota. Run
//! with `cargo test --release -- --ignored`.

#[test]
#[ignore]
fn the_picture_meets_the_owners_checks() {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/tools/visual_check.sh");
    let status = std::process::Command::new(script)
        .status()
        .expect("tools/visual_check.sh runs");
    assert!(status.success(), "a visual check failed; see its output");
}
