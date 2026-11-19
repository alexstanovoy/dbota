//! Localized talent faces and effect tooltip fallbacks. The rules supply the numbers.

mod blurb;
mod keys;

use bota_core::{EFFECT_COUNT, EFFECTS as EFFECT_DEFS, effect_def, hero_def};
use bota_proto::{EffectId, HeroId, ItemId};

use blurb::fill;

use crate::dota_text::Texts;

/// Installed announcement labels for taking and denying notable ground loot.
pub fn loot_labels(id: ItemId) -> Option<[&'static str; 2]> {
    const LABELS: &[(ItemId, [&str; 2])] = &[(
        ItemId::AEGIS,
        ["DOTA_HUD_PickedUpAegis", "DOTA_HUD_DeniedAegis"],
    )];
    LABELS
        .iter()
        .find_map(|(item, labels)| (*item == id).then_some(*labels))
}

/// Installed team labels for planting notable ground items.
pub fn placed_labels(id: ItemId) -> Option<[&'static str; 2]> {
    const LABELS: &[(ItemId, [&str; 2])] = &[(
        ItemId::ROSHANS_BANNER,
        [
            "DOTA_HUD_RadiantBannerPlanted",
            "DOTA_HUD_DireBannerPlanted",
        ],
    )];
    LABELS
        .iter()
        .find_map(|(item, labels)| (*item == id).then_some(*labels))
}

/// One timed effect, as the client shows it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectFace {
    /// The id it answers to on the wire.
    pub id: u16,
    /// What it does, for the hover popup, as a template.
    pub template: &'static str,
}

impl EffectFace {
    const fn of(id: u16, template: &'static str) -> EffectFace {
        EffectFace { id, template }
    }

    /// What it does, with the numbers the rules give it.
    pub fn blurb(&self) -> String {
        fill(self.template)
    }
}

/// Every timed effect, indexed by [`EffectId`]; the slot of a retired one
/// holds nothing.
pub const EFFECTS: [EffectFace; FIRST_SPELL_EFFECT] = [
    EffectFace::of(0, ""),
    EffectFace::of(EffectId::MENDING.0, "Regenerating health."),
    EffectFace::of(EffectId::CLARITY.0, "Regenerating mana."),
    EffectFace::of(
        EffectId::FOUNTAIN.0,
        "Regenerating health and mana for standing in the fountain.",
    ),
    EffectFace::of(EffectId::STUNNED.0, "Cannot move, attack or cast."),
    EffectFace::of(EffectId::SLOWED.0, "Movement speed reduced."),
    EffectFace::of(EffectId::BURNING.0, "Losing health over time."),
    EffectFace::of(EffectId::HASTENED.0, "Movement speed increased."),
    EffectFace::of(EffectId::SHIELDED.0, "Nothing gets through."),
    EffectFace::of(EffectId::PHASED.0, "Walking through the bodies in the way."),
    EffectFace::of(
        EffectId::FLESH_HEAP.0,
        "Strength kept from every enemy hero that died nearby.",
    ),
    EffectFace::of(
        EffectId::SOULS.0,
        "Attack damage from every unit brought down.",
    ),
    EffectFace::of(
        EffectId::ARMOR_BROKEN.0,
        "Armor worn down by a dark presence nearby.",
    ),
    EffectFace::of(
        EffectId::GUARDED.0,
        "Armor and health mended, for standing by a tower of your own.",
    ),
    EffectFace::of(
        EffectId::INSPIRED.0,
        "Health mended and magic shrugged off, for marching beside the one carrying the flag.",
    ),
    EffectFace::of(
        EffectId::SHADOWRAZE.0,
        "Shadowrazes from the same caster deal {raze_stack} more magic damage per stack. Each damaging hit refreshes all stacks for {raze_secs} s; up to {raze_max_stacks} stacks.",
    ),
    EffectFace::of(
        EffectId::ROT.0,
        "In a rot: {rot_dps} damage a second and slowed {rot_slow}%. Its owner burns as well, but never to death, and is not slowed.",
    ),
    EffectFace::of(
        EffectId::FEARED.0,
        "Running from whoever put the fear on. Cannot attack or cast.",
    ),
    EffectFace::of(
        EffectId::MAGIC_BROKEN.0,
        "Magic resistance torn by a requiem.",
    ),
    EffectFace::of(
        EffectId::FEASTING.0,
        "Attacking and moving faster while taking souls from enemies nearby; counts the souls taken.",
    ),
    EffectFace::of(
        EffectId::FEAST_LINGER.0,
        "Souls a feast took from enemies since fallen, kept past the cap until it runs out.",
    ),
    EffectFace::of(
        EffectId::BLOOD_GRENADE.0,
        "Slowed {grenade_slow}% and losing {grenade_dps} health to magic every second, from a Blood Grenade.",
    ),
];

/// The first effect a neutral's spell leaves: from it on, every effect is
/// shown by its rules name.
const FIRST_SPELL_EFFECT: usize = EffectId::ROOTED.0 as usize;

/// Every effect from [`FIRST_SPELL_EFFECT`] on.
const SPELL_EFFECT_FACES: [EffectFace; EFFECT_COUNT - FIRST_SPELL_EFFECT] = {
    let mut faces = [EffectFace::of(0, ""); EFFECT_COUNT - FIRST_SPELL_EFFECT];
    let mut at = 0;
    while at < faces.len() {
        assert!(!EFFECT_DEFS[FIRST_SPELL_EFFECT + at].retired);
        faces[at] = EffectFace::of((FIRST_SPELL_EFFECT + at) as u16, "");
        at += 1;
    }
    // The heroes' effects added after the neutrals' have their own words.
    faces[EffectId::MEAT_SHIELD.0 as usize - FIRST_SPELL_EFFECT] = EffectFace::of(
        EffectId::MEAT_SHIELD.0,
        "Every blow loses the damage shown, after armor and resistance.",
    );
    faces[EffectId::DISMEMBERED.0 as usize - FIRST_SPELL_EFFECT] = EffectFace::of(
        EffectId::DISMEMBERED.0,
        "Held stunned and bitten by a dismember, pulled towards whoever holds it.",
    );
    faces[EffectId::ILLUSION.0 as usize - FIRST_SPELL_EFFECT] = EffectFace::of(
        EffectId::ILLUSION.0,
        "An illusion of the hero. Only its own side can tell it from the hero.",
    );
    faces[EffectId::TIMED_LIFE.0 as usize - FIRST_SPELL_EFFECT] =
        EffectFace::of(EffectId::TIMED_LIFE.0, "Gone when its time runs out.");
    faces[EffectId::FOUNTAIN_INVULNERABLE.0 as usize - FIRST_SPELL_EFFECT] = EffectFace::of(
        EffectId::FOUNTAIN_INVULNERABLE.0,
        "Risen at its fountain: invulnerable and untargetable, and attacking nobody of its own accord, until given a command or until the fountain's mending wears off.",
    );
    faces[EffectId::FOUNTAIN_PASSIVE.0 as usize - FIRST_SPELL_EFFECT] =
        EffectFace::of(EffectId::FOUNTAIN_PASSIVE.0, "");
    faces[EffectId::FOUNTAIN_DAMAGE.0 as usize - FIRST_SPELL_EFFECT] = EffectFace::of(
        EffectId::FOUNTAIN_DAMAGE.0,
        "Each shot of the fountain deals more by every stack; each shot that lands adds one and renews them all.",
    );
    faces
};

/// The effect of that id, or nothing for one the catalog does not hold.
pub fn effect(id: u16) -> Option<&'static EffectFace> {
    effect_def(EffectId(id))?;
    EFFECTS.get(usize::from(id)).or_else(|| {
        let at = usize::from(id).checked_sub(FIRST_SPELL_EFFECT)?;
        SPELL_EFFECT_FACES.get(at)
    })
}

/// One talent of a hero's tree as it appears in the installed localization.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TalentFace {
    pub name: String,
    pub blurb: String,
}

/// Resolves the talent slots of the hero reported by the server through the shared rules.
pub fn talents(
    hero: u16,
    texts: &Texts,
) -> Option<[TalentFace; bota_proto::TalentSlot::COUNT as usize]> {
    let hero = hero_def(HeroId(hero))?;
    let abilities: Vec<_> = hero
        .abilities
        .iter()
        .copied()
        .chain(hero.innate)
        .filter_map(bota_core::ability_def)
        .filter_map(|ability| bota_core::ability_data(ability.key))
        .collect();
    Some(
        hero.talents?
            .map(|talent| talent_face(talent.key(), &abilities, texts)),
    )
}

fn talent_face(key: &str, abilities: &[&bota_core::AbilityData], texts: &Texts) -> TalentFace {
    let numbers = talent_numbers(key, abilities);
    if key == bota_core::Talent::RequiemFear.key()
        && let Some(face) = legacy_requiem_face(key, &numbers, texts)
    {
        return face;
    }
    TalentFace {
        name: texts
            .talent_name(key, &numbers)
            .unwrap_or_else(|| "—".to_string()),
        blurb: texts.talent_description(key, &numbers).unwrap_or_default(),
    }
}

/// Older installs describe a per-line increase. The current bonus is applied to the total.
fn legacy_requiem_face(
    key: &str,
    numbers: &[(String, String)],
    texts: &Texts,
) -> Option<TalentFace> {
    let installed = texts.get(key)?;
    if !installed.name.contains("bonus_requiem_slow_duration") {
        return None;
    }
    let (_, seconds) = numbers
        .iter()
        .find(|(key, _)| key == "bonus_requiem_fear_duration")?;
    let ability = bota_core::ability_def(bota_proto::AbilityId::REQUIEM)?;
    let ability_name = &texts.get(ability.key)?.name;
    Some(TalentFace {
        name: format!("+{seconds}s {ability_name} fear/slow"),
        blurb: String::new(),
    })
}

fn talent_numbers(key: &str, abilities: &[&bota_core::AbilityData]) -> Vec<(String, String)> {
    if let Some(talent) = bota_core::ability_data(key) {
        return talent
            .values
            .iter()
            .map(|value| (value.name.to_ascii_lowercase(), talent_number(value.at(0))))
            .collect();
    }
    abilities
        .iter()
        .flat_map(|ability| ability.values)
        .filter_map(|value| {
            let (_, bonus) = value.bonuses.iter().find(|(talent, _)| *talent == key)?;
            let number = match bonus {
                bota_core::Bonus::Add(levels) | bota_core::Bonus::Set(levels) => *levels.first()?,
                bota_core::Bonus::Times(factor) | bota_core::Bonus::Percent(factor) => *factor,
                bota_core::Bonus::HeroLevels { .. } => return None,
            };
            let name = format!("bonus_{}", value.name.to_ascii_lowercase());
            Some((name, talent_number(number)))
        })
        .collect()
}

/// Localization supplies the sign (including for cooldown reductions and stronger slows).
fn talent_number(value: bota_core::Dec) -> String {
    (f64::from(value.0.unsigned_abs()) / 1000.0).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_is_placed_at_its_own_id() {
        for (index, face) in EFFECTS.iter().enumerate().skip(1) {
            assert_eq!(usize::from(face.id), index, "effect {index} is in place");
            assert_eq!(effect(face.id), Some(face), "effect {index} is found");
        }
        assert!(effect(0).is_none(), "the retired effect is gone");
        assert!(effect(u16::MAX).is_none());
        // Ids are append-only: every effect the rules have has a face under
        // its own id, the spells' after the heroes' included.
        for (id, def) in EFFECT_DEFS
            .iter()
            .enumerate()
            .filter(|(_, def)| !def.retired)
        {
            let face = effect(id as u16).unwrap_or_else(|| panic!("{} is found", def.key));
            assert_eq!(usize::from(face.id), id, "{} is found by its id", def.key);
        }
    }

    #[test]
    fn every_blurb_takes_its_numbers_from_the_rules() {
        for template in EFFECTS.iter().map(|f| f.template) {
            assert!(
                !template.chars().any(|c| c.is_ascii_digit()),
                "{template:?} types a number the rules own"
            );
        }
        // Filling panics on a key nothing answers to, so one pass proves them all.
        for face in EFFECTS.iter().chain(&SPELL_EFFECT_FACES) {
            assert!(!face.blurb().contains('{'), "effect {}", face.id);
        }
        for (id, hero) in bota_core::HEROES.iter().enumerate() {
            let tree = talents(id as u16, &Texts::default())
                .unwrap_or_else(|| panic!("{} has a tree", hero.key));
            assert!(
                tree.iter()
                    .all(|t| !t.name.contains('{') && !t.blurb.contains('{')),
                "{}",
                hero.key
            );
        }
        assert!(talents(u16::MAX, &Texts::default()).is_none());
    }
}
