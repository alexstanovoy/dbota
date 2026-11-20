//! How each ability looks: the clip its caster plays, what the ground shows
//! while it is cast, and the missile it flies; and how an item's throw flies.
//! The one place the renderer keys on ability and item ids.

use bota_core::{Act, NeutralAbility, Spell};
use bota_proto::{AbilityId, Inflictor, ItemId};

/// What an ability draws on the ground while it is being cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Telegraph {
    /// Nothing.
    None,
    /// The ring of a raze at one of its three reaches, near to far.
    Raze { reach: usize },
    /// The rings of a requiem about its caster.
    Requiem,
}

/// What the missile of an ability looks like, if it flies one the renderer
/// draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flight {
    /// A flight with no body of its own: the ability's burst tells it.
    None,
    /// A plain missile, as an attack's, in its side's colour.
    Bolt,
    /// A line of the requiem.
    Soul,
    /// A blood grenade.
    Grenade,
}

/// How one ability looks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AbilityArt {
    /// The clip its caster plays through the cast point.
    pub clip: &'static str,
    /// What the ground shows meanwhile.
    pub telegraph: Telegraph,
    /// The missile it flies.
    pub flight: Flight,
    /// Whether the ground is scorched where its missile lands.
    pub scorches: bool,
    /// The clip its caster loops while it channels it, when it has one.
    pub channel: Option<&'static str>,
    /// The attack follow-through used by a conditional attack proc.
    pub attack: Option<&'static str>,
}

const PLAIN: AbilityArt = AbilityArt {
    clip: "cast1",
    telegraph: Telegraph::None,
    flight: Flight::None,
    scorches: false,
    channel: None,
    attack: None,
};

const RAZE: AbilityArt = AbilityArt {
    scorches: true,
    ..PLAIN
};

/// The look of an ability; a plain cast for one with none of its own.
/// A neutral's abilities map to the clip of their Dota activity.
pub fn art_of(id: AbilityId) -> AbilityArt {
    match id {
        AbilityId::RAZE_NEAR => AbilityArt {
            telegraph: Telegraph::Raze { reach: 0 },
            ..RAZE
        },
        AbilityId::RAZE_MID => AbilityArt {
            clip: "cast2",
            telegraph: Telegraph::Raze { reach: 1 },
            ..RAZE
        },
        AbilityId::RAZE_FAR => AbilityArt {
            clip: "cast3",
            telegraph: Telegraph::Raze { reach: 2 },
            ..RAZE
        },
        AbilityId::REQUIEM => AbilityArt {
            clip: "requiem",
            telegraph: Telegraph::Requiem,
            flight: Flight::Soul,
            ..PLAIN
        },
        // Dota's ACT_DOTA_GENERIC_CHANNEL_1, the channel at an Outpost, a
        // Watcher or a Twin Gate (twin_gate_portal_warp's AbilityCastAnimation).
        AbilityId::CAPTURE | AbilityId::LAMP_USE | AbilityId::TWIN_GATE_WARP => AbilityArt {
            channel: Some("channel"),
            ..PLAIN
        },
        AbilityId::DISMEMBER => AbilityArt {
            clip: "dismember",
            channel: Some("dismember_channel"),
            ..PLAIN
        },
        id => NeutralAbility::of(id).map_or(PLAIN, neutral_art),
    }
}

/// The look of a neutral's ability: the clip of the activity Dota plays for
/// it, and a bolt's flight as an attack's missile.
fn neutral_art(ability: NeutralAbility) -> AbilityArt {
    let bolt = matches!(
        ability.spell(),
        Spell::Cast {
            act: Act::Bolt { .. },
            ..
        }
    );
    AbilityArt {
        clip: neutral_cast_clip(ability),
        attack: matches!(
            ability.spell(),
            Spell::Strike(bota_core::Strike::Bash { .. })
        )
        .then_some("bash"),
        channel: matches!(
            ability.spell(),
            Spell::Cast {
                act: Act::Grab(_),
                ..
            }
        )
        .then_some("cast2"),
        flight: if bolt { Flight::Bolt } else { Flight::None },
        ..PLAIN
    }
}

/// An explicit numbered cast activity takes precedence over the model defaults.
const fn neutral_cast_clip(ability: NeutralAbility) -> &'static str {
    use NeutralAbility as N;
    if let Some(slot) = ability.entry().cast_animation_slot {
        const CLIPS: [&str; 7] = [
            "cast1", "cast2", "cast3", "cast4", "cast5", "cast6", "cast7",
        ];
        return CLIPS[slot as usize - 1];
    }
    match ability {
        N::GrabAndThrow
        | N::Bash
        | N::Vex
        | N::Intimidate
        | N::OgreSmash
        | N::RaiseDead
        | N::WaterBubbleSmall
        | N::WaterBubbleMedium
        | N::WaterBubbleLarge => "cast2",
        N::Slam | N::RoshanSlam => "cast3",
        N::ArmOfTheDeep | N::TendrilsOfTheDeep | N::CongregationsOfTheDeep => "cast5",
        _ => "cast1",
    }
}

/// The look of what an ability or an item lets fly: an ability's own, a
/// Blood Grenade's, and nothing of its own for any other item.
pub fn art_by(inflictor: Inflictor) -> AbilityArt {
    match inflictor {
        Inflictor::Ability(id) => art_of(id),
        Inflictor::Item(ItemId::BLOOD_GRENADE) => AbilityArt {
            flight: Flight::Grenade,
            ..PLAIN
        },
        Inflictor::Item(_) => PLAIN,
    }
}

/// Activity overrides keyed by the installed modifier identity.
pub fn run_clip(effects: &[bota_proto::EffectView]) -> &'static str {
    const ACTIVITIES: &[(&str, &str)] = &[("modifier_roshan_moving", "run_haste")];
    effects
        .iter()
        .filter_map(|e| bota_core::effect_def(e.id))
        .find_map(|effect| {
            ACTIVITIES
                .iter()
                .find(|(key, _)| *key == effect.key)
                .map(|(_, clip)| *clip)
        })
        .unwrap_or("run")
}

#[cfg(test)]
mod tests {
    use bota_core::NEUTRALS;

    use super::*;
    use crate::manifest::MANIFEST;

    /// Every neutral's cast has a clip of its activity on its model, but
    /// the Fell Spirit's Vex (its model has no animation for it in Dota) and
    /// the two casts Dota 7.42 gave the Kobold Soldier and the Mud Golem: the
    /// manifest holds no such clip of their models yet (7.42's art is not
    /// imported yet), so they keep their pose through the cast, as a
    /// model without the activity does.
    #[test]
    fn the_reworked_neutral_casts_use_the_imported_numbered_activities() {
        use NeutralAbility as N;
        for (spell, clip) in [
            (N::StealWeaponCast, "cast1"),
            (N::SnowballShot, "cast1"),
            (N::Eggstremism, "cast1"),
            (N::HurlBigBoulder, "cast2"),
            (N::SealFlop, "cast2"),
            (N::MinersSlam, "cast3"),
        ] {
            assert_eq!(art_of(spell.id()).clip, clip, "{spell:?}");
        }
    }

    #[test]
    fn every_neutral_cast_has_its_clip() {
        let words = || {
            MANIFEST
                .lines()
                .map(|l| l.split_whitespace().collect::<Vec<_>>())
        };
        let mut missing = Vec::new();
        for neutral in NEUTRALS
            .iter()
            .filter(|neutral| !crate::units::PENDING_742_ART.contains(&neutral.unit))
        {
            let model = words()
                .find_map(|w| match w.as_slice() {
                    ["unit", unit, model, ..] if *unit == neutral.unit => Some(model.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no unit line for {}", neutral.unit));
            for ability in neutral.abilities {
                if !matches!(ability.spell(), Spell::Cast { .. } | Spell::Uncast) {
                    continue;
                }
                let clip = art_of(ability.id()).clip;
                let has = words().any(|w| {
                    matches!(w.as_slice(),
                    ["clip", m, c, ..] if *m == model && *c == clip)
                });
                if !has {
                    missing.push((neutral.unit, ability.entry().def.key, clip));
                }
            }
        }
        assert_eq!(
            missing,
            [
                (
                    "npc_dota_neutral_kobold_tunneler",
                    "bota_kobold_steal_weapon",
                    "cast1"
                ),
                ("npc_dota_neutral_fel_beast", "fel_beast_haunt", "cast2"),
                (
                    "npc_dota_neutral_mud_golem",
                    "mud_golem_hurl_big_boulder",
                    "cast2"
                ),
            ]
        );
    }

    /// A neutral's cast clip reaches its pose at the ability's own cast
    /// point, in Dota's numbers (the Frostmage's Ice Armor 0.56 s, the
    /// Bruiser's Ogre Smash 2.8 s), so what is drawn and what the simulation
    /// does go off together.
    #[test]
    fn every_neutral_cast_clip_lands_on_the_abilitys_cast_point() {
        let words = || {
            MANIFEST
                .lines()
                .map(|l| l.split_whitespace().collect::<Vec<_>>())
        };
        // Where Dota 7.42 moved a cast point ("Searing Swoop: Cast Point increased from 0.3s to
        // 0.6s when cast by a neutral creep") the manifest still holds 7.41f's clip until 7.42's
        // art is imported. Each such pair names the clip point (in milliseconds) the manifest holds
        // now: the test asserts it, so that it fails when the art arrives and the entry has to go.
        const PENDING_742: [(&str, f32); 1] = [("bota_black_dragon_searing_swoop", 300.0)];
        let mut checked = Vec::new();
        let mut pending = Vec::new();
        for neutral in NEUTRALS
            .iter()
            .filter(|neutral| !crate::units::PENDING_742_ART.contains(&neutral.unit))
        {
            let model = words()
                .find_map(|w| match w.as_slice() {
                    ["unit", unit, model, ..] if *unit == neutral.unit => Some(model.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no unit line for {}", neutral.unit));
            for ability in neutral.abilities {
                if !matches!(ability.spell(), Spell::Cast { .. } | Spell::Uncast) {
                    continue;
                }
                let clip = art_of(ability.id()).clip;
                let point = words().find_map(|w| match w.as_slice() {
                    ["clip", m, c, _, _, rest @ ..] if *m == model && *c == clip => rest
                        .iter()
                        .find_map(|r| r.strip_prefix("point=")?.parse::<f32>().ok()),
                    _ => None,
                });
                let Some(point) = point else { continue };
                let def = ability.entry().def;
                if let Some((_, stale)) = PENDING_742.iter().find(|(key, _)| *key == def.key) {
                    assert!(
                        (point * 1000.0 - stale).abs() < 1.0,
                        "{} on {}: the clip point is {point} s now, no longer 7.41f's {stale} ms: \
                         7.42's art has arrived, so drop the entry of PENDING_742",
                        def.key,
                        neutral.unit
                    );
                    pending.push(def.key);
                    continue;
                }
                // Immediate activation may schedule an explicit effect windup.
                let effect_point = match ability.spell() {
                    Spell::Cast {
                        act: Act::Roar(spec),
                        ..
                    } => spec.windup * 1000 / bota_core::rules::TICKS_PER_SECOND,
                    _ => def.point,
                };
                assert!(
                    (point * 1000.0 - effect_point as f32).abs() < 1.0,
                    "{} on {}: clip point {point} s, effect point {} ms",
                    def.key,
                    neutral.unit,
                    effect_point
                );
                checked.push(def.key);
            }
        }
        assert!(checked.contains(&"ogre_magi_frost_armor"), "{checked:?}");
        assert!(checked.contains(&"ogre_bruiser_ogre_smash"), "{checked:?}");
        assert_eq!(
            pending,
            PENDING_742.map(|(key, _)| key),
            "every pending entry stands on an ability whose clip is read"
        );
    }
}
