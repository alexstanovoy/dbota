//! Reading the pack of the hero spoken for, off the frame's thread: on the desktop a task of
//! the compute pool reads, checks and splits it; in a browser the page's voice worker
//! (`web/voice_worker.js`) reads and checks it and hands each record over, its buffer moved, not
//! copied ([`crate::webvoice`]). Either way the frame only takes what is ready: the sounds as
//! assets (their compressed bytes, decoded as they play), the rules, the mixer's lines.

use std::sync::Arc;

use bevy::audio::AudioSource;
use bevy::prelude::*;
use dbota_import::voice::rules::HeroRules;
use dbota_import::voice::{Voice, pack_path};

use super::{HeroVoice, Pack, Resident};
use crate::face::Lipsync;
use crate::manifest::Manifest;
use crate::mixer::Mixer;
use crate::talk::LineFace;

/// A pack read and checked, its sounds' bytes and faces ready to be assets.
pub struct Loaded {
    pub rules: HeroRules,
    pub events: String,
    pub sounds: Vec<(Arc<[u8]>, Option<Arc<LineFace>>)>,
    pub bytes: usize,
}

/// The reading of a pack under way.
pub enum Job {
    #[cfg(not(target_arch = "wasm32"))]
    Task(bevy::tasks::Task<Result<Loaded, String>>),
    #[cfg(target_arch = "wasm32")]
    Worker,
}

/// The sounds and faces of a pack's contents, each sound's bytes taken as they are when they
/// are a buffer of their own already (a browser's record) and copied once out of the pack
/// otherwise. A lip sync that does not read leaves its line's face still and is told.
pub fn loaded<T: AsRef<[u8]> + Into<Arc<[u8]>>>(voice: Voice<T>, file: &str) -> Loaded {
    let mut bytes = 0;
    let mut broken = 0;
    let sounds = voice
        .sounds
        .into_iter()
        .enumerate()
        .map(|(i, sound)| {
            bytes += sound.audio.as_ref().len();
            let face = sound.lipsync.as_ref().and_then(|text| {
                bytes += text.as_ref().len();
                let lipsync = std::str::from_utf8(text.as_ref())
                    .ok()
                    .and_then(Lipsync::parse);
                broken += usize::from(lipsync.is_none());
                lipsync.map(|lipsync| {
                    Arc::new(LineFace {
                        lipsync,
                        file: format!("{file}#{}", dbota_import::voice::lipsync_record(i)),
                    })
                })
            });
            (sound.audio.into(), face)
        })
        .collect();
    if broken > 0 {
        warn!("voice: {file}: {broken} lip syncs do not read; those lines keep a still face");
    }
    Loaded {
        rules: voice.rules,
        events: voice.events,
        sounds,
        bytes,
    }
}

/// Starts reading `hero`'s pack.
pub fn start(manifest: &Manifest, hero: &str) -> Job {
    let rel = pack_path(hero);
    #[cfg(not(target_arch = "wasm32"))]
    {
        let path = manifest.root.join(&rel);
        let hero = hero.to_owned();
        Job::Task(
            bevy::tasks::AsyncComputeTaskPool::get().spawn(async move { read(&path, &hero, &rel) }),
        )
    }
    #[cfg(target_arch = "wasm32")]
    {
        let url = manifest.root.join(&rel);
        crate::webvoice::request(hero, &rel, &url.to_string_lossy());
        Job::Worker
    }
}

/// Reads, checks and splits a pack on the desktop.
#[cfg(not(target_arch = "wasm32"))]
fn read(path: &std::path::Path, hero: &str, rel: &str) -> Result<Loaded, String> {
    use std::io::Read;
    let limit = dbota_import::voice::pack::MAX_PACK_BYTES;
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path)
        .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes));
    match read {
        Ok(_) => {}
        // Every cache of this build has every hero's pack.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!(
                "{rel} is not in the asset cache: the cache was made by another dbota build of \
                 import revision {}; import the assets again",
                dbota_import::import::IMPORT_REV
            ));
        }
        Err(e) => return Err(format!("{rel}: {e}")),
    }
    if bytes.len() > limit {
        return Err(format!(
            "{rel}: more than the {limit} bytes a hero's voice may hold"
        ));
    }
    let voice = dbota_import::voice::open(hero, &bytes).map_err(|e| format!("{rel}: {e}"))?;
    Ok(loaded(voice, rel))
}

/// What a finished read gave, if it has finished.
fn poll(job: &mut Job, hero: &str) -> Option<Result<Loaded, String>> {
    match job {
        #[cfg(not(target_arch = "wasm32"))]
        Job::Task(task) => {
            use bevy::tasks::{block_on, futures_lite::future::poll_once};
            let _ = hero;
            block_on(poll_once(task))
        }
        #[cfg(target_arch = "wasm32")]
        Job::Worker => crate::webvoice::take(hero).map(|records| {
            let rel = pack_path(hero);
            let voice = Voice::assemble(hero, records?).map_err(|e| format!("{rel}: {e}"))?;
            Ok(loaded(voice, &rel))
        }),
    }
}

/// Takes a finished read: the sounds become assets, the mixer takes the lines' events, the
/// hero starts with nothing asked.
pub fn receive(
    mut voice: ResMut<HeroVoice>,
    mut audio: ResMut<Assets<AudioSource>>,
    mut mixer: ResMut<Mixer>,
) {
    let Pack::Loading { hero, job } = &mut voice.pack else {
        return;
    };
    let Some(done) = poll(job, hero) else {
        return;
    };
    let hero = std::mem::take(hero);
    voice.pack = match done {
        Ok(loaded) => {
            let events = mixer.set_voice_events(&loaded.events);
            let lines = loaded.rules.lines();
            info!(
                "voice: {hero}'s pack is resident: {lines} lines of {} rules, {} sounds, {} KiB, \
                 {events} events; pruned at import: {:?}",
                loaded.rules.rules.len(),
                loaded.sounds.len(),
                loaded.bytes / 1024,
                loaded.rules.pruned
            );
            let (sounds, faces) = loaded
                .sounds
                .into_iter()
                .map(|(bytes, face)| (audio.add(AudioSource { bytes }), face))
                .unzip();
            Pack::Resident(Box::new(Resident::new(hero, loaded.rules, sounds, faces)))
        }
        Err(why) => {
            error!("voice: {hero} stays silent: {why}");
            Pack::Failed { hero }
        }
    };
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("target/e2e/voice-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch folder");
        dir
    }

    /// A cache of the same import revision made by a build without voice packs (or with
    /// another layout of them) says so, and how to mend it, instead of leaving the hero silent
    /// unexplained.
    #[test]
    fn a_stale_cache_of_the_same_revision_fails_loudly() {
        let dir = scratch("stale");
        let rel = pack_path("pudge");
        let missing = read(&dir.join(&rel), "pudge", &rel).err().expect("refused");
        assert!(missing.contains("import the assets again"), "{missing}");
        let old = dir.join("old.bin");
        std::fs::write(&old, b"DBVOICE\x00\0\0\0\0").expect("written");
        let other = read(&old, "pudge", &rel).err().expect("refused");
        assert!(other.contains("import the assets again"), "{other}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
