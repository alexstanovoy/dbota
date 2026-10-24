//! The particle effects the manifest lists, read from the local Dota 2 install as the importer
//! reads them: which classes they use that the client does not simulate. The list is explicit,
//! so a manifest line that brings in another one fails here until it is simulated or added.
//!
//! ```sh
//! cargo test --release -p dbota_import --test particles -- --ignored
//! ```
//!
//! `DBOTA_DOTA_PATH` (a `dota 2 beta` folder) overrides where the install is looked for.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use heatel::particle::System;
use heatel::{DirSource, Resource, Vpk, find_game_dir};

/// The classes of the manifest's effects the client leaves out, each with what it draws or does.
const LEFT_OUT: [&str; 18] = [
    // The fountain's missile: its launch flash's path, its trail turned to the target, restarting.
    "C_INIT_CreateSequentialPath",
    // Noise on the dragons' and the drakes' missiles.
    "C_INIT_CreationNoise",
    // The ghost's missile taking on its parent's particles.
    "C_INIT_InheritFromParentParticles",
    "C_INIT_NormalAlignToCP",
    // A second sheet sequence of the dragons', the drakes' and the ice shamans' missiles.
    "C_INIT_RandomSecondSequence",
    // The gnoll's missile turned along its flight.
    "C_INIT_RemapInitialDirectionToCPToVector",
    // Noise forces on the Dire tower missile's sparks.
    "C_OP_CurlNoiseForce",
    // Noise on the dragons' and the drakes' missiles.
    "C_OP_Noise",
    "C_OP_NoiseEmitter",
    "C_OP_RadiusDecay",
    // The gnoll's missile's impact sized by its particles.
    "C_OP_RemapParticleCountOnScalarEndCap",
    // The ice shamans' missile turned along its flight.
    "C_OP_RemapVelocityToVector",
    // Models: the Dire tower missile's rock, and the neutrals' missiles' bodies.
    "C_OP_RenderModels",
    // Requiem of Souls shaking the camera.
    "C_OP_RenderScreenShake",
    // The fountain's missile, restarting.
    "C_OP_RestartAfterDuration",
    // The gnoll's missile turned at its target.
    "C_OP_SetCPOrientationToPointAtCP",
    // The fountain's launch flash.
    "C_OP_SetSingleControlPointPosition",
    // Collision of the Dire tower missile's core with the ground.
    "C_OP_WorldTraceConstraint",
];

#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_manifests_effects_leave_out_only_the_listed_classes() {
    let dota = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    let src = DirSource::new(
        find_game_dir(dota.as_deref()).expect("Dota 2 not found; set DBOTA_DOTA_PATH"),
    );
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    let manifest = include_str!("../../../assets-manifest/manifest.txt");
    let roots: Vec<&str> = manifest
        .lines()
        .filter_map(
            |line| match line.split_whitespace().collect::<Vec<_>>()[..] {
                ["particle", _, src, ..] => Some(src),
                _ => None,
            },
        )
        .collect();
    assert_eq!(
        roots.len(),
        20,
        "the missiles of Shadow Fiend, the creeps, the towers, the fountains and the neutrals, and \
         Shadow Fiend's abilities"
    );
    let mut left_out: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for root in roots {
        let mut queue = vec![root.strip_suffix("_c").unwrap().to_owned()];
        let mut seen = BTreeSet::new();
        while let Some(path) = queue.pop() {
            if !seen.insert(path.clone()) {
                continue;
            }
            let bytes = pak.read_file(&src, &format!("{path}_c")).unwrap();
            let system = System::read(&Resource::parse(&bytes).unwrap()).unwrap();
            for class in system.unsupported_classes() {
                left_out.entry(class.to_owned()).or_default().insert(root);
            }
            queue.extend(system.children.iter().map(|c| c.path.clone()));
        }
    }
    let classes: Vec<&str> = left_out.keys().map(String::as_str).collect();
    assert_eq!(classes, LEFT_OUT, "{left_out:#?}");
}
