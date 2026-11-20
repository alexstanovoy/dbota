//! Full-map terrain rendered after the interface images have loaded.

use super::{
    ground_effect_pixels::{SIZE, pixels, prepare, save},
    harness::{Client, Server},
};
use bevy::{prelude::*, time::TimeUpdateStrategy};
use bota_proto::{Cheat, HeroId};
use std::time::Duration;

/// Use the real terrain material, camera and full-map assets, including the match's
/// usual interface preload.
#[test]
fn full_map_ground_draws_after_the_interface_art_loads() {
    let server = Server::start(1, &["--map", "2"]);
    let mut client = Client::run_rendered(
        &["--addr", &server.address, "--mute", "--distance", "3500"],
        |app| {
            prepare(app);
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                17,
            )));
        },
    );
    client.until("full-map ground ready for play", |client| {
        let core = client.core_mut();
        if core.view().is_none() && core.my_slot.is_some() && !core.ready {
            core.act(bota_client_core::Intent::PickHero(HeroId::SHADOW_FIEND.0));
            core.act(bota_client_core::Intent::ToggleReady);
        }
        core.view().is_some() && client.world().resource::<crate::loading::Loading>().done()
    });
    client.cheat(Cheat::AllVision { on: true });
    let probes = [
        ("radiant_base", 2950.0, 3390.0, 45, 42),
        ("mid_river_bank", 8850.0, 9350.0, 63, 31),
        ("bottom_lane", 12600.0, 2780.0, 45, 42),
        ("dire_base", 14700.0, 14500.0, 46, 22),
        ("jungle_camp", 7762.0, 5860.0, 45, 42),
    ];
    let mut samples = Vec::new();
    for (name, x, y, left, top) in probes {
        let world = client.world_mut();
        let ground = world.resource::<crate::map::Ground>().at(x, y);
        world
            .query::<&mut crate::camera::RtsCamera>()
            .single_mut(world)
            .expect("the game camera")
            .focus = Vec3::new(x, ground, -y);
        client.frames(3);
        let frame = pixels(&mut client);
        if std::env::var_os("OBJECTIVE_EVIDENCE_DIR").is_some() {
            save(name, &frame);
        }
        samples.push((name, ground_detail(&frame, left, top)));
    }
    assert!(
        samples.iter().all(|(_, detail)| *detail),
        "expected a patch of stone, earth or grass at each landmark: {samples:?}"
    );
    assert_eq!(
        crate::cachefs::image_budget_refusals(client.world()),
        0,
        "the imported art fits"
    );
    eprintln!("Full-map terrain coverage {samples:?}");
}

fn ground_detail(frame: &[u8], left: u32, top: u32) -> bool {
    let left = SIZE.x * left / 100;
    let top = SIZE.y * top / 100;
    let (width, height) = (SIZE.x / 10, SIZE.y * 16 / 100);
    let at = |x, y| ((y * SIZE.x + x) * 4) as usize;
    let colour = |x, y| {
        let i = at(x, y);
        [frame[i], frame[i + 1], frame[i + 2]]
    };
    let covered = (top..top + height)
        .flat_map(|y| (left..left + width).map(move |x| (x, y)))
        .filter(|&(x, y)| {
            let [r, g, b] = colour(x, y);
            g > 22 && r > 18 && u16::from(b) < u16::from(r) + 16
        })
        .count();
    if covered * 10 < (width * height) as usize * 7 {
        return false;
    }
    // Median tile microcontrast discounts props and wide gradients. A second
    // difference two pixels apart measures the grass and stone texture grain.
    let luma = |x, y| {
        let [r, g, b] = colour(x, y).map(i32::from);
        r + 2 * g + b
    };
    let mut tiles = Vec::new();
    for y in (top..top + height - 16).step_by(16) {
        for x in (left..left + width - 16).step_by(16) {
            let detail: u32 = (y..y + 16)
                .flat_map(|row| (x + 2..x + 14).map(move |column| (column, row)))
                .map(|(column, row)| {
                    (2 * luma(column, row) - luma(column - 2, row) - luma(column + 2, row))
                        .unsigned_abs()
                })
                .sum();
            tiles.push(detail);
        }
    }
    tiles.sort_unstable();
    tiles[tiles.len() / 2] > 16 * 12 * 5
}

#[test]
fn a_flat_colour_does_not_count_as_textured_terrain() {
    let flat = [87, 105, 63, 255].repeat((SIZE.x * SIZE.y) as usize);
    assert!(
        !ground_detail(&flat, 45, 42),
        "the flat terrain fallback has no detail"
    );
}
