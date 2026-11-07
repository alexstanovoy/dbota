//! A hero's files as its voice pack reads them: every read, decode and copy the pack's making
//! takes is charged to one budget of the hero's, before it is allocated. Reading goes through
//! [`HeroFiles`] alone: the voice importer is given no package to read from, and each reader of
//! heatel it uses takes a budget, the hero's being the only one it holds.

use heatel::sound::{
    Lipsync, Sound, SoundEvent, SoundMixer, SoundStack, read_sound_events, read_sound_mixer,
    read_sound_stacks,
};
use heatel::talker::{ResponseRules, read_response_rules};
use heatel::{Budget, Resource, Result, Source, Vpk};

use super::super::{SOUND_MIXER_SRC, SOUND_STACKS, malformed, read_from};

/// Most bytes making one hero's voice pack may take: every file read, decoded and kept, and
/// nothing given back. Pudge's take 32,780,572 (a 2,639,400-byte pack); a pack at its own bound
/// (32 MiB) would take some 80 MiB.
const MAX_HERO_BYTES: usize = 128 << 20;
/// Most bytes of one rule file: the install's largest (Snapfire's) is 72,627.
const MAX_RULE_FILE_BYTES: u64 = 1 << 20;
/// Most bytes of one sound event script or hero script: the install's largest voice script
/// (Snapfire's) is 120,888, its largest hero script (Invoker's) 60,111.
pub(super) const MAX_SCRIPT_BYTES: u64 = 2 << 20;
/// Most bytes of the sound stacks or the mixer: the install's are 11,477 and 16,257 (stacks)
/// and 24,179 (mixer).
const MAX_SHARED_BYTES: u64 = 4 << 20;
/// Most bytes of one sound: the install's largest voice line is 2,118,182.
const MAX_SOUND_BYTES: u64 = 4 << 20;
/// What a WAV made of a sound's samples takes besides them: its header.
const WAV_HEADER: usize = 64;

/// A sound read for a pack: its audio, its kind, its lip sync or why it has none, its seconds.
pub(super) struct ReadSound {
    pub audio: Vec<u8>,
    pub kind: &'static str,
    pub lipsync: Result<Lipsync>,
    pub seconds: f32,
}

/// The package a hero's voice pack is read from, and the budget all its reading draws on.
pub(in crate::import) struct HeroFiles<'a, S> {
    src: &'a S,
    pak: &'a Vpk,
    budget: Budget,
}

impl<'a, S: Source> HeroFiles<'a, S> {
    pub(in crate::import) fn new(src: &'a S, pak: &'a Vpk) -> Self {
        Self {
            src,
            pak,
            budget: Budget::new(MAX_HERO_BYTES),
        }
    }

    /// The budget, for what the importer copies of what it read.
    pub(super) fn budget(&mut self) -> &mut Budget {
        &mut self.budget
    }

    /// The stored size of `path`, if the package holds it.
    pub(super) fn size(&self, path: &str) -> Option<u64> {
        self.pak.get(path).map(|entry| entry.size())
    }

    /// The bytes of `path`, refused before they are read when the package holds more than
    /// `max`, and charged before they are read.
    pub(super) fn bytes(&mut self, path: &str, max: u64) -> Result<Vec<u8>> {
        let size = self
            .size(path)
            .ok_or_else(|| heatel::Error::NotFound(path.to_owned()))?;
        if size > max {
            return Err(malformed(
                "voice",
                format!("{path}: {size} bytes, at most {max}"),
            ));
        }
        self.within(path, |budget| budget.charge(size as usize))?;
        read_from(self.src, &[self.pak], path)
    }

    /// What `read` makes within the budget; its error names `path`.
    fn within<T>(&mut self, path: &str, read: impl FnOnce(&mut Budget) -> Result<T>) -> Result<T> {
        read(&mut self.budget).map_err(|e| malformed("voice", format!("{path}: {e}")))
    }

    /// The response rules of the rule file at `path`.
    pub(super) fn rules(&mut self, path: &str) -> Result<ResponseRules> {
        let bytes = self.bytes(path, MAX_RULE_FILE_BYTES)?;
        let resource = Resource::parse(&bytes)?;
        self.within(path, |budget| read_response_rules(&resource, budget))
    }

    /// The events of the sound event script at `path`.
    pub(super) fn events(&mut self, path: &str) -> Result<Vec<SoundEvent>> {
        let bytes = self.bytes(path, MAX_SCRIPT_BYTES)?;
        let resource = Resource::parse(&bytes)?;
        self.within(path, |budget| read_sound_events(&resource, budget))
    }

    /// The event types of the install's sound stacks.
    pub(super) fn stacks(&mut self) -> Result<Vec<SoundStack>> {
        let mut stacks = Vec::new();
        for path in SOUND_STACKS {
            let bytes = self.bytes(path, MAX_SHARED_BYTES)?;
            let resource = Resource::parse(&bytes)?;
            let read = self.within(path, |budget| read_sound_stacks(&resource, budget))?;
            self.within(path, |budget| {
                budget.charge(read.len() * size_of::<SoundStack>())
            })?;
            stacks.extend(read);
        }
        Ok(stacks)
    }

    /// The install's mixer.
    pub(super) fn mixer(&mut self) -> Result<SoundMixer> {
        let bytes = self.bytes(SOUND_MIXER_SRC, MAX_SHARED_BYTES)?;
        self.within(SOUND_MIXER_SRC, |budget| read_sound_mixer(&bytes, budget))
    }

    /// The sound at `path` (a `.vsnd_c`), its audio copied out of the file it was read in.
    pub(super) fn sound(&mut self, path: &str) -> Result<ReadSound> {
        let bytes = self.bytes(path, MAX_SOUND_BYTES)?;
        let sound = self.within(path, |budget| Sound::parse(&bytes, budget))?;
        let lipsync = Lipsync::parse(&bytes, &mut self.budget);
        // The audio is a part of the file, a WAV a header besides.
        self.within(path, |budget| budget.charge(bytes.len() + WAV_HEADER))?;
        Ok(ReadSound {
            audio: sound.file().into_owned(),
            kind: sound.extension(),
            lipsync,
            seconds: sound.duration(),
        })
    }
}
