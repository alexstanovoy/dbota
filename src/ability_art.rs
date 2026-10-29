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
}

const PLAIN: AbilityArt = AbilityArt {
    clip: "cast1",
    telegraph: Telegraph::None,
    flight: Flight::None,
    scorches: false,
    channel: None,
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
        // Dota's ACT_DOTA_GENERIC_CHANNEL_1, the channel at an Outpost or a
        // Watcher.
        AbilityId::CAPTURE | AbilityId::LAMP_USE => AbilityArt {
            channel: Some("channel"),
            ..PLAIN
        },
        AbilityId::DISMEMBER => AbilityArt {
            clip: "dismember",
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
        flight: if bolt { Flight::Bolt } else { Flight::None },
        ..PLAIN
    }
}

/// The clip a neutral plays for an ability: `cast1` to `cast6` are Dota's
/// ACT_DOTA_CAST_ABILITY_1 to 6, the activity the ability's
/// `AbilityCastAnimation` names (1 where it names none). A model without
/// the activity (the Fell Spirit has none for Vex) has no such clip and
/// keeps its pose through the cast, as in Dota.
const fn neutral_cast_clip(ability: NeutralAbility) -> &'static str {
    use NeutralAbility as N;
    match ability {
        N::Heal
        | N::TakeOff
        | N::ChainLightning
        | N::WarStomp
        | N::Purge
        | N::ManaBurn
        | N::Shockwave
        | N::IceArmor
        | N::HurlBoulder
        | N::ThunderClap
        | N::Tornado
        | N::Hurricane
        | N::Ensnare
        | N::SeedShot
        | N::Fireball
        | N::Frenzy
        | N::IcefireBomb
        | N::Desecrate => "cast1",
        N::Vex
        | N::Intimidate
        | N::OgreSmash
        | N::RaiseDead
        | N::WaterBubbleSmall
        | N::WaterBubbleMedium
        | N::WaterBubbleLarge => "cast2",
        N::Slam => "cast3",
        N::ArmOfTheDeep | N::TendrilsOfTheDeep | N::CongregationsOfTheDeep => "cast5",
        // Never cast, so never shown: Dota's default animation.
        N::ProspectingAura
        | N::NeutralUpgrade
        | N::StealWeapon
        | N::SpeedAura
        | N::Break
        | N::Piercing
        | N::HealAmplificationAura
        | N::EnvenomedWeapon
        | N::FrostAttack
        | N::CloakAura
        | N::CriticalStrike
        | N::PackleadersAura
        | N::ManaAura
        | N::UnholyAura
        | N::ShardSplit
        | N::SwiftnessAura
        | N::DeathThroeRush
        | N::DeathThroePower
        | N::ToughnessAura
        | N::MagicAmplificationAura
        | N::SplashAttack
        | N::DragonhideAura
        | N::WeakeningAura
        | N::GraniteAura
        | N::WarDrumsAura
        | N::TimeWarpAura
        | N::ProwlerAura
        | N::Petrify
        | N::RiverbornAura
        | N::Rally
        | N::Tempest => "cast1",
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

#[cfg(test)]
mod tests {
    use bota_core::NEUTRALS;

    use super::*;
    use crate::manifest::MANIFEST;

    /// Every neutral's cast has a clip of its activity on its model, but
    /// the Fell Spirit's Vex: its model has no animation for it in Dota.
    #[test]
    fn every_neutral_cast_has_its_clip() {
        let words = || {
            MANIFEST
                .lines()
                .map(|l| l.split_whitespace().collect::<Vec<_>>())
        };
        let mut missing = Vec::new();
        for neutral in &NEUTRALS {
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
                    missing.push((neutral.unit, ability.entry().def.name, clip));
                }
            }
        }
        assert_eq!(missing, [("npc_dota_neutral_fel_beast", "Vex", "cast2")]);
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
        let mut checked = Vec::new();
        for neutral in &NEUTRALS {
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
                assert!(
                    (point * 1000.0 - def.point as f32).abs() < 1.0,
                    "{} on {}: clip point {point} s, cast point {} ms",
                    def.name,
                    neutral.unit,
                    def.point
                );
                checked.push(def.name);
            }
        }
        assert!(checked.contains(&"Ice Armor"), "{checked:?}");
        assert!(checked.contains(&"Ogre Smash!"), "{checked:?}");
    }
}
