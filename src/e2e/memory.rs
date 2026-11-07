//! What a test process holds once its clients have ended: what a client loaded goes with it,
//! and no client draws on the GPU. Measured in a process of its own, as the other tests of this
//! one share its heap.

use std::process::Command;

use bota_proto::HeroId;

use super::harness::{Client, Server, ServerEnd};

/// Clients of the measured run, one after another.
const CLIENTS: usize = 4;
/// Heap in use the clients after the first may leave behind, all of them: one client loads about
/// a gigabyte, and one that kept its asset server kept all of it.
const IN_USE_GROWTH_MIB: usize = 64;
/// Resident memory the clients after the first may leave behind, all of them.
const RESIDENT_GROWTH_MIB: usize = 256;

/// Clients started one after another leave the heap as the first left it, and none opens a GPU.
#[test]
fn clients_one_after_another_leave_no_memory_behind() {
    let output = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "--exact",
            "e2e::memory::measured_alone",
            "--ignored",
            "--test-threads",
            "1",
        ])
        .output()
        .expect("the test binary runs");
    let told = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.status.success(), "the measured run failed:\n{told}");
    assert!(
        told.contains("test result: ok. 1 passed"),
        "the measured run ran its test:\n{told}"
    );
}

#[test]
#[ignore = "measures its whole process: run alone by clients_one_after_another_leave_no_memory_behind"]
fn measured_alone() {
    let mut after = Vec::new();
    for _ in 0..CLIENTS {
        let server = Server::start(1, &[]);
        let client = Client::join(&server, HeroId::SHADOW_FIEND, &[]);
        drop(client);
        drop(server);
        after.push((in_use_mib(), resident_mib()));
    }
    // From the first client on: it leaves what a process makes once (thread pools, their stacks).
    let (first_in_use, first_resident) = after[0];
    let (last_in_use, last_resident) = after[CLIENTS - 1];
    assert!(
        last_in_use <= first_in_use + IN_USE_GROWTH_MIB,
        "heap in use after each client grows: {after:?} (MiB in use, MiB resident)"
    );
    assert!(
        last_resident <= first_resident + RESIDENT_GROWTH_MIB,
        "resident memory after each client grows: {after:?} (MiB in use, MiB resident)"
    );
    let maps = std::fs::read_to_string("/proc/self/maps").expect("the process's mappings");
    let drivers: Vec<&str> = maps
        .lines()
        .filter(|line| {
            ["libvulkan", "libnvidia", "libEGL", "libGL"]
                .iter()
                .any(|driver| line.contains(driver))
        })
        .collect();
    assert!(
        drivers.is_empty(),
        "a test client opened a GPU: {drivers:?}"
    );
}

/// A loaded asset the client's asset server still holds when the client ends is gone before the
/// client's end is told: the server goes on an IO thread when a load there held it last, and what
/// the client loaded is measured after the client ends.
#[test]
fn a_client_ends_after_the_assets_its_server_still_held() {
    use bevy::asset::{AssetApp, AssetServer};

    #[derive(bevy::asset::Asset, bevy::reflect::TypePath)]
    struct Held {
        end: std::sync::Weak<ServerEnd>,
        told: std::sync::mpsc::Sender<bool>,
    }

    impl Drop for Held {
        fn drop(&mut self) {
            let _ = self.told.send(self.end.upgrade().is_some());
        }
    }

    let mut client = Client::run(&[]);
    client.app.init_asset::<Held>();
    let (told, heard) = std::sync::mpsc::channel();
    let end = client.server_end();
    // Added for the app's next frame, which never comes: the server holds it until it ends.
    let _held = client
        .app
        .world()
        .resource::<AssetServer>()
        .add(Held { end, told });
    drop(client);
    assert_eq!(
        heard.try_recv(),
        Ok(true),
        "the asset the server held was destroyed (true) before the client's end was told"
    );
}

/// The heap's bytes in use, every arena's and the large blocks mapped apart.
fn in_use_mib() -> usize {
    /// glibc's `struct mallinfo2`.
    #[repr(C)]
    struct MallInfo2 {
        arena: usize,
        ordblks: usize,
        smblks: usize,
        hblks: usize,
        hblkhd: usize,
        usmblks: usize,
        fsmblks: usize,
        uordblks: usize,
        fordblks: usize,
        keepcost: usize,
    }
    unsafe extern "C" {
        fn mallinfo2() -> MallInfo2;
    }
    // SAFETY: mallinfo2 takes nothing and returns a plain struct of counters.
    let info = unsafe { mallinfo2() };
    (info.uordblks + info.hblkhd) >> 20
}

/// The process's resident memory.
fn resident_mib() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("the process's status");
    let kib: usize = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse().ok())
        .expect("VmRSS in kB");
    kib >> 10
}
