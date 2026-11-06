//! The simulator's numbers under Dota's names for them.
//!
//! Dota's text says `%blink_range%` or labels a line `DAMAGE:`; the number
//! printed in its place is the one bota simulates, found here by the key
//! Dota's scripts give it. A key bota has no number for is left to Dota's
//! own number by the caller.

use bota_core::{
    AbilityDef, Carried, ItemDef, Odds, UseSpec,
    rules::{self, TICKS_PER_SECOND},
};
use bota_proto::{AbilityId, Fixed};

use crate::dota_text::plain;

/// What a number is asked about.
#[derive(Clone, Copy)]
pub enum Subject {
    Item(&'static ItemDef),
    Ability(AbilityId, &'static AbilityDef),
}

/// A number per level; one for an item or a number that does not grow.
pub type Levels = Vec<String>;

/// The simulator's number for a Dota key (lower case), if it has one.
pub fn value(subject: Subject, key: &str) -> Option<Levels> {
    match subject {
        Subject::Item(def) => item_value(def, key),
        Subject::Ability(id, def) => ability_value(id, def, key),
    }
}

/// How much a unit's cooldowns and mana costs come to, in basis points of the table's, as its
/// view tells them (`UnitView::cast_rates`); the table's own when it tells none.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rates {
    pub cooldown_bp: i32,
    pub mana_bp: i32,
}

impl Default for Rates {
    fn default() -> Rates {
        Rates {
            cooldown_bp: rules::NOMINAL_BP,
            mana_bp: rules::NOMINAL_BP,
        }
    }
}

impl Rates {
    /// The rates a unit's view tells.
    pub fn of_view(unit: &bota_proto::UnitView) -> Rates {
        unit.cast_rates
            .map_or_else(Rates::default, |(cooldown_bp, mana_bp)| Rates {
                cooldown_bp,
                mana_bp,
            })
    }
}

/// The wait after a use, per level, as the table has it.
pub fn cooldown(subject: Subject) -> Levels {
    cooldown_under(subject, Rates::default())
}

/// The wait after a use, per level, under `rates`.
pub fn cooldown_under(subject: Subject, rates: Rates) -> Levels {
    match subject {
        Subject::Item(def) => collapse(&[secs(bota_core::cooldown_after(
            def.cooldown,
            rates.cooldown_bp,
        ))]),
        Subject::Ability(_, def) => collapse(
            &def.cooldown
                .iter()
                .map(|t| secs(bota_core::cooldown_after(*t, rates.cooldown_bp)))
                .collect::<Vec<_>>(),
        ),
    }
}

/// The mana a use costs, per level, as the table has it.
#[cfg(test)]
pub fn mana(subject: Subject) -> Levels {
    mana_under(subject, Rates::default())
}

/// The mana a use costs, per level, under `rates`.
pub fn mana_under(subject: Subject, rates: Rates) -> Levels {
    match subject {
        Subject::Item(def) => {
            collapse(&[bota_core::cost_after(def.mana_cost, rates.mana_bp).to_string()])
        }
        Subject::Ability(_, def) => collapse(
            &def.mana
                .iter()
                .map(|m| bota_core::cost_after(*m, rates.mana_bp).to_string())
                .collect::<Vec<_>>(),
        ),
    }
}

/// Numbers that are all alike are one number.
pub fn collapse(levels: &[String]) -> Levels {
    match levels.split_first() {
        Some((first, rest)) if rest.iter().all(|n| n == first) => vec![first.clone()],
        _ => levels.to_vec(),
    }
}

fn secs(ticks: u32) -> String {
    plain(f64::from(ticks) / f64::from(TICKS_PER_SECOND))
}

fn one(n: impl ToString) -> Option<Levels> {
    Some(vec![n.to_string()])
}

fn many<T: ToString>(list: &[T]) -> Option<Levels> {
    Some(collapse(&list.iter().map(T::to_string).collect::<Vec<_>>()))
}

fn fixed(n: Fixed) -> String {
    plain(f64::from(n.to_f32()))
}

fn per_second(per_tick: Fixed) -> String {
    plain(f64::from(per_tick.to_f32()) * f64::from(TICKS_PER_SECOND))
}

fn percent(chance: Odds) -> String {
    plain(f64::from(chance.get()) * 100.0 / f64::from(Odds::WHOLE))
}

fn item_value(def: &ItemDef, key: &str) -> Option<Levels> {
    let c: &Carried = &def.carried;
    let a = c.attributes;
    let all = a.strength == a.agility && a.agility == a.intelligence;
    match key {
        "bonus_strength" | "bonus_str" => one(a.strength.to_int()),
        "bonus_agility" | "bonus_agi" => one(a.agility.to_int()),
        "bonus_intellect" | "bonus_int" => one(a.intelligence.to_int()),
        "bonus_all_stats" if all => one(a.strength.to_int()),
        "bonus_damage" => one(c.damage),
        "bonus_damage_melee" => one(def.carried_by(false).damage),
        "bonus_damage_range" => one(def.carried_by(true).damage),
        "bonus_attack_speed" | "bonus_speed" => one(c.attack_speed),
        "bonus_attack_speed_pct" => one(c.base_attack_speed_pct),
        "bonus_armor" => one(fixed(c.armor)),
        "bonus_health" => one(c.hp),
        "bonus_mana" => one(c.mana),
        "bonus_health_regen" | "bonus_regen" | "hp_regen" => one(per_second(c.hp_regen)),
        "bonus_mana_regen" => one(per_second(c.mana_regen)),
        "bonus_movement_speed" => one(c.move_speed),
        "bonus_movement_speed_melee" => one(def.carried_by(false).move_speed),
        "bonus_movement_speed_ranged" => one(def.carried_by(true).move_speed),
        "movement_speed" => one(c.lace_speed),
        "bonus_evasion" => one(percent(c.evasion)),
        "bonus_max_mana_percentage" => one(c.mana_pct),
        "lifesteal_percent" => one(c.lifesteal_pct),
        "spell_lifesteal" => one(c.spell_lifesteal_pct),
        "bonus_magical_armor" | "tooltip_resist" => {
            one(plain(f64::from(c.magic_resist_bp) / 100.0))
        }
        "melee_attack_range" => one(c.melee_range),
        "bonus_stat" => one(def.mode_bonus),
        "bonus_chance" => one(percent(c.pierce)),
        "bonus_chance_damage" => one(c.pierce_damage),
        "damage_bonus" => one(c.damage_to_creeps),
        "damage_bonus_ranged" => one(def.carried_by(true).damage_to_creeps),
        "abilitycastrange" | "quelling_range_tooltip" => one(def.cast_range),
        "tooltip_charges" => one(def.charges),
        "max_charges" => one(def.cast_charges),
        "charge_radius" => one(def.charge_radius()?),
        _ => attack_value(def, key).or_else(|| use_value(def, key)),
    }
}

/// A value of what the item's passive lays on whatever its carrier's attacks strike.
fn attack_value(def: &ItemDef, key: &str) -> Option<Levels> {
    let laid = def.on_attack?;
    match key {
        "corruption_armor" => one(fixed(-laid.armor)),
        "corruption_duration" | "poison_duration" | "duration" => one(secs(laid.ticks)),
        "slow_melee" => one(-laid.slow_pct),
        "slow_ranged" => one(-laid.ranged_slow_pct),
        "heal_reduction" => one(laid.restoration_cut_pct),
        "damage" if laid.dps > 0 => one(laid.dps),
        "tick_interval" if laid.dps > 0 => one(secs(laid.beat_ticks)),
        _ => None,
    }
}

fn use_value(def: &ItemDef, key: &str) -> Option<Levels> {
    match (&def.effect, key) {
        (UseSpec::Mend(m), "buff_duration") => one(secs(m.ticks)),
        (UseSpec::Mend(m), "mana_regen" | "health_regen") => one(plain(
            f64::from(m.total) * f64::from(TICKS_PER_SECOND) / f64::from(m.ticks),
        )),
        (UseSpec::PlantTree { ticks, .. }, "tree_duration") => one(secs(*ticks)),
        (UseSpec::Ward { ticks, .. }, "duration_minutes_tooltip") => {
            one(plain(f64::from(*ticks) / f64::from(60 * TICKS_PER_SECOND)))
        }
        (UseSpec::Ward { units, .. }, "vision_range_tooltip") => one(units[0].unit().day_vision),
        (UseSpec::Ward { units, .. }, "true_sight_range") => one(units[0].unit().true_sight),
        (UseSpec::Teleport(_), "tooltip_channel_time") => one(secs(def.duration)),
        (UseSpec::Blink { range, .. }, "blink_range") => one(range),
        (UseSpec::Blink { overshoot, .. }, "blink_range_clamp") => one(overshoot),
        (UseSpec::Blink { .. }, "blink_damage_cooldown") => one(secs(def.breaks_on_damage)),
        (UseSpec::Phase { ticks, .. }, "phase_duration") => one(secs(*ticks)),
        (UseSpec::Phase { pct, .. }, "phase_movement_speed") => one(pct),
        (UseSpec::Phase { ranged_pct, .. }, "phase_movement_speed_range") => one(ranged_pct),
        (UseSpec::Restore { hp_per_charge, .. }, "restore_per_charge") => one(hp_per_charge),
        (UseSpec::Replenish { mana }, "replenish_amount") => one(mana),
        (UseSpec::Eat { health, .. }, "hp_restore") => one(health),
        (UseSpec::Grenade(g), "impact_damage") => one(g.impact_damage),
        (UseSpec::Grenade(g), "damage_over_time") => one(g.damage_per_second),
        (UseSpec::Grenade(g), "debuff_duration") => one(secs(g.debuff_ticks)),
        (UseSpec::Grenade(g), "movespeed_slow") => one(-g.slow_pct),
        (UseSpec::Grenade(g), "radius") => one(g.radius),
        _ => None,
    }
}

fn negated(list: &[i32]) -> Option<Levels> {
    many(&list.iter().map(|n| -n).collect::<Vec<_>>())
}

fn ability_value(id: AbilityId, def: &'static AbilityDef, key: &str) -> Option<Levels> {
    if key == "abilitycooldown" {
        return Some(cooldown(Subject::Ability(id, def)));
    }
    match id {
        AbilityId::MEAT_HOOK => match key {
            "damage" => many(&rules::HOOK_DAMAGE),
            "abilitycastrange" => one(rules::HOOK_RANGE),
            "hook_speed" => one(rules::HOOK_SPEED),
            "hook_width" => one(rules::HOOK_RADIUS),
            _ => None,
        },
        AbilityId::ROT => match key {
            "rot_radius" => one(rules::ROT_RADIUS),
            "rot_slow" => negated(&rules::ROT_SLOW_PCT),
            "rot_damage" => many(&rules::ROT_DAMAGE_PER_SECOND),
            _ => None,
        },
        AbilityId::FLESH_HEAP => match key {
            "flesh_heap_strength_buff_amount" => {
                Some(collapse(&[fixed(rules::FLESH_HEAP_STRENGTH_PER_STACK)]))
            }
            "flesh_heap_range" => one(rules::FLESH_HEAP_RANGE),
            _ => None,
        },
        AbilityId::DISMEMBER => match key {
            "dismember_damage" => many(&rules::DISMEMBER_DAMAGE_PER_SECOND),
            "strength_damage" => Some(collapse(&rules::DISMEMBER_STRENGTH_DAMAGE.map(fixed))),
            "ticks" => one(rules::DISMEMBER_BITES),
            "pull_units_per_second" => one(rules::DISMEMBER_PULL_SPEED),
            "pull_distance_limit" => one(rules::DISMEMBER_PULL_LIMIT),
            "abilitychanneltime" => one(secs(rules::DISMEMBER_TICKS)),
            _ => None,
        },
        AbilityId::MEAT_SHIELD => match key {
            "damage_block" => many(&rules::MEAT_SHIELD_BLOCK),
            "duration" => Some(collapse(&rules::MEAT_SHIELD_TICKS.map(secs))),
            _ => None,
        },
        AbilityId::BURST => match key {
            "duration" => one(secs(rules::COURIER_BURST_TICKS)),
            "movement_speed" => one(rules::COURIER_BURST_PCT),
            _ => None,
        },
        AbilityId::SHIELD => (key == "duration").then(|| vec![secs(rules::COURIER_SHIELD_TICKS)]),
        AbilityId::RAZE_NEAR | AbilityId::RAZE_MID | AbilityId::RAZE_FAR => raze_value(def, key),
        AbilityId::NECROMASTERY => match key {
            "necromastery_damage_per_soul" => {
                one(plain(f64::from(rules::SOUL_DAMAGE_TENTHS) / 10.0))
            }
            "necromastery_max_souls" => one(rules::NECRO_SOUL_CAP),
            "souls_per_kill" => one(rules::SOULS_PER_UNIT),
            "souls_per_hero_kill" => one(rules::SOULS_PER_HERO),
            "soul_release_tooltip" => one(rules::SOULS_LOST_ON_DEATH_PCT),
            _ => None,
        },
        AbilityId::PRESENCE => match key {
            "presence_armor_reduction" => Some(collapse(
                &rules::PRESENCE_ARMOR.map(|n| fixed(Fixed::ZERO - n)),
            )),
            "presence_radius" => one(def.range),
            _ => None,
        },
        AbilityId::FEAST_OF_SOULS => feast_value(def, key),
        AbilityId::REQUIEM => requiem_value(def, key),
        _ => None,
    }
}

fn raze_value(def: &AbilityDef, key: &str) -> Option<Levels> {
    match key {
        "shadowraze_damage" => many(&rules::RAZE_DAMAGE),
        "shadowraze_range" => one(def.range),
        "shadowraze_radius" => one(rules::RAZE_RADIUS),
        "stack_bonus_damage" => many(&rules::RAZE_STACK_DAMAGE),
        "duration" => one(secs(rules::RAZE_DEBUFF_TICKS)),
        "damage_per_soul" => one(rules::RAZE_DAMAGE_PER_SOUL),
        _ => None,
    }
}

fn feast_value(def: &AbilityDef, key: &str) -> Option<Levels> {
    match key {
        "bonus_attack_speed" => many(&rules::FEAST_ATTACK_SPEED),
        "bonus_move_speed" => many(&rules::FEAST_MOVE_PCT),
        "duration" => one(secs(rules::FEAST_TICKS)),
        "max_collection_count" => many(&rules::FEAST_TARGETS),
        "soul_collection_per_hero" => one(rules::FEAST_SOULS_PER_HERO),
        "soul_collection_per_creep" => one(rules::FEAST_SOULS_PER_UNIT),
        "soul_collection_interval" => one(secs(rules::FEAST_PERIOD_TICKS)),
        "soul_collection_per_interval" => one(rules::FEAST_PER_PERIOD),
        "soul_collection_radius" => one(def.range),
        "linger_duration" => one(secs(rules::FEAST_LINGER_TICKS)),
        _ => None,
    }
}

fn requiem_value(def: &AbilityDef, key: &str) -> Option<Levels> {
    match key {
        "max_soul_release" => one(rules::REQUIEM_LINES_MAX),
        "requiem_slow_duration" => one(secs(rules::REQUIEM_LINE_TICKS)),
        "requiem_slow_duration_max" => one(secs(rules::REQUIEM_HOLD_MAX_TICKS)),
        "requiem_reduction_ms" => negated(&rules::REQUIEM_SLOW_PCT),
        "requiem_reduction_mres" => negated(&rules::REQUIEM_MAGIC_BREAK_PCT),
        "requiem_radius" => one(def.range),
        "requiem_line_width_start" => one(rules::REQUIEM_LINE_WIDTH_START),
        "requiem_line_width_end" => one(rules::REQUIEM_LINE_WIDTH_END),
        "soul_death_release" => one(plain(f64::from(rules::REQUIEM_DEATH_LINES_PCT) / 100.0)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use bota_core::item_def;
    use bota_proto::ItemId;

    use super::*;

    /// Orb of Venom's tooltip prints `damage`, `tick_interval` and `poison_duration` as the
    /// simulator holds them: 12 every 1 second for 3 seconds.
    #[test]
    fn the_poison_attack_tooltip_reads_its_beat_from_the_simulator() {
        let venom = item_def(ItemId::ORB_OF_VENOM).expect("an Orb of Venom");
        let shown = |key| value(Subject::Item(venom), key);
        assert_eq!(shown("damage"), Some(vec!["12".to_owned()]));
        assert_eq!(shown("tick_interval"), Some(vec!["1".to_owned()]));
        assert_eq!(shown("poison_duration"), Some(vec!["3".to_owned()]));
    }

    /// A poison that beat twice a second would be told so: the tooltip follows the simulator's
    /// beat rather than a number of its own.
    #[test]
    fn a_faster_beat_is_told_as_it_is() {
        static FAST: std::sync::LazyLock<ItemDef> = std::sync::LazyLock::new(|| {
            let venom = item_def(ItemId::ORB_OF_VENOM).expect("an Orb of Venom");
            let laid = venom.on_attack.expect("it poisons");
            ItemDef {
                on_attack: Some(bota_core::OnAttack {
                    beat_ticks: TICKS_PER_SECOND / 2,
                    ..laid
                }),
                ..*venom
            }
        });
        assert_eq!(
            value(Subject::Item(&FAST), "tick_interval"),
            Some(vec!["0.5".to_owned()])
        );
    }
}
