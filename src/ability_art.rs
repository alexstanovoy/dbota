//! How each ability looks: the clip its caster plays, what the ground shows
//! while it is cast, and the missile it flies. The one place the renderer
//! keys on ability ids.

use bota_proto::AbilityId;

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
        AbilityId::THROW_GRENADE => AbilityArt {
            flight: Flight::Grenade,
            ..PLAIN
        },
        _ => PLAIN,
    }
}
