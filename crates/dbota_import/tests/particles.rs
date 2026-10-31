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

/// The classes of the manifest's effects the client leaves out, each with why. (A model
/// renderer whose model a control point carries, the town portal's image of the hero at its end,
/// is read but draws nothing: the client binds no model to a control point.)
const LEFT_OUT: [&str; 44] = [
    // Camera shake (Requiem of Souls) and the model a control point carries (the town portal's
    // image of the hero), and the rune, ward, teleport and lotus effects' finer operators (status
    // effects, parent-kill inheritance, paths, oscillation, rotation and culling): the client
    // draws these effects without them.
    "C_INIT_CreateOnModelAtHeight",
    "C_INIT_CreateSequentialPathV2",
    "C_INIT_InitFromParentKilled",
    "C_INIT_InitSkinnedPositionFromCPSnapshot",
    "C_INIT_NormalOffset",
    "C_INIT_RadiusFromCPObject",
    "C_INIT_RandomNamedModelSequence",
    "C_INIT_RemapNamedModelSequenceToScalar",
    "C_INIT_RemapScalarToVector",
    "C_INIT_SequenceLifeTime",
    "C_INIT_SetAttributeToScalarExpression",
    "C_INIT_SetHitboxToClosest",
    "C_INIT_StatusEffect",
    "C_OP_ConstrainDistance",
    "C_OP_DecayMaintainCount",
    "C_OP_DistanceBetweenCPs",
    "C_OP_DistanceCull",
    "C_OP_FadeIn",
    "C_OP_InheritFromParentParticlesV2",
    "C_OP_InstantaneousEmitter",
    "C_OP_LagCompensation",
    "C_OP_LerpEndCapVector",
    "C_OP_LerpScalar",
    "C_OP_MaintainEmitter",
    "C_OP_MovementRigidAttachToCP",
    "C_OP_NormalLock",
    "C_OP_OscillateScalar",
    "C_OP_RemapCPtoScalar",
    "C_OP_RemapDirectionToCPToVector",
    "C_OP_RemapSpeed",
    "C_OP_RemapSpeedtoCP",
    "C_OP_RemapTransformOrientationToYaw",
    "C_OP_RemapVisibilityScalar",
    "C_OP_RenderScreenShake",
    "C_OP_RenderStatusEffect",
    "C_OP_RotateVector",
    "C_OP_SetControlPointOrientation",
    "C_OP_SetControlPointRotation",
    "C_OP_SetControlPointToCenter",
    "C_OP_SetFloatAttributeToVectorExpression",
    "C_OP_SetRandomControlPointPosition",
    "C_OP_SetToCP",
    "C_OP_SetUserEvent",
    "C_OP_TurbulenceForce",
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
    // (A count pinned here went stale with every effect added; the classes below are the guard.)
    assert!(!roots.is_empty(), "the manifest lists no particle effect");
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
