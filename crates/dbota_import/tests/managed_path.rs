//! Which paths are the user's managed asset cache, which tests and tools must never name.
use dbota_import::cache_dir::inside_managed_cache;
use std::path::PathBuf;

fn scratch(what: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("managed-{what}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    dir
}

#[test]
fn the_managed_root_and_what_is_inside_it_are_managed_whether_or_not_they_exist() {
    for path in [
        "/home/a/.cache/dbota",
        "/home/a/.cache/dbota/v9",
        "rel/.cache/dbota/v9/x",
    ] {
        assert!(inside_managed_cache(path.as_ref()), "{path}");
    }
}

#[test]
fn dots_do_not_hide_the_managed_cache_even_below_a_folder_that_does_not_exist() {
    for path in [
        "/home/a/.cache/nonexistent/../dbota/v9",
        "/home/a/./.cache/./dbota",
        "/home/a/x/../.cache/dbota/v9/y",
        "/home/a/.cache/dbota/v9/../../elsewhere/../dbota",
    ] {
        assert!(inside_managed_cache(path.as_ref()), "{path}");
    }
    for path in [
        "/home/a/.cache/dbota/../other",
        "/home/a/.cache/x/../dbota-gate",
    ] {
        assert!(!inside_managed_cache(path.as_ref()), "{path}");
    }
}

#[test]
fn the_gates_and_other_caches_are_not_managed() {
    for path in [
        "/home/a/.cache/dbota-gate/k",
        "/home/a/.cache/other/dbota",
        "/home/a/dbota",
        "relative",
        "",
    ] {
        assert!(!inside_managed_cache(path.as_ref()), "{path}");
    }
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_into_the_managed_cache_is_managed() {
    let dir = scratch("link");
    let managed = dir.join("home/.cache/dbota/v9");
    std::fs::create_dir_all(&managed).expect("managed folder");
    let link = dir.join("innocent");
    std::os::unix::fs::symlink(&managed, &link).expect("link");
    assert!(inside_managed_cache(&link));
    assert!(
        inside_managed_cache(&link.join("not-yet/made")),
        "a path below the link that does not exist yet"
    );
    let outside = dir.join("elsewhere");
    std::fs::create_dir_all(&outside).expect("folder");
    assert!(!inside_managed_cache(&outside));
    let _ = std::fs::remove_dir_all(&dir);
}
