use super::line::Fx;
use super::rules::Rule;
use crate::manifest::{MANIFEST, Manifest};

/// The `fx` lines of the manifest, read.
fn lines() -> Vec<Fx> {
    MANIFEST
        .lines()
        .map(|l| l.split('#').next().unwrap_or_default())
        .filter_map(|l| l.trim().strip_prefix("fx "))
        .map(|l| {
            let words: Vec<&str> = l.split_whitespace().collect();
            Fx::parse(&words).unwrap_or_else(|why| panic!("{words:?}: {why}"))
        })
        .collect()
}

/// Whether the manifest imports a `kind` ("particle", "sound") of the logical name.
fn listed(kind: &str, logical: &str) -> bool {
    MANIFEST.lines().any(|l| {
        let mut words = l.split_whitespace();
        words.next() == Some(kind) && words.next() == Some(logical)
    })
}

/// Every `fx` line of the manifest reads, names a subject the ids know (a unit a `unit` line
/// draws), and names particles and sounds the manifest imports: a typo in a line would otherwise
/// show as an effect that never plays.
#[test]
fn every_fx_line_reads_and_names_what_the_manifest_has() {
    let manifest = Manifest::load(std::path::Path::new("/nonexistent"));
    let lines = lines();
    assert!(!lines.is_empty());
    for fx in lines {
        if let Err(why) = Rule::resolve(&fx, &manifest) {
            panic!("{}: {why}", fx.subject);
        }
        if let Some(particle) = &fx.particle {
            assert!(listed("particle", particle), "{fx:?}: no such particle");
        }
        if let Some(sound) = &fx.sound {
            assert!(listed("sound", sound), "{fx:?}: no such sound");
        }
    }
}

/// The items and abilities of ids.txt that have no `fx` line, and why. Everything else of ids.txt
/// has one (its own, or one for the modifier of the effect it shows on the buff bar).
const NO_LINE: &[(&str, &str)] = &[
    // Items
    ("belt_of_strength", "a stat component: no effect in Dota"),
    ("blade_of_alacrity", "a stat component: no effect in Dota"),
    ("blades_of_attack", "a stat component: no effect in Dota"),
    ("blitz_knuckles", "a stat component: no effect in Dota"),
    ("boots", "a stat item: no effect in Dota"),
    ("boots_of_elves", "a stat component: no effect in Dota"),
    ("bracer", "a stat item: no effect in Dota"),
    ("broadsword", "a stat component: no effect in Dota"),
    (
        "butterfly",
        "Dota draws no effect for the passive; the evasion is a floating word, not a particle",
    ),
    ("chainmail", "a stat component: no effect in Dota"),
    ("circlet", "a stat component: no effect in Dota"),
    ("claymore", "a stat component: no effect in Dota"),
    ("demon_edge", "a stat component: no effect in Dota"),
    ("eagle", "a stat component: no effect in Dota"),
    ("energy_booster", "a stat component: no effect in Dota"),
    ("gauntlets", "a stat component: no effect in Dota"),
    ("gloves", "a stat component: no effect in Dota"),
    ("javelin", "a stat component: no effect in Dota"),
    ("mantle", "a stat component: no effect in Dota"),
    (
        "monkey_king_bar",
        "Dota draws no effect for the passive; the server does not tell a proc",
    ),
    ("null_talisman", "a stat item: no effect in Dota"),
    ("ogre_axe", "a stat component: no effect in Dota"),
    ("quarterstaff", "a stat component: no effect in Dota"),
    (
        "quelling_blade",
        "Dota shows the tree falling and no particle",
    ),
    ("recipe", "a recipe: no effect in Dota"),
    ("ring_of_protection", "a stat component: no effect in Dota"),
    ("ring_of_regen", "a stat component: no effect in Dota"),
    ("robe", "a stat component: no effect in Dota"),
    (
        "royale_with_cheese",
        "its Savory Shield is not simulated, and the game's code names no particle or sound for it",
    ),
    ("slippers", "a stat component: no effect in Dota"),
    ("sobi_mask", "a stat component: no effect in Dota"),
    ("staff_of_wizardry", "a stat component: no effect in Dota"),
    ("talisman_of_evasion", "a stat component: no effect in Dota"),
    ("tpscroll", "its channel is drawn by teleport.rs"),
    ("vitality_booster", "a stat component: no effect in Dota"),
    ("wind_lace", "a stat component: no effect in Dota"),
    ("wraith_band", "a stat item: no effect in Dota"),
    // Abilities
    ("ability_capture", "a channel: the clip is its effect"),
    (
        "alpha_wolf_command_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "alpha_wolf_critical_strike",
        "a passive Dota draws no particle for",
    ),
    (
        "ancient_rock_golem_weakening_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "big_thunder_lizard_wardrums_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "black_dragon_dragonhide_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "black_dragon_splash_attack",
        "a passive: the attack's own missile is drawn",
    ),
    (
        "black_drake_magic_amplification_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "centaur_khan_endurance_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "courier_burst",
        "the install has no particle for it: Dota tints the courier",
    ),
    ("courier_return_to_base", "an order: no effect in Dota"),
    (
        "courier_shield",
        "the install has no particle for it: Dota tints the courier",
    ),
    ("courier_take_stash_items", "an order: no effect in Dota"),
    ("courier_transfer_items", "an order: no effect in Dota"),
    ("creep_piercing", "a passive Dota draws no particle for"),
    (
        "enraged_wildkin_toughness_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "forest_troll_high_priest_heal_amp_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "forest_troll_high_priest_mana_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "frogmen_congregation_of_the_deep",
        "bota tells no summon of it",
    ),
    (
        "frogmen_riverborn_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "frogmen_tendrils_of_the_deep",
        "a root: Dota draws no particle for a plain root",
    ),
    (
        "frostbitten_golem_time_warp_aura",
        "an aura Dota draws no particle for",
    ),
    (
        "granite_golem_hp_aura",
        "an aura Dota draws no particle for",
    ),
    ("hill_troll_rally", "Dota draws no particle for it"),
    (
        "kobold_tunneler_prospecting",
        "a passive Dota draws no particle for",
    ),
    (
        "mud_golem_rock_destroy",
        "a passive Dota draws no particle for",
    ),
    ("mudgolem_cloak_aura", "an aura Dota draws no particle for"),
    ("neutral_upgrade", "a hidden modifier"),
    (
        "pudge_innate_graft_flesh",
        "its count particle is one fixed glyph, no digit fed by a control point",
    ),
    (
        "nevermore_dark_lord",
        "Dota draws no particle for it: the install has none",
    ),
    (
        "nevermore_necromastery",
        "its eyes and mouth are the hero's: the line of the unit",
    ),
    (
        "ogre_magi_frost_armor",
        "bota tells no effect for the armor",
    ),
    (
        "satyr_hellcaller_unholy_aura",
        "an aura Dota draws no particle for",
    ),
    ("spawnlord_aura", "an aura Dota draws no particle for"),
    (
        "tornado_tempest",
        "the tornado's own unit wears the particle",
    ),
    ("watcher_channel", "a channel: the clip is its effect"),
];

/// Every item and ability of ids.txt has an `fx` line, or is listed above with the reason Dota
/// shows nothing for it or bota tells nothing of it: a new one must come with a line or a reason,
/// and a listed one that has since got a line must leave the list.
#[test]
fn every_item_and_ability_has_an_fx_line_or_a_reason_for_none() {
    let manifest = Manifest::load(std::path::Path::new("/nonexistent"));
    let lines = lines();
    // The icon (and so the item or ability) of the effect a modifier is.
    let icon_of = |modifier: &str| {
        let id = manifest
            .ids
            .iter()
            .find(|(kind, _, name)| kind == "modifier" && name == modifier)?
            .1;
        manifest
            .ids
            .iter()
            .find(|(kind, i, _)| kind == "effect" && *i == id)
            .map(|(_, _, icon)| icon.clone())
    };
    let has_line = |kind: &str, key: &str| {
        let subject = format!("{kind}.{key}");
        lines.iter().any(|fx| {
            let (what, name) = fx.subject_parts();
            let wild = |pattern: &str| match pattern.strip_suffix('*') {
                Some(prefix) => key.starts_with(prefix),
                None => pattern == key,
            };
            fx.subject == subject
                || (what == kind && wild(name))
                || (what == "modifier"
                    && icon_of(name).is_some_and(|icon| icon == format!("icon.{kind}.{key}")))
        })
    };
    let mut unlisted = Vec::new();
    let mut stale = Vec::new();
    for (kind, id, key) in manifest
        .ids
        .iter()
        .filter(|(k, ..)| matches!(k.as_str(), "item" | "ability"))
    {
        let none = NO_LINE.iter().find(|(listed, _)| listed == key);
        match (has_line(kind, key), none) {
            (false, None) => unlisted.push(format!("{kind} {id} {key}")),
            (true, Some(_)) => stale.push(key.clone()),
            _ => {}
        }
    }
    assert!(
        unlisted.is_empty(),
        "no fx line and no reason: {unlisted:?}"
    );
    assert!(
        stale.is_empty(),
        "listed as having none but has a line: {stale:?}"
    );
    for (key, why) in NO_LINE {
        assert!(!why.is_empty());
        assert!(
            manifest
                .ids
                .iter()
                .any(|(k, _, name)| k != "effect" && k != "modifier" && name == key),
            "{key}: no such item or ability"
        );
    }
}
