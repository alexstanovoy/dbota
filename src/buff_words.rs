//! What a buff or a debuff is called and what it says it does, as Dota's
//! buff tooltip has it: the name and the description of the Dota modifier
//! the effect is (`DOTA_Tooltip_<modifier>`, from the install's
//! localization), else the name of the ability whose icon it wears, else
//! dbota's own words. Never the rules' identifier of the effect.

use crate::dota_text::Texts;
use crate::manifest::Manifest;
use crate::tip_content::Lookup;

/// dbota's own name of every live effect, by id: what shows without the
/// asset cache, or where Dota has no tooltip for it.
const OWN_NAMES: [(u16, &str); 77] = [
    (1, "Healing Salve"),
    (2, "Clarity"),
    (3, "Fountain Rejuvenation"),
    (4, "Stunned"),
    (5, "Slowed"),
    (6, "Burning"),
    (7, "Speed Burst"),
    (8, "Shield"),
    (9, "Phase Boots"),
    (10, "Flesh Heap"),
    (11, "Necromastery"),
    (12, "Presence of the Dark Lord"),
    (13, "Tower Armor"),
    (14, "Inspiration"),
    (15, "Shadowraze"),
    (16, "Rot"),
    (17, "Requiem of Souls"),
    (18, "Requiem of Souls"),
    (19, "Feast of Souls"),
    (20, "Feast of Souls"),
    (21, "Blood Grenade"),
    (22, "Rooted"),
    (23, "Disarmed"),
    (24, "Broken"),
    (25, "Steal Weapon"),
    (26, "Speed Aura"),
    (27, "Envenomed Weapon"),
    (28, "Frost Attack"),
    (29, "Cloak Aura"),
    (30, "Intimidate"),
    (31, "Packleader's Aura"),
    (32, "Mana Aura"),
    (33, "Unholy Aura"),
    (34, "Shard Split"),
    (35, "Swiftness Aura"),
    (36, "Death Throe: Rush"),
    (37, "Thunder Clap"),
    (38, "Death Throe: Power"),
    (39, "Toughness Aura"),
    (40, "Magic Amplification Aura"),
    (41, "Dragonhide Aura"),
    (42, "Weakening Aura"),
    (43, "Granite Aura"),
    (44, "War Drums Aura"),
    (45, "Slam"),
    (46, "Frenzy"),
    (47, "Time Warp Aura"),
    (48, "Prowler Aura"),
    (49, "Desecrate"),
    (50, "Petrify"),
    (51, "Riverborn Aura"),
    (52, "Rally"),
    (53, "Tempest"),
    (54, "Water Bubble"),
    (55, "Purge"),
    (56, "Glyph of Fortification"),
    (57, "Meat Shield"),
    (58, "Dismember"),
    (59, "Backdoor Protection"),
    (60, "Illusion"),
    (61, "Time Left"),
    (62, "Neutral Upgrade"),
    (63, "Flying"),
    (64, "Fountain Invulnerability"),
    (65, "Fountain Aura"),
    (66, "Fountain Damage"),
    (67, "Granted Vision"),
    (68, "Deactivated"),
    (69, "Capturing"),
    (70, "Shrine Ready"),
    (71, "Haste"),
    (72, "Amplify Damage"),
    (73, "Arcane"),
    (74, "Regeneration"),
    (75, "Invisible"),
    (76, "Shielded"),
    (77, "Bottle Regeneration"),
];

/// dbota's own name of an effect; none for an id it does not know.
pub fn own_name(id: u16) -> Option<&'static str> {
    OWN_NAMES
        .iter()
        .find(|(known, _)| *known == id)
        .map(|(_, name)| *name)
}

/// The Dota modifier's tooltip line `suffix` (empty: its name), when the
/// cache has it and it holds no value Dota fills in at run time
/// (`%dMODIFIER_PROPERTY_…%`), which dbota cannot know.
fn modifier_text(texts: &Texts, manifest: &Manifest, id: u16, suffix: &str) -> Option<String> {
    let modifier = manifest.dota_name("modifier", id)?;
    let line = texts.label(&format!("DOTA_Tooltip_{modifier}{suffix}"), &[])?;
    let plain = line.replace("%%", "\u{0}");
    (!plain.contains('%') && !line.trim().is_empty()).then(|| plain.replace('\u{0}', "%"))
}

/// What the buff bar's tooltip calls an effect: Dota's modifier name, else
/// the name of the ability or item whose icon it wears, else dbota's own.
pub fn name(texts: &Texts, manifest: &Manifest, id: u16) -> String {
    modifier_text(texts, manifest, id, "")
        .or_else(|| {
            let icon = manifest.dota_name("effect", id)?;
            let key = icon
                .strip_prefix("icon.ability.")
                .map(str::to_owned)
                .or_else(|| icon.strip_prefix("icon.item.").map(|i| format!("item_{i}")))?;
            texts.get(&key).map(|entry| entry.name.clone())
        })
        .filter(|name| !name.trim().is_empty())
        .or_else(|| own_name(id).map(str::to_owned))
        .unwrap_or_default()
}

/// What the tooltip says an effect does: Dota's modifier description where
/// it needs no value filled in, else Dota's description of the ability or
/// item whose icon it wears (its numbers filled as that one's tooltip fills
/// them), else dbota's own.
pub fn description(look: &Lookup, id: u16) -> String {
    modifier_text(look.texts, look.manifest, id, "_Description")
        .or_else(|| source_description(look, id))
        .filter(|text| !text.trim().is_empty())
        .unwrap_or_else(|| crate::faces::effect(id).map_or_else(String::new, |f| f.blurb()))
}

/// The description of the ability or item whose icon an effect wears, as
/// its own tooltip words it, at its first level.
fn source_description(look: &Lookup, id: u16) -> Option<String> {
    let icon = look.manifest.dota_name("effect", id)?;
    let of = |kind: &str, name: &str| {
        look.manifest
            .ids
            .iter()
            .find(|(k, _, n)| k == kind && n == name)
            .map(|(_, at, _)| *at)
    };
    let content = if let Some(name) = icon.strip_prefix("icon.ability.") {
        look.ability(of("ability", name)?, 1, 1)?
    } else {
        look.item(of("item", icon.strip_prefix("icon.item.")?)?, None)?
    };
    let lines: Vec<String> = content
        .sections
        .iter()
        .map(|section| section.body.iter().map(|run| run.text.as_str()).collect())
        .collect();
    Some(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use bota_proto::{IdSpace, registered};

    use super::{OWN_NAMES, name};
    use crate::dota_text::Texts;
    use crate::manifest::Manifest;

    /// Whether a word runs two words together the way an identifier does
    /// (`TimeWarpAura`), or is shouted in capitals (`MANAAURA`).
    fn reads_as_identifier(name: &str) -> bool {
        let chars: Vec<char> = name.chars().collect();
        let joined = chars
            .windows(2)
            .any(|w| w[0].is_lowercase() && w[1].is_uppercase());
        let shouted =
            chars.iter().filter(|c| c.is_alphabetic()).count() > 3 && name == name.to_uppercase();
        joined || shouted || name.contains('_')
    }

    /// No buff is shown under the rules' identifier of its effect: every live
    /// effect has a name of dbota's own, worded, and that is what a client
    /// without Dota's texts shows.
    #[test]
    fn no_buff_is_named_by_its_identifier() {
        let texts = Texts::default();
        let manifest = Manifest::load(std::path::Path::new("/no/asset/cache"));
        for entry in registered(IdSpace::Effect).filter(|e| !e.retired) {
            let shown = name(&texts, &manifest, entry.id);
            assert!(
                !shown.is_empty(),
                "effect {} ({}) has no name",
                entry.id,
                entry.name
            );
            assert!(
                !reads_as_identifier(&shown),
                "effect {} is shown as {shown:?}",
                entry.id
            );
            if entry.name.chars().filter(|c| c.is_uppercase()).count() > 1 {
                assert_ne!(
                    shown, entry.name,
                    "effect {} shows its identifier",
                    entry.id
                );
            }
        }
        assert!(OWN_NAMES.iter().all(|(_, n)| !reads_as_identifier(n)));
    }
}
