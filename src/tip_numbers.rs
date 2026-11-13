//! The simulator's numbers under Dota's names for them.
//!
//! Dota's text says `%blink_range%` or labels a line `DAMAGE:`; the number
//! printed in its place is the one bota simulates, found here by the key
//! Dota's scripts give it. A key bota has no number for is left to Dota's
//! own number by the caller.

use bota_core::{
    AbilityDef, Carried, ItemDef, Odds, Strike, UseSpec,
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
        // Heart of Tarrasque's `hp_regen` is a percent of the maximum health; a Mango's is flat.
        "hp_regen" if c.max_hp_regen_bp > 0 => one(of_bp(c.max_hp_regen_bp)),
        "missing_health_regen" => one(of_bp(c.missing_hp_regen_bp)),
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
        "aoe_bonus" => one(c.aoe_bonus),
        "movement_speed_percent_bonus" => one(def.carried_by(true).move_speed_pct),
        "movement_speed_percent_bonus_melee" => one(def.carried_by(false).move_speed_pct),
        "slow_resistance" => one(of_bp(c.slow_resist_bp)),
        "hp_regen_amp" => one(of_bp(c.restoration_amp_bp)),
        "status_resistance" => one(of_bp(c.status_resist_bp)),
        "manacost_reduction" => one(of_bp(c.mana_cost_cut_bp)),
        "bonus_cooldown" => one(of_bp(c.cooldown_cut_bp)),
        "spell_amp" => one(c.spell_amp_pct),
        "mana_regen_multiplier" => one(c.mana_regen_amp_pct),
        "cast_speed_pct" => one(c.cast_speed_pct),
        "cast_range_bonus" => one(c.cast_range_bonus),
        "base_attack_range" => one(c.ranged_range),
        "base_attack_range_melee" => one(c.melee_range),
        "bonus_magical_armor" | "tooltip_resist" => {
            one(plain(f64::from(c.magic_resist_bp) / 100.0))
        }
        "melee_attack_range" => one(c.melee_range),
        "bonus_stat" => one(def.mode_bonus),
        "bonus_chance" => one(percent(c.pierce)),
        // A pierce's damage (Javelin's); a bash's is its strike's.
        "bonus_chance_damage" if c.pierce != Odds::NEVER => one(c.pierce_damage),
        "damage_bonus" | "quelling_bonus" => one(def.carried_by(false).damage_to_creeps),
        "damage_bonus_ranged" | "quelling_bonus_ranged" => {
            one(def.carried_by(true).damage_to_creeps)
        }
        "agility" => one(a.agility.to_int()),
        "abilitycastrange" | "quelling_range_tooltip" => one(def.cast_range),
        "tooltip_charges" => one(def.charges),
        "max_charges" => one(def.cast_charges),
        "charge_radius" => one(def.charge_radius()?),
        _ => attack_value(def, key)
            .or_else(|| block_value(def, key))
            .or_else(|| use_value(def, key))
            .or_else(|| strike_value(def, key))
            .or_else(|| damage_block_value(def, key))
            // A plain `damage` (Specialist's Array's) is the attack damage it adds, where no
            // passive of its attacks names a damage of its own (Orb of Venom's).
            .or_else(|| (key == "damage").then(|| vec![c.damage.to_string()])),
    }
}

/// A value of the procs the item's passive gives its carrier's attacks, read off the carrier the
/// key is about: a bash's chance is a melee or a ranged carrier's, every other key the first
/// carrier's whose strikes have it (Cleave a melee one's, Splitshot a ranged one's).
fn strike_value(def: &ItemDef, key: &str) -> Option<Levels> {
    let carriers: &[bool] = match key {
        "bash_chance_melee" => &[false],
        "bash_chance_ranged" => &[true],
        _ => &[false, true],
    };
    let mut strikes = carriers
        .iter()
        .flat_map(|ranged| def.carried_by(*ranged).strikes);
    if key == "bash_cooldown" {
        // A bash waits out the item's own cooldown.
        return strikes
            .any(|strike| matches!(strike, Strike::Bash { .. }))
            .then(|| vec![secs(def.cooldown)]);
    }
    strikes.find_map(|strike| strike_number(*strike, key))
}

fn strike_number(strike: Strike, key: &str) -> Option<Levels> {
    match (strike, key) {
        (Strike::Crit { chance_pct, .. }, "crit_chance") => one(chance_pct),
        (Strike::Crit { mult_pct, .. }, "crit_multiplier") => many(&mult_pct),
        // What a critical strike deals beyond the swing (Crystalys's): its multiplier past 100.
        (Strike::Crit { mult_pct, .. }, "tooltip_crit_damage") => many(&mult_pct.map(|m| m - 100)),
        (Strike::Bash { chance_pct, .. }, "bash_chance_melee" | "bash_chance_ranged") => {
            one(chance_pct)
        }
        (Strike::Bash { damage, .. }, "bonus_chance_damage") => many(&damage),
        (Strike::Bash { stun_ms, .. }, "bash_duration") => Some(collapse(&stun_ms.map(of_ms))),
        (Strike::Cleave { hero_pct, .. }, "cleave_damage_percent") => one(hero_pct),
        (Strike::Cleave { creep_pct, .. }, "cleave_damage_percent_creep") => one(creep_pct),
        (Strike::Cleave { start_width, .. }, "cleave_starting_width") => one(start_width),
        (Strike::Cleave { end_width, .. }, "cleave_ending_width") => one(end_width),
        (Strike::Cleave { distance, .. }, "cleave_distance") => one(distance),
        (Strike::Chain { chance_pct, .. }, "chain_chance") => one(chance_pct),
        (Strike::Chain { damage, .. }, "chain_damage") => one(damage),
        (Strike::Chain { strikes, .. }, "chain_strikes") => one(strikes),
        (Strike::Chain { radius, .. }, "chain_radius") => one(radius),
        (Strike::Chain { delay_ms, .. }, "chain_delay") => one(of_ms(delay_ms)),
        (Strike::Chain { wait_ms, .. }, "chain_cooldown") => one(of_ms(wait_ms)),
        (Strike::Split { chance_pct, .. }, "proc_chance") => one(chance_pct),
        (Strike::Split { count, .. }, "count") => one(count),
        (Strike::Split { range_bonus, .. }, "secondary_target_range_bonus") => one(range_bonus),
        (Strike::Split { angle, .. }, "secondary_target_angle") => one(angle),
        (Strike::Split { base_damage, .. }, "base_proc_dmg") => one(base_damage),
        (Strike::Split { pct, .. }, "proc_dmg_pct") => one(pct),
        _ => None,
    }
}

/// A value of the damage the item's passive blocks off an attack on its carrier, of a melee or a
/// ranged carrier as the key says.
fn damage_block_value(def: &ItemDef, key: &str) -> Option<Levels> {
    let amount = |ranged| {
        def.carried_by(ranged)
            .damage_block
            .map(|block| block.amount)
    };
    match key {
        "block_chance" => one(percent(def.carried.damage_block?.chance)),
        "block_damage_melee" => one(amount(false)?),
        "block_damage_ranged" => one(amount(true)?),
        _ => None,
    }
}

/// Milliseconds as the seconds Dota's text prints.
fn of_ms(ms: i32) -> String {
    plain(f64::from(ms) / 1000.0)
}

/// A share in basis points as the percent Dota's text prints.
fn of_bp(bp: i32) -> String {
    plain(f64::from(bp) / 100.0)
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

/// A value of the magical damage the item's passive blocks off a blow on its carrier.
fn block_value(def: &ItemDef, key: &str) -> Option<Levels> {
    let block = def.magic_block?;
    match key {
        "magic_damage_block" => one(block.amount),
        "min_damage" => one(block.min_damage),
        "initial_charges" => one(def.charges),
        "mana_regen" => one(per_second(def.carried.mana_regen)),
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
        (UseSpec::Ethereal { ticks, .. }, "duration") => one(secs(*ticks)),
        (UseSpec::Ethereal { magic_pct, .. }, "extra_spell_damage_percent") => one(-magic_pct),
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

    /// The tooltips of the stat upgrades of Dota 7.41f's first item batch print what the simulator
    /// carries under each Dota key: percents of a share, ranges and amplifications.
    #[test]
    fn the_stat_upgrades_tooltips_read_their_stats_from_the_simulator() {
        let shown = |item, key| value(Subject::Item(item_def(item).expect("an item")), key);
        let one = |text: &str| Some(vec![text.to_owned()]);
        assert_eq!(
            shown(ItemId::YASHA, "movement_speed_percent_bonus"),
            one("10")
        );
        assert_eq!(shown(ItemId::SANGE, "slow_resistance"), one("25"));
        assert_eq!(shown(ItemId::SANGE, "hp_regen_amp"), one("12"));
        assert_eq!(shown(ItemId::KAYA, "spell_amp"), one("10"));
        assert_eq!(shown(ItemId::KAYA, "mana_regen_multiplier"), one("20"));
        assert_eq!(
            shown(ItemId::SANGE_AND_YASHA, "status_resistance"),
            one("16")
        );
        assert_eq!(
            shown(
                ItemId::SANGE_AND_YASHA,
                "movement_speed_percent_bonus_melee"
            ),
            one("12")
        );
        assert_eq!(
            shown(ItemId::KAYA_AND_SANGE, "manacost_reduction"),
            one("25")
        );
        assert_eq!(shown(ItemId::YASHA_AND_KAYA, "cast_speed_pct"), one("25"));
        assert_eq!(shown(ItemId::AETHER_LENS, "cast_range_bonus"), one("225"));
        assert_eq!(shown(ItemId::OCTARINE_CORE, "bonus_cooldown"), one("25"));
        assert_eq!(shown(ItemId::DRAGON_LANCE, "base_attack_range"), one("130"));
        assert_eq!(shown(ItemId::HEART, "hp_regen"), one("1"));
        assert_eq!(shown(ItemId::HEART, "missing_health_regen"), one("1.5"));
        let mango = shown(ItemId::MANGO, "hp_regen");
        assert!(mango.is_some(), "a Mango's flat regeneration");
        assert_eq!(mango, shown(ItemId::MANGO, "bonus_health_regen"));
    }

    /// The tooltips of the attack procs of Dota 7.41f's second item batch print what the simulator
    /// strikes with under each Dota key: critical strikes, the bash, Cleave and Quell, Chain
    /// Lightning, Splitshot and Damage Block, each of the carrier (melee or ranged) its key names.
    #[test]
    fn the_attack_procs_tooltips_read_their_strikes_from_the_simulator() {
        let expect = |item: ItemId, pairs: &[(&str, &str)]| {
            let def = item_def(item).expect("an item");
            for (key, number) in pairs {
                assert_eq!(
                    value(Subject::Item(def), key),
                    Some(vec![(*number).to_owned()]),
                    "{item:?} {key}"
                );
            }
        };
        expect(
            ItemId::CRYSTALYS,
            &[
                ("crit_chance", "30"),
                ("crit_multiplier", "160"),
                ("tooltip_crit_damage", "60"),
                ("bonus_damage", "32"),
            ],
        );
        expect(
            ItemId::DAEDALUS,
            &[
                ("crit_chance", "30"),
                ("crit_multiplier", "225"),
                ("bonus_damage", "88"),
            ],
        );
        expect(
            ItemId::REVENANTS_BROOCH,
            &[
                ("crit_chance", "30"),
                ("crit_multiplier", "80"),
                ("bonus_damage", "35"),
                ("spell_lifesteal", "15"),
            ],
        );
        expect(
            ItemId::SKULL_BASHER,
            &[
                ("bash_chance_melee", "25"),
                ("bash_chance_ranged", "10"),
                ("bonus_chance_damage", "100"),
                ("bash_duration", "1.2"),
                ("bash_cooldown", "2.3"),
                ("bonus_strength", "10"),
                ("bonus_damage", "30"),
            ],
        );
        expect(
            ItemId::BATTLE_FURY,
            &[
                ("cleave_damage_percent", "60"),
                ("cleave_damage_percent_creep", "40"),
                ("cleave_starting_width", "150"),
                ("cleave_ending_width", "360"),
                ("cleave_distance", "650"),
                ("quelling_bonus", "10"),
                ("quelling_bonus_ranged", "5"),
                ("bonus_damage", "50"),
                ("bonus_health_regen", "7.5"),
                ("bonus_mana_regen", "2.75"),
            ],
        );
        expect(
            ItemId::MAELSTROM,
            &[
                ("chain_chance", "25"),
                ("chain_damage", "110"),
                ("chain_strikes", "4"),
                ("chain_radius", "650"),
                ("chain_delay", "0.25"),
                ("chain_cooldown", "0.2"),
                ("bonus_damage", "25"),
                ("bonus_attack_speed", "25"),
            ],
        );
        expect(
            ItemId::SPECIALISTS_ARRAY,
            &[
                ("proc_chance", "30"),
                ("count", "2"),
                ("secondary_target_range_bonus", "150"),
                ("secondary_target_angle", "120"),
                ("base_proc_dmg", "20"),
                ("proc_dmg_pct", "75"),
                ("agility", "15"),
                ("damage", "20"),
            ],
        );
        expect(
            ItemId::VANGUARD,
            &[
                ("block_chance", "60"),
                ("block_damage_melee", "50"),
                ("block_damage_ranged", "25"),
                ("bonus_health", "250"),
                ("bonus_health_regen", "4.5"),
            ],
        );
        // A pierce's damage stays its own, and Orb of Venom's `damage` its poison's.
        expect(ItemId::JAVELIN, &[("bonus_chance_damage", "60")]);
        expect(ItemId::ORB_OF_VENOM, &[("damage", "12")]);
    }

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

    /// Infused Raindrops' tooltip prints its block, the least damage it answers to, its charges
    /// and its mana regeneration as the simulator holds them.
    #[test]
    fn the_raindrops_tooltip_reads_its_block_from_the_simulator() {
        let drops = item_def(ItemId::INFUSED_RAINDROP).expect("Infused Raindrops");
        let shown = |key| value(Subject::Item(drops), key);
        assert_eq!(shown("magic_damage_block"), Some(vec!["120".to_owned()]));
        assert_eq!(shown("min_damage"), Some(vec!["75".to_owned()]));
        assert_eq!(shown("initial_charges"), Some(vec!["6".to_owned()]));
        assert_eq!(shown("mana_regen"), Some(vec!["0.6".to_owned()]));
    }

    /// The Ghost Scepter's tooltip prints Ghost Form's duration and magic vulnerability, and the
    /// attributes it adds, as the simulator holds them.
    #[test]
    fn the_ghost_scepter_tooltip_reads_ghost_form_from_the_simulator() {
        let ghost = item_def(ItemId::GHOST_SCEPTER).expect("a Ghost Scepter");
        let shown = |key| value(Subject::Item(ghost), key);
        assert_eq!(shown("duration"), Some(vec!["4".to_owned()]));
        assert_eq!(
            shown("extra_spell_damage_percent"),
            Some(vec!["-30".to_owned()])
        );
        assert_eq!(shown("bonus_all_stats"), Some(vec!["5".to_owned()]));
    }

    /// The Chasm Stone's tooltip prints its AoE bonus as the simulator holds it.
    #[test]
    fn the_chasm_stone_tooltip_reads_its_aoe_bonus_from_the_simulator() {
        let stone = item_def(ItemId::CHASM_STONE).expect("a Chasm Stone");
        assert_eq!(
            value(Subject::Item(stone), "aoe_bonus"),
            Some(vec!["40".to_owned()])
        );
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
