//! The particle effects the manifest lists, read from the local Dota 2 install as the importer
//! reads them: which classes they use that the client does not simulate. The list is explicit,
//! so a manifest line that brings in another one fails here until it is simulated or added.
//!
//! ```sh
//! cargo test --profile test-release -p dbota_import --test particles -- --include-ignored
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
const LEFT_OUT: [&str; 53] = [
    // Camera shake (Requiem of Souls) and the model a control point carries (the town portal's
    // image of the hero), and the rune, ward, teleport and lotus effects' finer operators (status
    // effects, parent-kill inheritance, paths, oscillation, rotation and culling): the client
    // draws these effects without them.
    // And the map objectives' finer functions (their ambients, channels and rings; the Watcher's
    // channel places its motes on a spiral round it with C_INIT_CreateSpiralSphere, which the
    // client does not, so they keep where its other initializers put them): the client draws
    // their effects without them.
    // The Twin Gates' ambient strings a cable of rope between two of its points
    // (team_portal_ambient_rope): the client draws the glow without it.
    "C_INIT_CreateOnModelAtHeight",
    "C_INIT_CreateSequentialPathV2",
    "C_INIT_CreateSpiralSphere",
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
    "C_OP_ColorInterpolateRandom",
    "C_OP_ConstrainDistance",
    "C_OP_CylindricalDistanceToTransform",
    "C_OP_DecayMaintainCount",
    "C_OP_DistanceBetweenCPs",
    "C_OP_DistanceCull",
    "C_OP_FadeIn",
    "C_OP_InheritFromParentParticlesV2",
    "C_OP_InstantaneousEmitter",
    "C_OP_LagCompensation",
    "C_OP_LerpEndCapVector",
    "C_OP_LerpScalar",
    "C_OP_LocalAccelerationForce",
    "C_OP_LockToSavedSequentialPathV2",
    "C_OP_MovementRigidAttachToCP",
    "C_OP_OscillateScalar",
    "C_OP_RemapCPtoCP",
    "C_OP_RemapCPtoScalar",
    "C_OP_RemapCPtoVector",
    "C_OP_RemapDirectionToCPToVector",
    "C_OP_RemapScalarEndCap",
    "C_OP_RemapSpeed",
    "C_OP_RemapSpeedtoCP",
    "C_OP_RemapTransformOrientationToYaw",
    "C_OP_RemapVisibilityScalar",
    "C_OP_RenderCables",
    "C_OP_RenderScreenShake",
    "C_OP_RenderStatusEffect",
    "C_OP_RotateVector",
    "C_OP_SetControlPointOrientation",
    "C_OP_SetControlPointRotation",
    "C_OP_SetControlPointToCenter",
    "C_OP_SetControlPointsToModelParticles",
    "C_OP_SetFloatAttributeToVectorExpression",
    "C_OP_SetRandomControlPointPosition",
    "C_OP_SetUserEvent",
    "C_OP_SnapshotRigidSkinToBones",
    "C_OP_SnapshotSkinToBones",
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

#[test]
#[ignore = "needs a local Dota 2 install"]
fn particle_models_import_the_skin_selected_by_the_installed_renderer() {
    let game = find_game_dir(None).expect("local install");
    let source = DirSource::new(&game);
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let bytes = pak
        .read_file(&source, "models/particle/sphere.vmdl_c")
        .expect("sphere");
    let sphere = heatel::Model::read(&Resource::parse(&bytes).expect("resource")).expect("model");
    let material = &sphere.material_groups[5].1[0];
    let plan = dbota_import::import::Plan::parse(
        "particle bubble particles/neutral_fx/frogmen_water_bubble.vpcf_c particles/bubble.json\n",
        "",
    )
    .expect("particle plan");
    let folder = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("particle_skin_{}", std::process::id()));
    let report = dbota_import::cache_dir::import_into(&game, &folder, &plan, "test", &mut |_| {
        dbota_import::import::Flow::Continue
    })
    .expect("import");
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let particle: serde_json::Value = serde_json::from_slice(
        &std::fs::read(folder.join("particles/bubble.json")).expect("particle"),
    )
    .expect("particle JSON");
    let model = particle["models"]
        .as_array()
        .expect("models")
        .iter()
        .find(|m| m["vmdl"] == "models/particle/sphere.vmdl" && m["skin"] == 5)
        .expect("the sphere's selected skin 5 is imported");
    let glb = std::fs::read(folder.join(model["file"].as_str().expect("model file"))).expect("GLB");
    let length = u32::from_le_bytes(glb[12..16].try_into().expect("JSON length")) as usize;
    let gltf: serde_json::Value = serde_json::from_slice(&glb[20..20 + length]).expect("glTF JSON");
    let expected = material
        .rsplit('/')
        .next()
        .expect("name")
        .trim_end_matches(".vmat");
    assert!(
        gltf["materials"]
            .as_array()
            .expect("materials")
            .iter()
            .any(|m| m["name"] == expected),
        "the authored skin's material {expected}, got {:?}",
        gltf["materials"]
    );
    std::fs::remove_dir_all(folder).expect("scratch removed");
}

/// Geometry copies are currently limited to selected literal groups: 35 model/group pairs
/// from 32 paths, about 1.97 MB of GLBs and 1120 pooled roots. A computed selector must not
/// silently turn a large material table into hundreds of extra scene instances.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn installed_particle_models_fit_the_skin_and_geometry_budget() {
    use heatel::particle::{FloatInput, Renderer};
    let source = DirSource::new(find_game_dir(None).expect("local install"));
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let mut queue: Vec<_> = include_str!("../../../assets-manifest/manifest.txt")
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            (words.next() == Some("particle")).then(|| {
                words
                    .nth(1)
                    .expect("source")
                    .trim_end_matches("_c")
                    .to_owned()
            })
        })
        .collect();
    let mut seen = BTreeSet::new();
    let mut models = BTreeSet::new();
    let mut pairs = BTreeSet::new();
    while let Some(path) = queue.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        let bytes = pak
            .read_file(&source, &format!("{path}_c"))
            .expect("particle");
        let system = System::read(&Resource::parse(&bytes).expect("resource")).expect("system");
        queue.extend(system.children.iter().map(|child| child.path.clone()));
        for renderer in system.renderers {
            let Renderer::Models(model) = renderer.class else {
                continue;
            };
            if model.model_input.is_some() {
                continue;
            }
            let FloatInput::Literal(skin) = model.skin else {
                panic!("{path}: computed skins need a material-only variant/budget review");
            };
            let skin = if skin.is_finite() && (0.0..64.0).contains(&skin) {
                skin as u16
            } else {
                0
            };
            for name in model.paths() {
                models.insert(name.to_owned());
                pairs.insert((name.to_owned(), skin));
            }
        }
    }
    assert!(
        !models.is_empty(),
        "the guard must inspect installed models"
    );
    assert!(
        pairs.len() <= 64,
        "{} model variants exceed the client library capacity",
        pairs.len()
    );
    let mut maximum = 0;
    for model in &models {
        let bytes = pak
            .read_file(&source, &format!("{model}_c"))
            .expect("model");
        let data = Resource::parse(&bytes)
            .expect("resource")
            .data()
            .expect("DATA")
            .expect("model data");
        let count = data
            .get("m_materialGroups")
            .and_then(heatel::kv3::Value::as_array)
            .map_or(0, |groups| groups.len());
        maximum = maximum.max(count);
        assert!(
            count <= 64,
            "{model}: {count} material groups need an explicit budget review"
        );
    }
    eprintln!(
        "particle models: {} paths, {} selected groups, maximum {maximum} material groups",
        models.len(),
        pairs.len()
    );
}
