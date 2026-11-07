//! Golden tests must name their reference rather than reading a developer's cache, and the install tests their install.
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

#[test]
fn no_test_looks_the_install_up_under_the_callers_home() {
    let tests = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut looking_by_home = Vec::new();
    for entry in std::fs::read_dir(&tests).expect("the tests folder") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|extension| extension == "rs")
            && std::fs::read_to_string(&path)
                .expect("a test file")
                .contains(concat!("find_game_dir", "(None)"))
        {
            looking_by_home.push(path.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    assert!(
        looking_by_home.is_empty(),
        "{looking_by_home:?} look the install up under HOME, which the gate's private HOME hides: \
         use common::game_dir(), which honours DBOTA_DOTA_PATH"
    );
}
