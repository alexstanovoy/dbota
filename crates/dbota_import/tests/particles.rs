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
const LEFT_OUT: [&str; 50] = [
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
    let game = local_install();
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

/// Manifest and map effects share the client's model budget. Count unique geometry files,
/// including output directories and aliases whose absent material group resolves to zero.
#[test]
#[ignore = "needs a local Dota 2 install"]
fn installed_particle_models_fit_the_skin_and_geometry_budget() {
    let source = DirSource::new(local_install());
    let pak = Vpk::open(&source, "pak01_dir.vpk").expect("VPK");
    let requests = installed_model_requests(&source, &pak);
    assert!(
        !requests.is_empty(),
        "the guard must inspect installed models"
    );
    let mut counts = BTreeMap::new();
    let mut geometry = BTreeSet::new();
    let mut selected = 0;
    for ((dir, model), selectors) in &requests {
        let count = *counts.entry(model.clone()).or_insert_with(|| {
            let bytes = pak
                .read_file(&source, &format!("{model}_c"))
                .expect("model");
            let data = Resource::parse(&bytes)
                .expect("resource")
                .data()
                .expect("DATA")
                .expect("model data");
            data.get("m_materialGroups")
                .and_then(heatel::kv3::Value::as_array)
                .map_or(1, |g| g.len().max(1))
        });
        assert!(
            count <= 64,
            "{model}: {count} groups need an explicit budget review"
        );
        for selector in selectors {
            let indices = match selector {
                Some(skin) => usize::from(*skin)..usize::from(*skin) + 1,
                None => 0..count,
            };
            for skin in indices {
                selected += 1;
                geometry.insert((dir, model, if skin < count { skin } else { 0 }));
            }
        }
    }
    assert!(
        geometry.len() <= 64,
        "{} geometry variants exceed the shared client budget",
        geometry.len()
    );
    eprintln!(
        "particle models including map ambient: {} source paths, {} directory/model pairs, {selected} selected groups, {} unique geometry files, max {} groups; {} total pooled roots, {} extra roots from skins",
        counts.len(),
        requests.len(),
        geometry.len(),
        counts.values().max().expect("tables"),
        geometry.len() * 32,
        (geometry.len() - requests.len()) * 32
    );
}

fn installed_particle_roots(source: &DirSource) -> Vec<(String, String)> {
    use heatel::world::EntityLump;
    let mut roots = Vec::new();
    let mut ambient_count = 0;
    for line in include_str!("../../../assets-manifest/manifest.txt").lines() {
        let words: Vec<_> = line.split_whitespace().collect();
        if words.first() == Some(&"particle") {
            let dir = words[3].rsplit_once('/').map_or("", |(dir, _)| dir);
            roots.push((words[2].trim_end_matches("_c").to_owned(), dir.to_owned()));
        }
        if words.first() != Some(&"world") || !words.iter().any(|w| w.starts_with("ambient=")) {
            continue;
        }
        let vpk = words
            .iter()
            .find_map(|w| w.strip_prefix("vpk="))
            .expect("map VPK");
        let map = Vpk::open(source, vpk).expect("map");
        let bytes = map.read_file(source, words[2]).expect("world");
        let world = Resource::parse(&bytes)
            .expect("world resource")
            .data()
            .expect("DATA")
            .expect("world data");
        let lump = world
            .get("m_entityLumps")
            .and_then(heatel::kv3::Value::as_array)
            .and_then(|v| v.first())
            .and_then(heatel::kv3::Value::as_str)
            .expect("default entity lump");
        let bytes = map.read_file(source, &format!("{lump}_c")).expect("lump");
        let lump =
            EntityLump::read(&Resource::parse(&bytes).expect("lump resource")).expect("entities");
        let (_, effects) = dbota_import::text::ambient_txt(&lump.entities);
        ambient_count += effects.len();
        roots.extend(
            effects
                .into_iter()
                .map(|path| (path, "particles/map".to_owned())),
        );
    }
    assert!(
        ambient_count > 0,
        "the guard must include installed map emitters"
    );
    roots
}

type ModelRequests = BTreeMap<(String, String), BTreeSet<Option<u16>>>;

fn installed_model_requests(source: &DirSource, pak: &Vpk) -> ModelRequests {
    use heatel::particle::{FloatInput, Renderer};
    let mut queue = installed_particle_roots(source);
    let mut seen = BTreeSet::new();
    let mut requests = ModelRequests::new();
    while let Some((path, dir)) = queue.pop() {
        if !seen.insert((path.clone(), dir.clone())) {
            continue;
        }
        let bytes = pak
            .read_file(source, &format!("{path}_c"))
            .expect("particle");
        let system = System::read(&Resource::parse(&bytes).expect("resource")).expect("system");
        queue.extend(
            system
                .children
                .iter()
                .map(|child| (child.path.clone(), dir.clone())),
        );
        for renderer in system.renderers {
            let Renderer::Models(model) = renderer.class else {
                continue;
            };
            if model.model_input.is_some() {
                continue;
            }
            let skin = match model.skin {
                FloatInput::Literal(value) => {
                    Some(if value.is_finite() && (0.0..64.0).contains(&value) {
                        value as u16
                    } else {
                        0
                    })
                }
                _ => None,
            };
            for name in model.paths() {
                requests
                    .entry((dir.clone(), name.to_owned()))
                    .or_default()
                    .insert(skin);
            }
        }
    }
    requests
}

fn local_install() -> PathBuf {
    let provided = std::env::var_os("DBOTA_DOTA_PATH").map(PathBuf::from);
    find_game_dir(provided.as_deref()).expect("local install; set DBOTA_DOTA_PATH")
}
