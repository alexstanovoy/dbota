//! What each `{key}` of a template stands for.

use bota_core::rules;

use super::blurb::{fixed, nums, secs};

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
        "talent_feast_speed" => rules::TALENT_FEAST_ATTACK_SPEED.to_string(),
        "talent_raze_stack" => rules::TALENT_RAZE_STACK_DAMAGE.to_string(),
        "talent_feast_souls" => rules::TALENT_FEAST_SOULS_PER_HERO.to_string(),
        "talent_presence" => fixed(rules::TALENT_PRESENCE_ARMOR),
        "talent_fear" => secs(rules::TALENT_REQUIEM_LINE_TICKS),
        "talent_fear_max" => secs(rules::TALENT_REQUIEM_HOLD_TICKS),
        "talent_souls" => rules::TALENT_NECRO_SOUL_CAP.to_string(),
        "talent_cast_pct" => rules::TALENT_FEAST_CAST_POINT_PCT.to_string(),
        "talent_cast_speed" => (100 - rules::TALENT_FEAST_CAST_POINT_PCT).to_string(),
        _ => return None,
    })
}

/// What the Blood Grenade does, by bota's catalog.
fn grenade() -> Option<bota_core::Grenade> {
    bota_core::item_def(bota_proto::ItemId::BLOOD_GRENADE).and_then(bota_core::ItemDef::grenade)
}
