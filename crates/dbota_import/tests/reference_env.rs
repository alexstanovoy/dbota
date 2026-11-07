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

/// Converter tests still find the explicit install when their HOME is private.
#[test]
fn the_explicit_install_is_found_on_a_private_home() {
    if std::env::var_os("DBOTA_REFERENCE_TEST_CHILD").is_some() {
        let install =
            std::path::PathBuf::from(std::env::var_os("DBOTA_DOTA_PATH").expect("install"));
        assert_eq!(common::game_dir(), install.join("game/dota"));
        return;
    }
    let root = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("explicit-install-{}", std::process::id()));
    let install = root.join("dota");
    std::fs::create_dir_all(install.join("game/dota")).expect("install directory");
    std::fs::write(install.join("game/dota/pak01_dir.vpk"), []).expect("install marker");
    let output = std::process::Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "the_explicit_install_is_found_on_a_private_home",
            "--nocapture",
        ])
        .env("DBOTA_REFERENCE_TEST_CHILD", "1")
        .env("DBOTA_DOTA_PATH", &install)
        .env("HOME", root.join("private-home"))
        .output()
        .expect("child test starts");
    std::fs::remove_dir_all(&root).expect("remove test directory");
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
