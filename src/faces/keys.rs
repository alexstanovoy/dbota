//! What each `{key}` of a template stands for.

use bota_core::rules;

use super::blurb::{fixed, millis, nums, secs};

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
        "talent_armor" => bota_core::TALENT_ARMOR.to_string(),
        "talent_spell_lifesteal" => bota_core::TALENT_SPELL_LIFESTEAL_PCT.to_string(),
        "talent_rot_slow" => rules::TALENT_ROT_SLOW_PCT.to_string(),
        "talent_hook_damage" => rules::TALENT_HOOK_DAMAGE.to_string(),
        "talent_hook_cooldown" => secs(rules::TALENT_HOOK_COOLDOWN_TICKS),
        "talent_dismember_secs" => millis(rules::TALENT_DISMEMBER_CHANNEL.0),
        "talent_dismember_bites" => rules::TALENT_DISMEMBER_BITES.to_string(),
        "talent_heap_factor" => fixed(rules::TALENT_FLESH_HEAP_FACTOR),
        "talent_dismember_factor" => fixed(rules::TALENT_DISMEMBER_DAMAGE_FACTOR),
        "talent_dismember_str_factor" => fixed(rules::TALENT_DISMEMBER_STRENGTH_FACTOR),
        _ => return None,
    })
}

/// What the Blood Grenade does, by bota's catalog.
fn grenade() -> Option<bota_core::Grenade> {
    bota_core::item_def(bota_proto::ItemId::BLOOD_GRENADE).and_then(bota_core::ItemDef::grenade)
}
