//! Client fallback names by internal key. Installed localization takes precedence.

pub const FALLBACK: &[(&str, &str)] = &[
    ("ability_capture", "Capture"),
    ("ability_lamp_use", "Lamp Use"),
    ("alpha_wolf_command_aura", "Packleader's Aura"),
    ("alpha_wolf_critical_strike", "Critical Strike"),
    ("ancient_rock_golem_weakening_aura", "Weakening Aura"),
    ("berserker_troll_break", "Break"),
    ("big_thunder_lizard_frenzy", "Frenzy"),
    ("big_thunder_lizard_slam", "Slam"),
    ("big_thunder_lizard_wardrums_aura", "War Drums Aura"),
    ("black_dragon_dragonhide_aura", "Dragonhide Aura"),
    ("black_dragon_fireball", "Fireball"),
    ("black_dragon_splash_attack", "Splash Attack"),
    (
        "black_drake_magic_amplification_aura",
        "Magic Amplification Aura",
    ),
    ("bota_burning", "Burning"),
    ("bota_shrine_ready", "Shrine Ready"),
    ("bota_slowed", "Slowed"),
    ("bota_throw_grenade", "Throw Grenade"),
    ("centaur_khan_endurance_aura", "Swiftness Aura"),
    ("centaur_khan_war_stomp", "War Stomp"),
    ("courier_burst", "Burst"),
    ("courier_return_to_base", "Return"),
    ("courier_shield", "Shield"),
    ("courier_take_stash_items", "Take Stash"),
    ("courier_transfer_items", "Deliver"),
    ("creep_piercing", "Piercing"),
    ("dark_troll_warlord_ensnare", "Ensnare"),
    ("dark_troll_warlord_raise_dead", "Raise Dead"),
    ("enraged_wildkin_hurricane", "Hurricane"),
    ("enraged_wildkin_tornado", "Tornado"),
    ("enraged_wildkin_toughness_aura", "Toughness Aura"),
    ("fel_beast_haunt", "Vex"),
    ("forest_troll_high_priest_heal", "Heal"),
    (
        "forest_troll_high_priest_heal_amp_aura",
        "Heal Amplification Aura",
    ),
    ("forest_troll_high_priest_mana_aura", "Mana Aura"),
    ("frogmen_arm_of_the_deep", "Arm of the Deep"),
    (
        "frogmen_congregation_of_the_deep",
        "Congregations of the Deep",
    ),
    ("frogmen_riverborn_aura", "Riverborn Aura"),
    ("frogmen_tendrils_of_the_deep", "Tendrils of the Deep"),
    ("frogmen_water_bubble_large", "Water Bubble (Large)"),
    ("frogmen_water_bubble_medium", "Water Bubble (Medium)"),
    ("frogmen_water_bubble_small", "Water Bubble (Small)"),
    ("frostbitten_golem_time_warp_aura", "Time Warp Aura"),
    ("furbolg_enrage_attack_speed", "Death Throe: Rush"),
    ("furbolg_enrage_damage", "Death Throe: Power"),
    ("ghost_frost_attack", "Frost Attack"),
    ("giant_wolf_intimidate", "Intimidate"),
    ("gnoll_assassin_envenomed_weapon", "Envenomed Weapon"),
    ("granite_golem_hp_aura", "Granite Aura"),
    ("harpy_scout_take_off", "Take Off"),
    ("harpy_storm_chain_lightning", "Chain Lightning"),
    ("hill_troll_rally", "Rally"),
    ("ice_shaman_incendiary_bomb", "Icefire Bomb"),
    ("kobold_disarm", "Steal Weapon"),
    ("kobold_taskmaster_speed_aura", "Speed Aura"),
    ("kobold_tunneler_prospecting", "Prospecting Aura"),
    ("modifier_aegis_regen", "Recovery"),
    (
        "modifier_alpha_wolf_command_aura_bonus",
        "Packleader's Aura",
    ),
    ("modifier_ancient_rock_golem_weakening", "Weakening Aura"),
    ("modifier_backdoor_protection_active", "Backdoor Protection"),
    ("modifier_bashed", "Bashed"),
    ("modifier_big_thunder_lizard_frenzy", "Frenzy"),
    ("modifier_big_thunder_lizard_slam", "Slam"),
    (
        "modifier_big_thunder_lizard_wardrums_aura",
        "War Drums Aura",
    ),
    (
        "modifier_black_dragon_dragonhide_aura_bonus",
        "Dragonhide Aura",
    ),
    (
        "modifier_black_drake_magic_amplification",
        "Magic Amplification Aura",
    ),
    ("modifier_blight_stone_buff", "Lesser Corruption"),
    ("modifier_bottle_regeneration", "Bottle Regeneration"),
    ("modifier_break", "Broken"),
    (
        "modifier_centaur_khan_endurance_aura_bonus",
        "Swiftness Aura",
    ),
    ("modifier_clarity_potion", "Clarity"),
    ("modifier_courier_burst", "Speed Burst"),
    ("modifier_courier_flying", "Flying"),
    ("modifier_courier_shield", "Shield"),
    ("modifier_disarmed", "Disarmed"),
    (
        "modifier_enraged_wildkin_toughness_aura_bonus",
        "Toughness Aura",
    ),
    ("modifier_flagbearer_creep_aura_effect", "Inspiration"),
    ("modifier_flask_healing", "Healing Salve"),
    (
        "modifier_forest_troll_high_priest_mana_aura_bonus",
        "Mana Aura",
    ),
    ("modifier_fountain_aura_buff", "Fountain Rejuvenation"),
    (
        "modifier_fountain_fury_swipes_damage_increase",
        "Fountain Damage",
    ),
    ("modifier_fountain_glyph", "Glyph of Fortification"),
    (
        "modifier_fountain_invulnerability",
        "Fountain Invulnerability",
    ),
    ("modifier_fountain_passive", "Fountain Aura"),
    ("modifier_frogmen_riverborn_aura_bonus", "Riverborn Aura"),
    ("modifier_frogmen_water_bubble", "Water Bubble"),
    (
        "modifier_frostbitten_golem_time_warp_aura_bonus",
        "Time Warp Aura",
    ),
    ("modifier_furbolg_enrage_attack_speed", "Death Throe: Rush"),
    ("modifier_furbolg_enrage_damage", "Death Throe: Power"),
    ("modifier_ghost_frost_attack_slow", "Frost Attack"),
    ("modifier_ghost_state", "Ghost Form"),
    ("modifier_giant_wolf_intimidate", "Intimidate"),
    (
        "modifier_gnoll_assassin_envenomed_weapon_poison",
        "Envenomed Weapon",
    ),
    ("modifier_granite_golem_hp_aura_bonus", "Granite Aura"),
    ("modifier_hill_troll_rally", "Rally"),
    ("modifier_illusion", "Illusion"),
    ("modifier_item_blood_grenade_debuff", "Blood Grenade"),
    ("modifier_item_orb_of_venom_slow", "Poison Attack"),
    ("modifier_item_phase_boots_active", "Phase Boots"),
    ("modifier_kill", "Time Left"),
    ("modifier_knockback", "Knocked Back"),
    ("modifier_kobold_disarm", "Steal Weapon"),
    ("modifier_kobold_taskmaster_speed_aura_bonus", "Speed Aura"),
    ("modifier_lamp_off", "Deactivated"),
    ("modifier_lamp_on", "Granted Vision"),
    ("modifier_mud_golem_rock_destroy", "Shard Split"),
    ("modifier_mudgolem_cloak_aura_bonus", "Cloak Aura"),
    ("modifier_neutral_upgrade", "Neutral Upgrade"),
    ("modifier_nevermore_feast_of_souls_linger", "Feast of Souls"),
    ("modifier_nevermore_frenzy", "Feast of Souls"),
    ("modifier_nevermore_necromastery", "Necromastery"),
    ("modifier_nevermore_presence", "Presence of the Dark Lord"),
    ("modifier_nevermore_requiem_fear", "Requiem of Souls"),
    ("modifier_nevermore_requiem_slow", "Requiem of Souls"),
    ("modifier_nevermore_shadowraze_debuff", "Shadowraze"),
    ("modifier_orb_of_frost_debuff", "Frost"),
    (
        "modifier_polar_furbolg_ursa_warrior_thunder_clap",
        "Thunder Clap",
    ),
    ("modifier_pudge_dismember", "Dismember"),
    ("modifier_pudge_flesh_heap_block", "Meat Shield"),
    ("modifier_pudge_innate_graft_flesh", "Flesh Heap"),
    (
        "modifier_pudge_meat_hook_followthrough",
        "Meat Hook Followthrough",
    ),
    ("modifier_pudge_rot", "Rot"),
    ("modifier_rooted", "Rooted"),
    ("modifier_roshan_devotion", "Roshan's Armor"),
    ("modifier_roshan_devotion_aura", "Devotion Aura"),
    ("modifier_roshan_grab", "Grabbed"),
    ("modifier_roshan_grab_thrown", "Thrown"),
    ("modifier_roshan_inherent_buffs", "Strength of the Immortal"),
    ("modifier_roshan_moving", "On the Move"),
    (
        "modifier_roshan_moving_knockback_timeout",
        "Collision Cooldown",
    ),
    ("modifier_roshan_revengeroar", "Roar of Retribution"),
    ("modifier_roshan_revengeroar_aura", "Roar Protection"),
    ("modifier_roshan_revengeroar_disarm", "Disarmed"),
    ("modifier_roshan_revengeroar_windup", "Roaring"),
    ("modifier_roshan_slam", "Slam"),
    ("modifier_roshan_spell_block", "Spell Block"),
    ("modifier_rune_arcane", "Arcane"),
    ("modifier_rune_doubledamage", "Amplify Damage"),
    ("modifier_rune_haste", "Haste"),
    ("modifier_rune_invis", "Invisible"),
    ("modifier_rune_regen", "Regeneration"),
    ("modifier_rune_shield", "Shielded"),
    ("modifier_satyr_hellcaller_unholy_aura_bonus", "Unholy Aura"),
    ("modifier_satyr_trickster_purge", "Purge"),
    ("modifier_silence", "Silenced"),
    ("modifier_spawnlord_aura_bonus", "Prowler Aura"),
    ("modifier_spawnlord_master_freeze_root", "Petrify"),
    ("modifier_spawnlord_master_stomp", "Desecrate"),
    ("modifier_stunned", "Stunned"),
    ("modifier_tornado_tempest_debuff", "Tempest"),
    ("modifier_tower_armor_bonus", "Tower Armor"),
    ("modifier_twin_gate_fx", "Warp"),
    ("modifier_watch_tower_invulnerable", "Capturing"),
    ("mud_golem_hurl_boulder", "Hurl Boulder"),
    ("mud_golem_rock_destroy", "Shard Split"),
    ("mudgolem_cloak_aura", "Cloak Aura"),
    ("neutral_upgrade", "Neutral Upgrade"),
    ("nevermore_dark_lord", "Presence of the Dark Lord"),
    ("nevermore_frenzy", "Feast of Souls"),
    ("nevermore_necromastery", "Necromastery"),
    ("nevermore_requiem", "Requiem of Souls"),
    ("nevermore_shadowraze1", "Shadowraze (Near)"),
    ("nevermore_shadowraze2", "Shadowraze (Mid)"),
    ("nevermore_shadowraze3", "Shadowraze (Far)"),
    ("npc_dota_badguys_fillers", "Building"),
    ("npc_dota_badguys_fort", "Dire's Ancient"),
    ("npc_dota_badguys_healers", "Fountain"),
    ("npc_dota_badguys_melee_rax_bot", "Melee Barracks"),
    ("npc_dota_badguys_melee_rax_mid", "Melee Barracks"),
    ("npc_dota_badguys_melee_rax_top", "Melee Barracks"),
    ("npc_dota_badguys_range_rax_bot", "Ranged Barracks"),
    ("npc_dota_badguys_range_rax_mid", "Ranged Barracks"),
    ("npc_dota_badguys_range_rax_top", "Ranged Barracks"),
    ("npc_dota_badguys_siege", "Siege Creep"),
    ("npc_dota_badguys_siege_upgraded", "Siege Creep"),
    ("npc_dota_badguys_siege_upgraded_mega", "Siege Creep"),
    ("npc_dota_badguys_tower1_bot", "Tower"),
    ("npc_dota_badguys_tower1_mid", "Tower"),
    ("npc_dota_badguys_tower1_top", "Tower"),
    ("npc_dota_badguys_tower2_bot", "Tower"),
    ("npc_dota_badguys_tower2_mid", "Tower"),
    ("npc_dota_badguys_tower2_top", "Tower"),
    ("npc_dota_badguys_tower3_bot", "Tower"),
    ("npc_dota_badguys_tower3_mid", "Tower"),
    ("npc_dota_badguys_tower3_top", "Tower"),
    ("npc_dota_badguys_tower4", "Tower"),
    ("npc_dota_courier", "Courier"),
    ("npc_dota_creep_badguys_flagbearer", "Flagbearer Creep"),
    (
        "npc_dota_creep_badguys_flagbearer_upgraded",
        "Flagbearer Creep",
    ),
    (
        "npc_dota_creep_badguys_flagbearer_upgraded_mega",
        "Flagbearer Creep",
    ),
    ("npc_dota_creep_badguys_melee", "Melee Creep"),
    ("npc_dota_creep_badguys_melee_upgraded", "Melee Creep"),
    ("npc_dota_creep_badguys_melee_upgraded_mega", "Melee Creep"),
    ("npc_dota_creep_badguys_ranged", "Ranged Creep"),
    ("npc_dota_creep_badguys_ranged_upgraded", "Ranged Creep"),
    (
        "npc_dota_creep_badguys_ranged_upgraded_mega",
        "Ranged Creep",
    ),
    ("npc_dota_creep_goodguys_flagbearer", "Flagbearer Creep"),
    (
        "npc_dota_creep_goodguys_flagbearer_upgraded",
        "Flagbearer Creep",
    ),
    (
        "npc_dota_creep_goodguys_flagbearer_upgraded_mega",
        "Flagbearer Creep",
    ),
    ("npc_dota_creep_goodguys_melee", "Melee Creep"),
    ("npc_dota_creep_goodguys_melee_upgraded", "Melee Creep"),
    ("npc_dota_creep_goodguys_melee_upgraded_mega", "Melee Creep"),
    ("npc_dota_creep_goodguys_ranged", "Ranged Creep"),
    ("npc_dota_creep_goodguys_ranged_upgraded", "Ranged Creep"),
    (
        "npc_dota_creep_goodguys_ranged_upgraded_mega",
        "Ranged Creep",
    ),
    ("npc_dota_creep_neutral", "Neutral Creep"),
    (
        "npc_dota_dark_troll_warlord_skeleton_warrior",
        "Skeleton Warrior",
    ),
    ("npc_dota_enraged_wildkin_tornado", "Tornado"),
    ("npc_dota_flying_courier", "Courier"),
    ("npc_dota_goodguys_fillers", "Building"),
    ("npc_dota_goodguys_fort", "Radiant's Ancient"),
    ("npc_dota_goodguys_healers", "Fountain"),
    ("npc_dota_goodguys_melee_rax_bot", "Melee Barracks"),
    ("npc_dota_goodguys_melee_rax_mid", "Melee Barracks"),
    ("npc_dota_goodguys_melee_rax_top", "Melee Barracks"),
    ("npc_dota_goodguys_range_rax_bot", "Ranged Barracks"),
    ("npc_dota_goodguys_range_rax_mid", "Ranged Barracks"),
    ("npc_dota_goodguys_range_rax_top", "Ranged Barracks"),
    ("npc_dota_goodguys_siege", "Siege Creep"),
    ("npc_dota_goodguys_siege_upgraded", "Siege Creep"),
    ("npc_dota_goodguys_siege_upgraded_mega", "Siege Creep"),
    ("npc_dota_goodguys_tower1_bot", "Tower"),
    ("npc_dota_goodguys_tower1_mid", "Tower"),
    ("npc_dota_goodguys_tower1_top", "Tower"),
    ("npc_dota_goodguys_tower2_bot", "Tower"),
    ("npc_dota_goodguys_tower2_mid", "Tower"),
    ("npc_dota_goodguys_tower2_top", "Tower"),
    ("npc_dota_goodguys_tower3_bot", "Tower"),
    ("npc_dota_goodguys_tower3_mid", "Tower"),
    ("npc_dota_goodguys_tower3_top", "Tower"),
    ("npc_dota_goodguys_tower4", "Tower"),
    ("npc_dota_hero_base", "Hero"),
    ("npc_dota_hero_nevermore", "Shadow Fiend"),
    ("npc_dota_hero_pudge", "Pudge"),
    ("npc_dota_lantern", "Watcher"),
    ("npc_dota_lotus_pool", "Lotus Pool"),
    ("npc_dota_neutral_alpha_wolf", "Alpha Wolf"),
    ("npc_dota_neutral_ancient_frog", "Ancient Croaker"),
    ("npc_dota_neutral_ancient_frog_mage", "Ancient Marshmage"),
    ("npc_dota_neutral_big_thunder_lizard", "Ancient Thunderhide"),
    ("npc_dota_neutral_black_dragon", "Ancient Black Dragon"),
    ("npc_dota_neutral_black_drake", "Ancient Black Drake"),
    ("npc_dota_neutral_centaur_khan", "Centaur Conqueror"),
    ("npc_dota_neutral_centaur_outrunner", "Centaur Courser"),
    ("npc_dota_neutral_dark_troll", "Hill Troll"),
    ("npc_dota_neutral_dark_troll_warlord", "Dark Troll Summoner"),
    ("npc_dota_neutral_enraged_wildkin", "Wildwing Ripper"),
    ("npc_dota_neutral_fel_beast", "Fell Spirit"),
    (
        "npc_dota_neutral_forest_troll_berserker",
        "Hill Troll Berserker",
    ),
    (
        "npc_dota_neutral_forest_troll_high_priest",
        "Hill Troll Priest",
    ),
    ("npc_dota_neutral_froglet", "Boglet"),
    ("npc_dota_neutral_froglet_mage", "Marshmage Apprentice"),
    (
        "npc_dota_neutral_frostbitten_golem",
        "Ancient Frostbitten Golem",
    ),
    ("npc_dota_neutral_ghost", "Ghost"),
    ("npc_dota_neutral_giant_wolf", "Giant Wolf"),
    ("npc_dota_neutral_gnoll_assassin", "Vhoul Assassin"),
    ("npc_dota_neutral_granite_golem", "Ancient Granite Golem"),
    ("npc_dota_neutral_grown_frog", "Croaker"),
    ("npc_dota_neutral_grown_frog_mage", "Marshmage"),
    ("npc_dota_neutral_harpy_scout", "Harpy Scout"),
    ("npc_dota_neutral_harpy_storm", "Harpy Stormcrafter"),
    ("npc_dota_neutral_ice_shaman", "Ancient Ice Shaman"),
    ("npc_dota_neutral_kobold", "Kobold"),
    ("npc_dota_neutral_kobold_taskmaster", "Kobold Foreman"),
    ("npc_dota_neutral_kobold_tunneler", "Kobold Soldier"),
    ("npc_dota_neutral_mud_golem", "Mud Golem"),
    ("npc_dota_neutral_mud_golem_split", "Shard Golem"),
    ("npc_dota_neutral_ogre_magi", "Ogre Frostmage"),
    ("npc_dota_neutral_ogre_mauler", "Ogre Bruiser"),
    ("npc_dota_neutral_polar_furbolg_champion", "Hellbear"),
    (
        "npc_dota_neutral_polar_furbolg_ursa_warrior",
        "Hellbear Smasher",
    ),
    (
        "npc_dota_neutral_prowler_acolyte",
        "Ancient Prowler Acolyte",
    ),
    ("npc_dota_neutral_prowler_shaman", "Ancient Prowler Shaman"),
    ("npc_dota_neutral_rock_golem", "Ancient Rock Golem"),
    ("npc_dota_neutral_satyr_hellcaller", "Satyr Tormenter"),
    ("npc_dota_neutral_satyr_soulstealer", "Satyr Mindstealer"),
    ("npc_dota_neutral_satyr_trickster", "Satyr Banisher"),
    (
        "npc_dota_neutral_small_thunder_lizard",
        "Ancient Rumblehide",
    ),
    ("npc_dota_neutral_tadpole", "Pollywog"),
    ("npc_dota_neutral_warpine_raider", "Warpine Raider"),
    ("npc_dota_neutral_wildkin", "Wildwing"),
    ("npc_dota_observer_wards", "Observer Ward"),
    ("npc_dota_roshan", "Roshan"),
    ("npc_dota_sentry_wards", "Sentry Ward"),
    ("npc_dota_unit_roshans_banner", "Roshan's Banner"),
    ("npc_dota_unit_twin_gate", "Twin Gate"),
    ("npc_dota_watch_tower", "Outpost"),
    ("npc_dota_xp_fountain", "Shrine of Wisdom"),
    ("ogre_bruiser_ogre_smash", "Ogre Smash!"),
    ("ogre_magi_frost_armor", "Ice Armor"),
    ("polar_furbolg_ursa_warrior_thunder_clap", "Thunder Clap"),
    ("pudge_dismember", "Dismember"),
    ("pudge_flesh_heap", "Meat Shield"),
    ("pudge_innate_graft_flesh", "Flesh Heap"),
    ("pudge_meat_hook", "Meat Hook"),
    ("pudge_rot", "Rot"),
    ("roshan_bash", "Bash"),
    ("roshan_devotion", "Devotion"),
    ("roshan_grab_and_throw", "Throw"),
    ("roshan_inherent_buffs", "StrengthOfTheImmortal"),
    ("roshan_revengeroar", "RoarOfRetribution"),
    ("roshan_slam", "RoshanSlam"),
    ("roshan_spell_block", "SpellBlock"),
    ("satyr_hellcaller_shockwave", "Shockwave"),
    ("satyr_hellcaller_unholy_aura", "Unholy Aura"),
    ("satyr_soulstealer_mana_burn", "Mana Burn"),
    ("satyr_trickster_purge", "Purge"),
    ("spawnlord_aura", "Prowler Aura"),
    ("spawnlord_master_freeze", "Petrify"),
    ("spawnlord_master_stomp", "Desecrate"),
    ("tornado_tempest", "Tempest"),
    ("twin_gate_portal_warp", "Warp"),
    ("warpine_raider_seed_shot", "Seed Shot"),
    ("modifier_item_headdress_aura", "Regeneration Aura"),
    ("modifier_item_ring_of_basilius_aura", "Mana Aura"),
    ("modifier_item_buckler_effect", "Armor Aura"),
    ("modifier_item_mekansm_aura", "Mekansm Aura"),
    ("modifier_item_mekansm_noheal", "Mekansm Recovery"),
    ("modifier_item_ancient_janggo_aura", "Swiftness Aura"),
    ("modifier_item_ancient_janggo_active", "Endurance"),
    ("modifier_item_pipe_aura", "Insight Aura"),
    ("modifier_item_pipe_barrier", "Magic Barrier"),
    ("modifier_item_pipe_debuff", "Barrier Recovery"),
    ("modifier_item_guardian_greaves_aura", "Guardian Aura"),
    ("modifier_item_guardian_greaves_bonus", "Low Health Boost"),
    ("modifier_item_assault_positive_armor", "Assault Aura"),
    ("modifier_item_assault_negative_armor", "Armor Reduction"),
    ("modifier_item_vladmir_aura", "Vladmir's Aura"),
    ("modifier_item_radiance_debuff", "Radiance Burn"),
    ("modifier_item_shivas_guard_aura", "Freezing Aura"),
    ("modifier_item_shivas_guard_blast", "Arctic Blast"),
    ("modifier_item_boots_of_bearing_aura", "Swiftness Aura"),
    ("modifier_item_boots_of_bearing_active", "Endurance"),
    (
        "modifier_item_boots_of_bearing_active_bonus",
        "Slow Immunity",
    ),
    ("bota_spell_immunity", "Spell Immunity"),
    ("modifier_item_crellas_crozier_debuff", "Putrefaction Aura"),
    ("modifier_item_crellas_crozier_active", "Rite of Rumusque"),
    (
        "modifier_item_crellas_crozier_movespeed_debuff",
        "Movement Slowed",
    ),
    (
        "modifier_item_crellas_crozier_movespeed_buff",
        "Stolen Speed",
    ),
];

pub fn fallback(key: &str) -> Option<&'static str> {
    FALLBACK
        .iter()
        .find_map(|(known, name)| (*known == key).then_some(*name))
}

pub fn plain(key: &str) -> &str {
    fallback(key)
        .or_else(|| crate::item_names::fallback(key))
        .unwrap_or(key)
}

pub fn resolve<'a>(texts: Option<&'a crate::dota_text::Texts>, key: &'a str) -> &'a str {
    texts.map_or_else(|| plain(key), |texts| texts.name(key))
}

/// A category without a known unit uses a representative NPC key.
pub fn kind_key(kind: bota_proto::UnitKind) -> &'static str {
    use bota_proto::UnitKind;
    match kind {
        UnitKind::Hero => "npc_dota_hero_base",
        UnitKind::CreepMelee => "npc_dota_creep_goodguys_melee",
        UnitKind::CreepFlagbearer => "npc_dota_creep_goodguys_flagbearer",
        UnitKind::CreepRanged => "npc_dota_creep_goodguys_ranged",
        UnitKind::CreepSiege => "npc_dota_goodguys_siege",
        UnitKind::CreepNeutral => "npc_dota_creep_neutral",
        UnitKind::Tower => "npc_dota_goodguys_tower1_mid",
        UnitKind::Ancient => "npc_dota_goodguys_fort",
        UnitKind::Barracks => "npc_dota_goodguys_melee_rax_mid",
        UnitKind::Fountain => "npc_dota_goodguys_healers",
        UnitKind::Ward => "npc_dota_observer_wards",
        UnitKind::Courier => "npc_dota_courier",
        UnitKind::Filler => "npc_dota_goodguys_fillers",
        UnitKind::Outpost => "npc_dota_watch_tower",
        UnitKind::Watcher => "npc_dota_lantern",
        UnitKind::Shrine => "npc_dota_xp_fountain",
        UnitKind::LotusPool => "npc_dota_lotus_pool",
        UnitKind::Roshan => "npc_dota_roshan",
        UnitKind::Banner => "npc_dota_unit_roshans_banner",
        UnitKind::TwinGate => "npc_dota_unit_twin_gate",
    }
}

/// A building's NPC key, from the facts the map or its fall supplies.
pub fn structure_key(
    kind: bota_proto::UnitKind,
    team: bota_proto::Team,
    lane: Option<u8>,
    tier: Option<u8>,
    ranged: Option<bool>,
) -> String {
    use bota_proto::{Team, UnitKind};
    let faction = if team == Team::Dire {
        "badguys"
    } else {
        "goodguys"
    };
    let lane = match lane {
        Some(bota_core::rules::LANE_TOP) => "top",
        Some(bota_core::rules::LANE_BOT) => "bot",
        _ => "mid",
    };
    match kind {
        UnitKind::Ancient => format!("npc_dota_{faction}_fort"),
        UnitKind::Fountain => format!("npc_dota_{faction}_healers"),
        UnitKind::Filler => format!("npc_dota_{faction}_fillers"),
        UnitKind::Barracks => format!(
            "npc_dota_{faction}_{}_rax_{lane}",
            if ranged == Some(true) {
                "range"
            } else {
                "melee"
            }
        ),
        UnitKind::Tower => {
            let tier = tier.unwrap_or(1);
            let suffix = if tier == 4 {
                String::new()
            } else {
                format!("_{lane}")
            };
            format!("npc_dota_{faction}_tower{tier}{suffix}")
        }
        _ => kind_key(kind).to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_live_definition_has_one_client_fallback_by_its_internal_key() {
        use bota_proto::IdSpace;
        for space in [
            IdSpace::Hero,
            IdSpace::Ability,
            IdSpace::Neutral,
            IdSpace::Effect,
        ] {
            for entry in bota_proto::registered(space).filter(|entry| !entry.retired) {
                assert!(
                    fallback(entry.name).is_some(),
                    "{space:?} {} has no client name",
                    entry.name
                );
            }
        }
        for definition in bota_core::UNIT_KINDS {
            let key = kind_key(definition.kind);
            assert!(
                fallback(key).is_some(),
                "{:?} produces {key} without a client name",
                definition.kind
            );
        }
        for (at, (key, name)) in FALLBACK.iter().enumerate() {
            assert!(!name.trim().is_empty(), "{key} has an empty name");
            assert!(
                FALLBACK[..at].iter().all(|(other, _)| other != key),
                "{key} is repeated"
            );
        }
    }

    #[test]
    fn installed_names_win_and_empty_or_missing_names_use_fallbacks_then_keys() {
        let root = crate::test_support::scratch_path("name_resolution");
        std::fs::create_dir_all(root.join("text")).expect("fixture directory");
        std::fs::write(
            root.join("text/labels.json"),
            r#"{
            "npc_dota_hero_nevermore:n":"Installed Hero",
            "npc_dota_neutral_kobold":"Installed Neutral",
            "npc_dota_courier":"   "
        }"#,
        )
        .expect("fixture labels");
        std::fs::write(
            root.join("text/tooltips.json"),
            r#"{
            "pudge_meat_hook":{"name":"Installed Hook"},
            "nevermore_shadowraze1":{"name":""}
        }"#,
        )
        .expect("fixture tooltips");
        let names = crate::dota_text::Texts::load(&root);
        assert_eq!(names.name("npc_dota_hero_nevermore"), "Installed Hero");
        assert_eq!(names.name("npc_dota_neutral_kobold"), "Installed Neutral");
        assert_eq!(names.name("pudge_meat_hook"), "Installed Hook");
        assert_eq!(names.name("nevermore_shadowraze1"), "Shadowraze (Near)");
        assert_eq!(names.name("npc_dota_courier"), "Courier");
        assert_eq!(names.name("npc_dota_future_unit"), "npc_dota_future_unit");
        assert_eq!(names.name("future_ability"), "future_ability");
    }
}
