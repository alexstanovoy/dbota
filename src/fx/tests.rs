use super::line::Fx;
use super::rules::Rule;
use crate::manifest::{MANIFEST, Manifest};

#[test]
fn model_radius_is_only_accepted_for_worn_effects() {
    for (trigger, at) in [
        ("use", "caster"),
        ("cast", "caster"),
        ("hit", "unit"),
        ("land", "point"),
        ("died", "unit"),
        ("spawn", "point"),
        ("activate", "caster"),
        ("store", "caster"),
        ("pluck", "unit"),
        ("held", "unit"),
        ("casting", "unit"),
        ("flies", "flight"),
    ] {
        let name = format!("ability.example.{trigger}");
        let at = format!("at={at}");
        let result = Fx::parse(&[&name, &at, "particle=example", "cp1=model_radius"]);
        assert_eq!(
            result.expect_err(trigger),
            "model_radius requires a worn line"
        );
    }
    assert!(
        Fx::parse(&[
            "ability.example.worn",
            "at=unit",
            "particle=example",
            "cp1=model_radius"
        ])
        .is_ok()
    );
}

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

/// Whether an item of bota's catalogue is used for nothing and procs nothing: it carries stats
/// alone (a recipe or a retired id nothing at all), and Dota draws no effect for it.
fn carries_stats_alone(id: u16) -> bool {
    bota_core::item_def(bota_proto::ItemId(id)).is_none_or(|def| {
        let procs_nothing = |carried: &bota_core::Carried| {
            carried.evasion == bota_core::Odds::NEVER
                && carried.pierce == bota_core::Odds::NEVER
                && carried.lifesteal_pct == 0
                && carried.spell_lifesteal_pct == 0
                && carried.strikes.is_empty()
                && carried.damage_block.is_none()
        };
        def.aim.is_none()
            && def.on_attack.is_none()
            && def.reincarnation.is_none()
            && def.expiry_recovery.is_none()
            && def.magic_block.is_none()
            && matches!(def.effect, bota_core::UseSpec::None)
            && procs_nothing(&def.carried)
            && def.ranged.as_ref().is_none_or(procs_nothing)
    })
}

/// The items and abilities of ids.txt that have no `fx` line, and why. Everything else of ids.txt
/// has one (its own, or one for the modifier of the effect it shows on the buff bar).
const NO_LINE: &[(&str, &str)] = &[
    (
        "aegis",
        "the passive reincarnation is represented by the hero's death and rise, with an item lifetime indicator",
    ),
    (
        "roshan_inherent_buffs",
        "permanent intrinsic properties, with no persistent particle",
    ),
    (
        "roshan_devotion",
        "armor and slow resistance, with no persistent particle",
    ),
    (
        "roshan_spell_block",
        "the spent spell-block cooldown is shown in the ability panel",
    ),
    (
        "roshans_banner",
        "the placed unit owns its ambient effect, ending it when the unit is removed",
    ),
    // Items
    (
        "bfury",
        "its Cleave's ring plays once a swing at the attacker, which the server tells only as a blow on each unit \
         cleaved (follow-up: typed attack-proc facts from the server, after item batch 2); its Chop Tree shows \
         the tree falling and no particle",
    ),
    (
        "butterfly",
        "Dota draws no effect for the passive; the evasion is a floating word, not a particle",
    ),
    (
        "greater_crit",
        "Dota sounds a critical strike and floats its damage; the server does not tell one (follow-up: typed attack-proc facts from the server, after item batch 2)",
    ),
    (
        "infused_raindrop",
        "a passive whose block the server does not tell",
    ),
    (
        "javelin",
        "Dota draws no effect for the passive; the server does not tell a proc",
    ),
    (
        "lesser_crit",
        "Dota sounds a critical strike and floats its damage; the server does not tell one (follow-up: typed attack-proc facts from the server, after item batch 2)",
    ),
    (
        "lifesteal",
        "Dota bursts blood on a lifesteal heal; the server does not tell one",
    ),
    (
        "monkey_king_bar",
        "Dota draws no effect for the passive; the server does not tell a proc",
    ),
    (
        "quelling_blade",
        "Dota shows the tree falling and no particle",
    ),
    (
        "revenants_brooch",
        "Phantom Critical is told as a blow of the item, but which of the install's brooch effects Dota plays for \
         it is not established (follow-up: typed attack-proc facts from the server, after item batch 2)",
    ),
    (
        "royale_with_cheese",
        "its Savory Shield is not simulated, and the game's code names no particle or sound for it",
    ),
    (
        "specialists_array",
        "the server does not tell which missiles a Splitshot threw: they fly as the carrier's attack (follow-up: typed attack-proc facts from the server, after item batch 2)",
    ),
    (
        "talisman_of_evasion",
        "Dota draws no effect for the passive; the evasion is a floating word, not a particle",
    ),
    ("tpscroll", "its channel is drawn by teleport.rs"),
    (
        "vanguard",
        "a passive whose block the server does not tell (follow-up: typed attack-proc facts from the server, after \
         item batch 2)",
    ),
    (
        "voodoo_mask",
        "Dota bursts on a spell lifesteal heal; the server does not tell one",
    ),
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
    let mut derived = Vec::new();
    for (kind, id, key) in manifest
        .ids
        .iter()
        .filter(|(k, ..)| matches!(k.as_str(), "item" | "ability"))
    {
        let none = NO_LINE.iter().find(|(listed, _)| listed == key);
        if kind == "item" && carries_stats_alone(*id) {
            // Dota draws no effect for what is used for nothing and procs nothing.
            if none.is_some() {
                derived.push(key.clone());
            }
            continue;
        }
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
    assert!(
        derived.is_empty(),
        "listed, but bota's data already says it carries stats alone: {derived:?}"
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

#[test]
fn gate_warp_and_look_keys_coexist_with_worn_model_radius_feeds() {
    use super::line::{Feed, Trigger};
    let warp = Fx::parse(&["unit.twin_gate.warp", "at=unit", "particle=example"])
        .expect("the gate warp trigger remains available");
    assert_eq!(warp.trigger, Trigger::Warp);
    for (name, team) in [
        ("radiant", bota_proto::Team::Radiant),
        ("dire", bota_proto::Team::Dire),
    ] {
        let look = format!("look={name}");
        let worn = Fx::parse(&[
            "unit.twin_gate.worn",
            "at=unit",
            "particle=example",
            &look,
            "cp1=model_radius",
        ])
        .expect("a gate's look-bound worn effect can use its imported model radius");
        assert_eq!(worn.look, Some(team));
        assert!(
            worn.feeds
                .iter()
                .any(|(_, feed)| matches!(feed, Feed::ModelRadius))
        );
        let error = Fx::parse(&["unit.twin_gate.warp", "at=unit", "particle=example", &look])
            .expect_err("a one-shot warp must not accept a worn-only look selector");
        assert_eq!(error, "only a worn line asks whose, how many or whose look");
    }
}
