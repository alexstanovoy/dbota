//! The miss notice as the screen shows it, end to end in the real client: a scripted seat's Shadow
//! Fiend stands in the mid river and attacks a hero up the Dire bank (a cheat match), a quarter of
//! its blows missing uphill; the client, the other seat, looks at the bank. Once the client draws
//! the word "miss", its frame holds a patch of the notice's red over the struck hero that the frame
//! before the hero came did not: the notice is legible on the GPU's own picture, not only laid
//! out.
//! Drawn on the GPU in a headless compositor, so it is left out of a plain `cargo test`;
//! `tools/visual_check.sh` runs it. `DBOTA_SHOTS` keeps the two frames for a person to look at.

mod common;

use std::path::{Path, PathBuf};

use bota_client_core::{ClientSession, IssuedOrder, Link};
use bota_proto::{
    Cheat, CreepSide, HeroId, ItemId, ItemSlot, ModifierSpec, Order, Target, Team, UnitKind, Vec2,
};

/// In the mid river (elevation tier 0) and up its Dire bank (tier 1), in a ranged hero's reach.
const RIVER: Vec2 = Vec2::from_ints(8750, 8880);
const BANK: Vec2 = Vec2::from_ints(9150, 9150);
/// Up the bank, in an Observer Ward's cast range from the river spot.
const WARD: Vec2 = Vec2::from_ints(9000, 9080);
/// The tick the scripted seat creates the hero it attacks on, the rest of the scene set long
/// before; the client shoots the frame before it a little earlier.
const ATTACK_TICK: u32 = 600;
const BEFORE_TICK: u32 = 450;
/// The window the client draws in, and the camera's distance from the bank.
const WINDOW: (u32, u32) = (1600, 900);
const DISTANCE: &str = "2500";

/// The scripted seat's cheats, one a tick: no creeps, its own hero unhurt by anything and its
/// blows a quarter as strong (so the struck hero outlives the scene), in the river with an
/// Observer Ward (the river does not see up the bank: a ward planted there lends that sight, as it
/// does in a match).
fn setup() -> Vec<Cheat> {
    vec![
        Cheat::CreepSpawning { on: false },
        Cheat::KillCreeps {
            which: CreepSide::All,
        },
        Cheat::Invulnerable { on: true },
        Cheat::ApplyModifier {
            target: Target::None,
            spec: ModifierSpec {
                physical_damage: ModifierSpec::MIN_SCALE,
                ..ModifierSpec::NOMINAL
            },
            ticks: bota_proto::MAX_MODIFIER_TICKS,
        },
        Cheat::Teleport { at: RIVER },
        Cheat::Item {
            item: ItemId::OBSERVER_WARD,
        },
    ]
}

/// The first seat, Radiant, held by a client with no front end: sets the scene and plants its
/// ward; on [`ATTACK_TICK`] creates an enemy hero on the bank and attacks it.
fn attacker_seat(address: &str) {
    let (mut link, seated) =
        Link::join(address, "attacker", HeroId::SHADOW_FIEND, None).expect("the first seat");
    let mut cheats = setup().into_iter();
    let (mut planted, mut created, mut attacking) = (false, false, false);
    let decide = move |session: &ClientSession| -> Vec<IssuedOrder> {
        if let Some(cheat) = cheats.next() {
            return vec![IssuedOrder::hero(Order::Cheat { cheat })];
        }
        let Some(view) = session.current() else {
            return Vec::new();
        };
        if !planted {
            let ward = session.own_hero().and_then(|me| {
                me.items
                    .iter()
                    .position(|item| item.is_some_and(|item| item.id == ItemId::OBSERVER_WARD))
            });
            let Some(slot) = ward else {
                return Vec::new();
            };
            planted = true;
            return vec![IssuedOrder::hero(Order::Use {
                slot: ItemSlot(slot as u8),
                target: Target::Pos(WARD),
            })];
        }
        if attacking || view.tick < ATTACK_TICK {
            return Vec::new();
        }
        if !created {
            created = true;
            return vec![IssuedOrder::hero(Order::Cheat {
                cheat: Cheat::CreateHero {
                    hero: HeroId::SHADOW_FIEND,
                    enemy: true,
                    at: BANK,
                },
            })];
        }
        let struck = view.units.iter().find(|unit| {
            unit.kind == UnitKind::Hero
                && unit.team == Team::Dire
                && unit.pos.within(BANK, bota_proto::Fixed::from_int(300))
        });
        let Some(struck) = struck else {
            return Vec::new();
        };
        attacking = true;
        vec![IssuedOrder::hero(Order::Attack {
            target: Target::Unit(struck.id),
        })]
    };
    std::thread::spawn(move || {
        let _ = bota_client_core::play(&mut link, seated, &mut (), decide);
    });
}

/// Pixels of a frame within `half` of its middle, `up` to `down` pixels above it, of the notice's
/// red: red well over its green and blue, and bright enough to read.
fn red_pixels(path: &Path, half: u32, (up, down): (u32, u32)) -> usize {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let frame = bevy::image::Image::from_buffer(
        &bytes,
        bevy::image::ImageType::Extension("png"),
        bevy::image::CompressedImageFormats::NONE,
        true,
        bevy::image::ImageSampler::Default,
        bevy::asset::RenderAssetUsages::MAIN_WORLD,
    )
    .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let (mx, my) = (frame.width() / 2, frame.height() / 2);
    let mut count = 0;
    for y in my.saturating_sub(up)..my.saturating_sub(down) {
        for x in mx.saturating_sub(half)..(mx + half).min(frame.width()) {
            let pixel = frame
                .pixel_bytes(bevy::math::UVec3::new(x, y, 0))
                .expect("a pixel of the frame");
            let (r, g, b) = (
                u32::from(pixel[0]),
                u32::from(pixel[1]),
                u32::from(pixel[2]),
            );
            if r >= 110 && r >= 2 * g && r >= 2 * b {
                count += 1;
            }
        }
    }
    count
}

#[test]
#[ignore = "draws on the GPU in a headless compositor: tools/visual_check.sh runs it"]
fn a_miss_uphill_puts_a_legible_red_miss_over_the_struck_hero_on_the_screen() {
    let kept = std::env::var_os("DBOTA_SHOTS").map(PathBuf::from);
    let shots = kept.clone().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("miss-{}", std::process::id()))
    });
    let _ = std::fs::remove_file(shots.join("miss_before.png"));
    let _ = std::fs::remove_file(shots.join("miss_notice.png"));
    let server = common::serve(&[
        "--cheats",
        "--players",
        "2",
        "--seed",
        "5",
        "--pick-secs",
        "5",
        "--tactical-secs",
        "0",
        "--ack-timeout-ticks",
        "100000",
    ]);
    attacker_seat(&server.address);
    let look = format!("{},{}", BANK.x.to_int(), BANK.y.to_int());
    let window = format!("{}x{}", WINDOW.0, WINDOW.1);
    let script = format!(
        "await:game;click:pick shadow fiend;click:pick ready;await:tick:{BEFORE_TICK};\
         shot:miss_before.png;await:text:miss;shot:miss_notice.png;exit"
    );
    common::play_script_with(
        &server.address,
        &script,
        "miss-notice",
        &shots,
        &["--look", &look, "--distance", DISTANCE, "--window", &window],
    );
    // The struck hero stands in the middle of the frame and its health bar about 130 pixels above
    // the middle at this distance; the notice floats over the bar. The area is the band above the
    // bar, clear of the hero's model and of its attack's impact flash.
    let area = (100, (220, 135));
    let before = red_pixels(&shots.join("miss_before.png"), area.0, area.1);
    let after = red_pixels(&shots.join("miss_notice.png"), area.0, area.1);
    if kept.is_none() {
        let _ = std::fs::remove_dir_all(&shots);
    }
    assert!(
        after >= before + 80,
        "the frame with the notice holds {after} red pixels over the struck hero, the frame \
         before the hero came {before}: the notice adds too little red to be read"
    );
}
