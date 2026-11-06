//! State-to-model rows fail on misspelled facts and invalid elevation.

use dbota_import::catalog::parse;

const MODELS: &str =
    "model ground ground.vmdl_c ground.glb\nmodel flying flying.vmdl_c flying.glb\n";

#[test]
fn model_variants_reject_unknown_models_modifiers_and_invalid_conditions() {
    for (row, why) in [
        (
            "model_variant ground missing modifier=modifier_courier_flying",
            "model_variant: no model missing",
        ),
        (
            "model_variant ground flying modifier=modifier_no_such_effect",
            "model_variant: no modifier modifier_no_such_effect",
        ),
        (
            "model_variant ground flying modifier=modifier_courier_flying height=1025",
            "model_variant: height must be within 0..=1024",
        ),
        (
            "model_variant ground flying pool_tier=great map_skin=300",
            "model_variant: map_skin is no byte",
        ),
        (
            "model_variant ground flying pool_tier=unknown map_skin=0",
            "model_variant: unknown pool_tier=unknown",
        ),
        (
            "model_variant ground flying modifier=modifier_courier_flying pool_tier=great map_skin=0",
            "model_variant requires modifier=, side= or both pool_tier= and map_skin=",
        ),
        (
            "model_variant ground flying side=green",
            "model_variant: unknown side=green",
        ),
        (
            "model_variant ground flying side=radiant modifier=modifier_courier_flying",
            "model_variant requires modifier=, side= or both pool_tier= and map_skin=",
        ),
        (
            "model_variant ground flying modifier=modifier_courier_flying height=160\nmodel_variant ground ground modifier=modifier_courier_flying",
            "model_variant: duplicate condition for ground",
        ),
        (
            "model_variant ground flying modifier=modifier_courier_flying height=160\nmodel_variant ground flying pool_tier=great map_skin=0 height=170",
            "model_variant: conflicting heights for flying",
        ),
    ] {
        let errors = parse(&format!("{MODELS}{row}\n"), "").expect_err(row);
        assert!(
            errors.iter().any(|error| error.why == why),
            "{row}: {errors:?}"
        );
    }
}

#[test]
fn model_variant_rows_keep_existing_asset_conversion_fingerprint() {
    let before = dbota_import::import::Plan::parse(MODELS, "").expect("models");
    let after = dbota_import::import::Plan::parse(
        &format!(
            "{MODELS}model_variant ground flying modifier=modifier_courier_flying height=160\n"
        ),
        "",
    )
    .expect("state row");
    assert_eq!(
        dbota_import::import::plan_id(&before),
        dbota_import::import::plan_id(&after),
        "a rendering choice converts no different assets"
    );
}
