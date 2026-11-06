//! Hero voices: what a hero says to its own player, from the install's own response rules.
//!
//! The importer reads each hero's rules (its `scripts/talker/response_rules_<hero>.vrr` and the
//! files it includes), keeps the rules a query of this client can match ([`concept`]), and packs
//! them with the voice events and sounds of their lines into one cache file per hero
//! ([`pack_path`], [`pack`]). The client reads only the pack of the hero it speaks for, off its
//! frame's thread, keeps its compressed sounds resident and answers its queries with a
//! [`speaker::Speaker`].

pub mod concept;
pub mod expr;
pub mod pack;
pub mod rules;
pub mod speaker;

use rules::HeroRules;

/// Where a hero's voice pack is in the cache.
pub fn pack_path(hero: &str) -> String {
    format!("sounds/voice/{hero}.bin")
}

/// The name of the rules record of a pack.
pub const RULES: &str = "rules.json";
/// The name of the record of the mixer lines of the pack's voice events
/// ([`crate::text::sound_events_txt`]).
pub const EVENTS: &str = "events.txt";

/// The record name of a pack's sound `index`, by its kind (`mp3`, `wav`).
pub fn sound_record(index: usize, extension: &str) -> String {
    format!("sound/{index}.{extension}")
}

/// The record name of the lip sync of a pack's sound `index` ([`crate::text::lipsync_txt`]).
pub fn lipsync_record(index: usize) -> String {
    format!("lipsync/{index}.txt")
}

/// One sound of a pack: its compressed audio and its lip sync, if it has one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound<T> {
    pub audio: T,
    pub lipsync: Option<T>,
}

/// A pack's contents, checked: its rules, its events' mixer lines and its sounds, each sound's
/// audio and lip sync as `T` (a slice of the pack, or a record handed over on its own).
#[derive(Clone, Debug, PartialEq)]
pub struct Voice<T> {
    pub rules: HeroRules,
    pub events: String,
    pub sounds: Vec<Sound<T>>,
}

/// The sound index and kind a record name says, for a sound or a lip sync.
fn indexed<'a>(name: &'a str, folder: &str) -> Option<(usize, &'a str)> {
    let (stem, extension) = name.strip_prefix(folder)?.split_once('.')?;
    let index: usize = stem.parse().ok()?;
    // `07` and `+7` would name 7 twice.
    (index.to_string() == stem).then_some((index, extension))
}

impl<T: AsRef<[u8]>> Voice<T> {
    /// The contents of a pack's records, each its name and data: the rules, the events, and
    /// sounds numbered from 0 without a gap, each with at most one lip sync, every sound the
    /// rules name present. `hero` is the hero whose pack it must be.
    pub fn assemble(
        hero: &str,
        records: impl IntoIterator<Item = (String, T)>,
    ) -> Result<Voice<T>, String> {
        let mut rules = None;
        let mut events = None;
        let mut audio: Vec<Option<T>> = Vec::new();
        let mut lipsync: Vec<(usize, T)> = Vec::new();
        for (name, data) in records {
            if name == RULES {
                rules = Some(data);
            } else if name == EVENTS {
                let text = std::str::from_utf8(data.as_ref())
                    .map_err(|_| format!("{EVENTS} is not text"))?;
                events = Some(text.to_owned());
            } else if let Some((index, kind)) = indexed(&name, "sound/") {
                if !matches!(kind, "mp3" | "wav") || index >= pack::MAX_RECORDS {
                    return Err(format!("record {name} is no sound of a pack"));
                }
                if audio.len() <= index {
                    audio.resize_with(index + 1, || None);
                }
                audio[index] = Some(data);
            } else if let Some((index, "txt")) = indexed(&name, "lipsync/") {
                lipsync.push((index, data));
            } else {
                return Err(format!("record {name} is not one of a pack"));
            }
        }
        let audio: Vec<T> = audio
            .into_iter()
            .enumerate()
            .map(|(i, a)| a.ok_or_else(|| format!("sound {i} is missing")))
            .collect::<Result<_, _>>()?;
        let mut sounds: Vec<Sound<T>> = audio
            .into_iter()
            .map(|audio| Sound {
                audio,
                lipsync: None,
            })
            .collect();
        for (index, data) in lipsync {
            let sound = sounds
                .get_mut(index)
                .ok_or_else(|| format!("the lip sync of sound {index}, which is missing"))?;
            sound.lipsync = Some(data);
        }
        let rules = rules.ok_or_else(|| format!("no {RULES}"))?;
        let rules = HeroRules::parse(rules.as_ref(), sounds.len())?;
        if rules.hero != hero {
            return Err(format!("the pack is {}'s, not {hero}'s", rules.hero));
        }
        Ok(Voice {
            rules,
            events: events.ok_or_else(|| format!("no {EVENTS}"))?,
            sounds,
        })
    }
}

/// The contents of a whole pack's bytes, every sound a slice of them.
pub fn open<'a>(hero: &str, bytes: &'a [u8]) -> Result<Voice<&'a [u8]>, String> {
    let records = pack::read(bytes).map_err(|e| e.to_string())?;
    Voice::assemble(
        hero,
        records
            .into_iter()
            .map(|record| (record.name, &bytes[record.data])),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pack_of(rules: &HeroRules, sounds: usize) -> Vec<u8> {
        let json = rules.to_json();
        let names: Vec<String> = (0..sounds).map(|i| sound_record(i, "mp3")).collect();
        let mut records: Vec<(&str, &[u8])> = vec![(RULES, &json), (EVENTS, b"")];
        records.extend(names.iter().map(|n| (n.as_str(), &b"ID3"[..])));
        pack::write(records).unwrap()
    }

    #[test]
    fn a_pack_opens_with_every_sound_its_rules_name() {
        let rules = rules::tests::sample();
        let bytes = pack_of(&rules, 1);
        let voice = open("pudge", &bytes).unwrap();
        assert_eq!(voice.rules, rules);
        assert_eq!(
            voice.sounds,
            [Sound {
                audio: &b"ID3"[..],
                lipsync: None
            }]
        );
    }

    #[test]
    fn a_pack_missing_a_sound_its_rules_name_is_refused() {
        let error = open("pudge", &pack_of(&rules::tests::sample(), 0)).unwrap_err();
        assert_eq!(error, "rules.json: a sound index out of its list");
    }

    #[test]
    fn another_heros_pack_is_refused() {
        let error = open("nevermore", &pack_of(&rules::tests::sample(), 1)).unwrap_err();
        assert_eq!(error, "the pack is pudge's, not nevermore's");
    }

    #[test]
    fn sound_names_name_each_index_once() {
        assert_eq!(indexed("sound/7.mp3", "sound/"), Some((7, "mp3")));
        assert_eq!(indexed("sound/07.mp3", "sound/"), None);
        assert_eq!(indexed("sound/+7.mp3", "sound/"), None);
    }
}
