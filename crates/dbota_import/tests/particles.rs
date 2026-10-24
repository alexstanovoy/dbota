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
const LEFT_OUT: [&str; 38] = [
    // Not simulated yet: what the missiles of the fountains and the neutrals, the town portal,
    // the Glyph, a level up, a Blood Grenade and Shadow Fiend's death use besides the classes
    // the client simulates.
    "C_INIT_CreateInEpitrochoid",
    "C_INIT_CreateOnModel",
    "C_INIT_CreateSequentialPath",
    "C_INIT_CreateWithinBox",
    "C_INIT_CreationNoise",
    "C_INIT_InheritFromParentParticles",
    "C_INIT_NormalAlignToCP",
    "C_INIT_RadiusFromCPObject",
    "C_INIT_RandomSecondSequence",
    "C_INIT_RemapInitialDirectionToCPToVector",
    "C_INIT_RemapTransformToVector",
    "C_OP_ConstrainDistanceToPath",
    "C_OP_CurlNoiseForce",
    "C_OP_DistanceBetweenCPsToCP",
    "C_OP_LockToBone",
    "C_OP_LockToSavedSequentialPath",
    "C_OP_MaintainSequentialPath",
    "C_OP_MovementRotateParticleAroundAxis",
    "C_OP_Noise",
    "C_OP_NoiseEmitter",
    "C_OP_OscillateScalarSimple",
    "C_OP_PercentageBetweenTransforms",
    "C_OP_PlanarConstraint",
    "C_OP_RadiusDecay",
    "C_OP_RampScalarSplineSimple",
    "C_OP_RemapCPVelocityToVector",
    "C_OP_RemapParticleCountOnScalarEndCap",
    "C_OP_RemapVelocityToVector",
    "C_OP_RenderModels",
    "C_OP_RenderScreenShake",
    "C_OP_RenderScreenVelocityRotate",
    "C_OP_RestartAfterDuration",
    "C_OP_SetCPOrientationToPointAtCP",
    "C_OP_SetControlPointFieldToScalarExpression",
    "C_OP_SetParentControlPointsToChildCP",
    "C_OP_SetSingleControlPointPosition",
    "C_OP_SpinYaw",
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
        27,
        "the missiles of Shadow Fiend, the creeps, the towers, the fountains and the neutrals, \
         Shadow Fiend's abilities and death, the town portal, the Glyph, a level up and a Blood \
         Grenade"
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
