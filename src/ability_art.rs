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
}

const PLAIN: AbilityArt = AbilityArt {
    clip: "cast1",
    telegraph: Telegraph::None,
    flight: Flight::None,
    scorches: false,
};

const RAZE: AbilityArt = AbilityArt {
    scorches: true,
    ..PLAIN
};

/// The look of an ability; a plain cast for one with none of its own.
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
        AbilityId::DISMEMBER => AbilityArt {
            clip: "dismember",
            ..PLAIN
        },
        // A neutral's bolt flies as an attack's missile does.
        id if NeutralAbility::of(id).is_some_and(|ability| {
            matches!(
                ability.spell(),
                Spell::Cast {
                    act: Act::Bolt { .. },
                    ..
                }
            )
        }) =>
        {
            AbilityArt {
                flight: Flight::Bolt,
                ..PLAIN
            }
        }
        _ => PLAIN,
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
