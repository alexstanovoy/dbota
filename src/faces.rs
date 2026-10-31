//! The words of items, effects and talents, for the tooltips of a game
//! without Dota's own text and for the fallback shop.
//!
//! Item names, one to an item, found by the id the wire carries; effects and
//! talents also carry blurb templates. The numbers in a blurb are the rules':
//! a template names them and `bota_core` gives them.

mod blurb;
mod keys;

use bota_core::{
    EFFECT_COUNT, EFFECTS as EFFECT_DEFS, ITEMS as ITEM_DEFS, Talent, effect_def, hero_def,
};
use bota_proto::{EffectId, HeroId, ItemId};

use blurb::fill;

/// One item, as the client shows and aims it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ItemFace {
    /// The id it answers to on the wire.
    pub id: u16,
    /// Short name for the panel and the shop.
    pub name: &'static str,
}

impl ItemFace {
    const fn new(id: u16, name: &'static str) -> ItemFace {
        ItemFace { id, name }
    }
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

/// Every item the shop sells, in id order.
pub const ITEMS: [ItemFace; ITEM_DEFS.len()] = [
    ItemFace::new(ItemId::BOOTS.0, "Boots"),
    ItemFace::new(ItemId::CLARITY.0, "Clarity"),
    ItemFace::new(ItemId::HEALING_SALVE.0, "Salve"),
    ItemFace::new(ItemId::IRON_BRANCH.0, "Branch"),
    ItemFace::new(ItemId::OBSERVER_WARD.0, "Obs"),
    ItemFace::new(ItemId::QUELLING_BLADE.0, "Quell"),
    ItemFace::new(ItemId::SENTRY_WARD.0, "Sentry"),
    ItemFace::new(ItemId::TANGO.0, "Tango"),
    ItemFace::new(ItemId::TOWN_PORTAL_SCROLL.0, "TP"),
    ItemFace::new(ItemId::CIRCLET.0, "Circlet"),
    ItemFace::new(ItemId::GAUNTLETS.0, "Gauntlet"),
    ItemFace::new(ItemId::SLIPPERS.0, "Slippers"),
    ItemFace::new(ItemId::MANTLE.0, "Mantle"),
    ItemFace::new(ItemId::BELT.0, "Belt"),
    ItemFace::new(ItemId::BAND.0, "Band"),
    ItemFace::new(ItemId::ROBE.0, "Robe"),
    ItemFace::new(ItemId::OGRE_AXE.0, "Ogre Axe"),
    ItemFace::new(ItemId::BLADE_OF_ALACRITY.0, "Alacrity"),
    ItemFace::new(ItemId::STAFF_OF_WIZARDRY.0, "Wizardry"),
    ItemFace::new(ItemId::GLOVES.0, "Gloves"),
    ItemFace::new(ItemId::BLADES_OF_ATTACK.0, "Blades"),
    ItemFace::new(ItemId::BROADSWORD.0, "Broad"),
    ItemFace::new(ItemId::QUARTERSTAFF.0, "Qstaff"),
    ItemFace::new(ItemId::RING_OF_PROTECTION.0, "Ring Pro"),
    ItemFace::new(ItemId::CHAINMAIL.0, "Chain"),
    ItemFace::new(ItemId::RING_OF_REGEN.0, "Regen"),
    ItemFace::new(ItemId::SAGES_MASK.0, "Sage"),
    ItemFace::new(ItemId::VITALITY_BOOSTER.0, "Vitality"),
    ItemFace::new(ItemId::ENERGY_BOOSTER.0, "Energy"),
    ItemFace::new(ItemId::POWER_TREADS.0, "Treads"),
    ItemFace::new(ItemId::PHASE_BOOTS.0, "Phase"),
    ItemFace::new(ItemId::BLINK_DAGGER.0, "Blink"),
    ItemFace::new(ItemId::BRACER.0, "Bracer"),
    ItemFace::new(ItemId::WRAITH_BAND.0, "Wraith"),
    ItemFace::new(ItemId::NULL_TALISMAN.0, "Null"),
    ItemFace::new(ItemId::MAGIC_STICK.0, "Stick"),
    ItemFace::new(ItemId::MAGIC_WAND.0, "Wand"),
    // Retired: the recipe Phase Boots were built with.
    ItemFace::new(37, "Retired"),
    ItemFace::new(ItemId::RECIPE_BRACER.0, "Rcp Bracer"),
    ItemFace::new(ItemId::RECIPE_WRAITH_BAND.0, "Rcp Wraith"),
    ItemFace::new(ItemId::RECIPE_NULL_TALISMAN.0, "Rcp Null"),
    ItemFace::new(ItemId::RECIPE_MAGIC_WAND.0, "Rcp Wand"),
    ItemFace::new(ItemId::MANGO.0, "Mango"),
    ItemFace::new(ItemId::EAGLESONG.0, "Eaglesong"),
    ItemFace::new(ItemId::CLAYMORE.0, "Claymore"),
    ItemFace::new(ItemId::TALISMAN_OF_EVASION.0, "Talisman"),
    ItemFace::new(ItemId::BUTTERFLY.0, "Butterfly"),
    ItemFace::new(ItemId::JAVELIN.0, "Javelin"),
    ItemFace::new(ItemId::DEMON_EDGE.0, "Demon Edge"),
    ItemFace::new(ItemId::BLITZ_KNUCKLES.0, "Blitz"),
    ItemFace::new(ItemId::RECIPE_MONKEY_KING_BAR.0, "Rcp MKB"),
    ItemFace::new(ItemId::MONKEY_KING_BAR.0, "MKB"),
    ItemFace::new(ItemId::WIND_LACE.0, "Lace"),
    ItemFace::new(ItemId::BLOOD_GRENADE.0, "Grenade"),
    ItemFace::new(ItemId::WARD_DISPENSER.0, "Wards"),
    ItemFace::new(ItemId::TANGO_SHARED.0, "Tango"),
    ItemFace::new(ItemId::BOTTLE.0, "Bottle"),
];

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

/// One talent of a hero's tree, as the talent panel shows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TalentFace {
    /// What it gives, short, as Dota's tree writes it.
    pub name: String,
    /// What it does, for the hover popup.
    pub blurb: String,
}

/// What a talent is called and does, with the rules' numbers in.
fn talent_face(talent: Talent) -> TalentFace {
    let (name, blurb) = match talent {
        Talent::FeastAttackSpeed => (
            "+{talent_feast_speed} Feast of Souls Attack Speed",
            "A feast gives {talent_feast_speed} more attack speed.",
        ),
        Talent::RazeStackDamage => (
            "+{talent_raze_stack} Shadowraze Stack Damage",
            "Each stack of a raze's mark adds {talent_raze_stack} more magic damage.",
        ),
        Talent::FeastSoulsPerHero => (
            "+{talent_feast_souls} Feast of Souls Souls Per Hero",
            "A feast takes {talent_feast_souls} more souls from every enemy hero.",
        ),
        Talent::PresenceArmor => (
            "+{talent_presence} Presence Armor Reduction",
            "The presence takes {talent_presence} more armor from enemies near you.",
        ),
        Talent::RequiemFear => (
            "+{talent_fear}s Requiem Fear per Line (max +{talent_fear_max}s)",
            "Every requiem line fears, slows and tears {talent_fear} s longer, up to {talent_fear_max} s more in all.",
        ),
        Talent::NecroSoulCap => (
            "+{talent_souls} Necromastery Max Souls",
            "Hold {talent_souls} more souls from kills.",
        ),
        Talent::RazeAttackDamage => (
            "Shadowraze Applies Attack Damage",
            "A raze also deals your attack damage, as physical damage.",
        ),
        Talent::FeastCastSpeed => (
            "Feast of Souls Grants +{talent_cast_speed}% Cast Speed",
            "While a feast runs, cast points last {talent_cast_pct}% of their length.",
        ),
    };
    TalentFace {
        name: fill(name),
        blurb: fill(blurb),
    }
}

/// A hero's talent tree by talent slot, if it has one.
pub fn talents(hero: u16) -> Option<Vec<TalentFace>> {
    let tree = hero_def(HeroId(hero))?.talents?;
    Some(tree.iter().copied().map(talent_face).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_face_is_placed_at_its_own_id() {
        for (index, face) in ITEMS.iter().enumerate() {
            assert_eq!(usize::from(face.id), index, "{} is in place", face.name);
            assert!(!face.name.is_empty(), "item {index} has a name");
        }
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
            let face = effect(id as u16).unwrap_or_else(|| panic!("{} is found", def.name));
            assert_eq!(usize::from(face.id), id, "{} is found by its id", def.name);
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
        let tree = talents(bota_proto::HeroId::SHADOW_FIEND.0).expect("Shadow Fiend has a tree");
        assert!(
            tree.iter()
                .all(|t| !t.name.contains('{') && !t.blurb.contains('{'))
        );
        assert!(talents(u16::MAX).is_none());
    }
}
