//! Golden tests must name their reference rather than reading a developer's cache.
mod common;

#[test]
fn missing_reference_is_reported_without_using_the_home_directory() {
    if std::env::var_os("DBOTA_REFERENCE_TEST_CHILD").is_some() {
        common::reference();
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "missing_reference_is_reported_without_using_the_home_directory",
            "--nocapture",
        ])
        .env("DBOTA_REFERENCE_TEST_CHILD", "1")
        .env_remove("DBOTA_REFERENCE")
        .env("HOME", "/nonexistent")
        .output()
        .expect("child test starts");
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("DBOTA_REFERENCE must name the reference cache")
    );
}
