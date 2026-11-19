//! What each `{key}` of a template stands for.

use bota_core::rules;

use super::blurb::{nums, secs};

/// The value of a key, if the rules have one for it.
pub(super) fn value(key: &str) -> Option<String> {
    Some(match key {
        "rot_dps" => nums(&rules::ROT_DAMAGE_PER_SECOND),
        "rot_slow" => nums(&rules::ROT_SLOW_PCT),
        "raze_stack" => nums(&rules::RAZE_STACK_DAMAGE),
        "raze_secs" => secs(rules::RAZE_DEBUFF_TICKS),
        "raze_max_stacks" => rules::RAZE_MAX_STACKS.to_string(),
        "grenade_dps" => grenade()?.damage_per_second.to_string(),
        "grenade_slow" => grenade()?.slow_pct.to_string(),
        _ => return None,
    })
}

/// What the Blood Grenade does, by bota's catalog.
fn grenade() -> Option<bota_core::Grenade> {
    bota_core::item_def(bota_proto::ItemId::BLOOD_GRENADE).and_then(bota_core::ItemDef::grenade)
}
