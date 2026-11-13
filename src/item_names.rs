//! The words dbota names an item by when the player's install gives none: Dota's English names,
//! keyed by the item's internal Dota key (what bota knows it by). The install's own text comes first
//! ([`crate::tip_content::Lookup::item_name`]); this table is the fallback, and an item it misses
//! too is shown by its key.

/// Each item's English name, by its Dota key, in the order of bota's id registry.
pub const FALLBACK: &[(&str, &str)] = &[
    ("item_boots", "Boots of Speed"),
    ("item_clarity", "Clarity"),
    ("item_flask", "Healing Salve"),
    ("item_branches", "Iron Branch"),
    ("item_ward_observer", "Observer Ward"),
    ("item_quelling_blade", "Quelling Blade"),
    ("item_ward_sentry", "Sentry Ward"),
    ("item_tango", "Tango"),
    ("item_tpscroll", "Town Portal Scroll"),
    ("item_circlet", "Circlet"),
    ("item_gauntlets", "Gauntlets of Strength"),
    ("item_slippers", "Slippers of Agility"),
    ("item_mantle", "Mantle of Intelligence"),
    ("item_belt_of_strength", "Belt of Strength"),
    ("item_boots_of_elves", "Band of Elvenskin"),
    ("item_robe", "Robe of the Magi"),
    ("item_ogre_axe", "Ogre Axe"),
    ("item_blade_of_alacrity", "Blade of Alacrity"),
    ("item_staff_of_wizardry", "Staff of Wizardry"),
    ("item_gloves", "Gloves of Haste"),
    ("item_blades_of_attack", "Blades of Attack"),
    ("item_broadsword", "Broadsword"),
    ("item_quarterstaff", "Quarterstaff"),
    ("item_ring_of_protection", "Ring of Protection"),
    ("item_chainmail", "Chainmail"),
    ("item_ring_of_regen", "Ring of Regen"),
    ("item_sobi_mask", "Sage's Mask"),
    ("item_vitality_booster", "Vitality Booster"),
    ("item_energy_booster", "Energy Booster"),
    ("item_power_treads", "Power Treads"),
    ("item_phase_boots", "Phase Boots"),
    ("item_blink", "Blink Dagger"),
    ("item_bracer", "Bracer"),
    ("item_wraith_band", "Wraith Band"),
    ("item_null_talisman", "Null Talisman"),
    ("item_magic_stick", "Magic Stick"),
    ("item_magic_wand", "Magic Wand"),
    ("item_recipe_phase_boots", "Recipe: Phase Boots"),
    ("item_recipe_bracer", "Recipe: Bracer"),
    ("item_recipe_wraith_band", "Recipe: Wraith Band"),
    ("item_recipe_null_talisman", "Recipe: Null Talisman"),
    ("item_recipe_magic_wand", "Recipe: Magic Wand"),
    ("item_enchanted_mango", "Enchanted Mango"),
    ("item_eagle", "Eaglesong"),
    ("item_claymore", "Claymore"),
    ("item_talisman_of_evasion", "Talisman of Evasion"),
    ("item_butterfly", "Butterfly"),
    ("item_javelin", "Javelin"),
    ("item_demon_edge", "Demon Edge"),
    ("item_blitz_knuckles", "Blitz Knuckles"),
    ("item_recipe_monkey_king_bar", "Recipe: Monkey King Bar"),
    ("item_monkey_king_bar", "Monkey King Bar"),
    ("item_wind_lace", "Wind Lace"),
    ("item_blood_grenade", "Blood Grenade"),
    ("item_ward_dispenser", "Ward Dispenser"),
    ("item_tango_single", "Tango (Shared)"),
    ("item_bottle", "Bottle"),
    ("item_famango", "Healing Lotus"),
    ("item_great_famango", "Great Healing Lotus"),
    ("item_greater_famango", "Greater Healing Lotus"),
    ("item_cheese", "Cheese"),
    ("item_royale_with_cheese", "Block of Cheese"),
    ("item_crown", "Crown"),
    ("item_diadem", "Diadem"),
    ("item_splintmail", "Splintmail"),
    ("item_helm_of_iron_will", "Helm of Iron Will"),
    ("item_mithril_hammer", "Mithril Hammer"),
    ("item_fluffy_hat", "Fluffy Hat"),
    ("item_wizard_hat", "Wizard Hat"),
    ("item_ring_of_health", "Ring of Health"),
    ("item_void_stone", "Void Stone"),
    ("item_platemail", "Platemail"),
    ("item_ring_of_tarrasque", "Ring of Tarrasque"),
    ("item_tiara_of_selemene", "Tiara of Selemene"),
    ("item_hyperstone", "Hyperstone"),
    ("item_point_booster", "Point Booster"),
    ("item_relic", "Sacred Relic"),
    ("item_reaver", "Reaver"),
    ("item_mystic_staff", "Mystic Staff"),
    ("item_ultimate_orb", "Ultimate Orb"),
    ("item_shawl", "Shawl"),
    ("item_cloak", "Cloak"),
    ("item_lifesteal", "Morbid Mask"),
    ("item_voodoo_mask", "Voodoo Mask"),
    ("item_faerie_fire", "Faerie Fire"),
    ("item_blight_stone", "Orb of Blight"),
    ("item_orb_of_venom", "Orb of Venom"),
    ("item_orb_of_frost", "Orb of Frost"),
    ("item_aegis", "Aegis of the Immortal"),
    ("item_refresher_shard", "Refresher Shard"),
    ("item_roshans_banner", "Roshan's Banner"),
    ("item_infused_raindrop", "Infused Raindrops"),
    ("item_ghost", "Ghost Scepter"),
    ("item_chasm_stone", "Chasm Stone"),
    ("item_oblivion_staff", "Oblivion Staff"),
    ("item_pers", "Perseverance"),
    ("item_soul_booster", "Soul Booster"),
    ("item_falcon_blade", "Falcon Blade"),
    ("item_recipe_falcon_blade", "Recipe: Falcon Blade"),
    ("item_yasha", "Yasha"),
    ("item_recipe_yasha", "Recipe: Yasha"),
    ("item_sange", "Sange"),
    ("item_recipe_sange", "Recipe: Sange"),
    ("item_kaya", "Kaya"),
    ("item_recipe_kaya", "Recipe: Kaya"),
    ("item_sange_and_yasha", "Sange and Yasha"),
    ("item_kaya_and_sange", "Kaya and Sange"),
    ("item_yasha_and_kaya", "Yasha and Kaya"),
    ("item_aether_lens", "Aether Lens"),
    ("item_recipe_aether_lens", "Recipe: Aether Lens"),
    ("item_octarine_core", "Octarine Core"),
    ("item_recipe_octarine_core", "Recipe: Octarine Core"),
    ("item_dragon_lance", "Dragon Lance"),
    ("item_recipe_dragon_lance", "Recipe: Dragon Lance"),
    ("item_heart", "Heart of Tarrasque"),
    ("item_recipe_heart", "Recipe: Heart of Tarrasque"),
];

/// The English name of the item whose Dota key is `key`, if the table has one.
pub fn fallback(key: &str) -> Option<&'static str> {
    FALLBACK
        .iter()
        .find(|(known, _)| *known == key)
        .map(|(_, name)| *name)
}

/// What the client calls an item when the install's text is missing: its English name, or its
/// key.
pub fn plain(item: bota_proto::ItemId) -> &'static str {
    bota_core::item_def(item).map_or("an item", |def| fallback(def.key).unwrap_or(def.key))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every live item of bota's id registry has a name the client can show without the install.
    #[test]
    fn every_live_item_of_the_registry_has_a_fallback_name() {
        for entry in bota_proto::registered(bota_proto::IdSpace::Item).filter(|e| !e.retired) {
            assert!(
                fallback(entry.name).is_some(),
                "item {} ({}) has no fallback name",
                entry.id,
                entry.name
            );
        }
    }

    /// The table names each key once.
    #[test]
    fn the_fallback_table_names_each_key_once() {
        for (at, (key, _)) in FALLBACK.iter().enumerate() {
            assert!(
                FALLBACK[..at].iter().all(|(other, _)| other != key),
                "{key} twice"
            );
        }
    }
}
