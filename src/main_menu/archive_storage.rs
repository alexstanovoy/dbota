//! A small index and independently retained raw metadata. A new match writes its blob once;
//! pruning replay bytes never touches these records. All filenames are content digests.
use super::{
    local_matches::Saved,
    store::{Entry, SettingsStore},
};
use serde::{Deserialize, Serialize};
use std::io;

pub const MAX_MATCHES: usize = 50;
pub const META_LIMIT: usize =
    bota_replay::limits::MAX_META_BYTES + bota_replay::limits::END_RESERVE_BYTES;

#[derive(Serialize, Deserialize)]
struct Stored {
    entry: Saved,
    digest: String,
}

fn index(store: &SettingsStore) -> Vec<Stored> {
    store
        .read(Entry::MatchHistory)
        .and_then(|text| serde_json::from_str::<Index>(&text).ok())
        .map_or_else(Vec::new, |index| index.0)
}

// Reject an index above the policy before decoding any metadata, rather than decode thousands
// of recordings and truncate the result afterward.
struct Index(Vec<Stored>);
impl<'de> Deserialize<'de> for Index {
    fn deserialize<D: serde::Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Index;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "at most {MAX_MATCHES} local matches")
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> Result<Index, A::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = seq.next_element()? {
                    if entries.len() == MAX_MATCHES {
                        return Err(serde::de::Error::custom("too many local matches"));
                    }
                    entries.push(entry);
                }
                Ok(Index(entries))
            }
        }
        decoder.deserialize_seq(Visitor)
    }
}

pub fn load(store: &SettingsStore, max_count: usize) -> Vec<Saved> {
    index(store)
        .into_iter()
        .take(max_count.min(MAX_MATCHES))
        .map(|mut stored| {
            let key = valid_digest(&stored.digest).then_some(stored.digest);
            let bytes = key.as_ref().and_then(|key| store.read_metadata(key));
            stored.entry.metadata = bytes
                .filter(|bytes| {
                    key.as_ref()
                        .is_some_and(|key| blake3::hash(bytes).to_hex().as_str() == key)
                })
                .unwrap_or_default();
            stored.entry.metadata_key = key;
            stored.entry
        })
        .collect()
}

pub fn save(store: &SettingsStore, entries: &[Saved]) -> io::Result<()> {
    let previous = index(store);
    let digests = write_snapshot(
        entries,
        cfg!(target_arch = "wasm32"),
        |digest, bytes| store.write_metadata(digest, bytes),
        |text| store.write(Entry::MatchHistory, text),
    )?;
    for old in previous {
        if !digests.contains(&old.digest) {
            store.remove_metadata(&old.digest);
        }
    }
    Ok(())
}

pub(super) fn write_snapshot(
    entries: &[Saved],
    best_effort_blobs: bool,
    mut write_blob: impl FnMut(&str, &[u8]) -> io::Result<()>,
    mut write_index: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<Vec<String>> {
    #[derive(Serialize)]
    struct Record<'a> {
        entry: &'a Saved,
        digest: String,
    }
    let mut records = Vec::with_capacity(entries.len());
    for entry in entries.iter().take(MAX_MATCHES) {
        let digest = if entry.metadata.is_empty() {
            entry
                .metadata_key
                .clone()
                .unwrap_or_else(|| blake3::hash(&[]).to_hex().to_string())
        } else {
            blake3::hash(&entry.metadata).to_hex().to_string()
        };
        if !entry.metadata.is_empty()
            && let Err(error) = write_blob(&digest, &entry.metadata)
        {
            if !best_effort_blobs {
                return Err(error);
            }
            bevy::log::warn!("Could not retain match details; keeping its history card: {error}");
        }
        records.push(Record { entry, digest });
    }
    let text = serde_json::to_string(&records).map_err(io::Error::other)?;
    if text.len() > super::local_matches::MAX_HISTORY_BYTES {
        return Err(io::Error::other("local match index exceeds its byte limit"));
    }
    write_index(&text)?;
    Ok(records.into_iter().map(|record| record.digest).collect())
}

fn valid_digest(digest: &str) -> bool {
    digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(not(target_arch = "wasm32"))]
impl SettingsStore {
    fn metadata_path(&self, digest: &str) -> Option<std::path::PathBuf> {
        valid_digest(digest).then_some(())?;
        Some(
            self.path(Entry::MatchHistory)?
                .parent()?
                .join("match_metadata")
                .join(digest),
        )
    }
    fn read_metadata(&self, digest: &str) -> Option<Vec<u8>> {
        use io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(self.metadata_path(digest)?)
            .ok()?
            .take(META_LIMIT as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        (bytes.len() <= META_LIMIT).then_some(bytes)
    }
    fn write_metadata(&self, digest: &str, bytes: &[u8]) -> io::Result<()> {
        use io::Write;
        let Some(path) = self.metadata_path(digest) else {
            return Ok(());
        };
        if path.exists()
            && self
                .read_metadata(digest)
                .is_some_and(|bytes| blake3::hash(&bytes).to_hex().as_str() == digest)
        {
            return Ok(());
        }
        if bytes.len() > META_LIMIT {
            return Err(io::Error::other("metadata exceeds its byte limit"));
        }
        std::fs::create_dir_all(path.parent().expect("metadata parent"))?;
        let fresh = path.with_extension("new");
        let _ = std::fs::remove_file(&fresh);
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&fresh)?.write_all(bytes)?;
        std::fs::rename(fresh, path)
    }
    fn remove_metadata(&self, digest: &str) {
        if let Some(path) = self.metadata_path(digest) {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[cfg(target_arch = "wasm32")]
impl SettingsStore {
    fn metadata_key(digest: &str) -> Option<String> {
        valid_digest(digest).then(|| format!("dbota.match_metadata.{digest}"))
    }
    fn read_metadata(&self, digest: &str) -> Option<Vec<u8>> {
        use base64::Engine;
        let text = Self::storage()?
            .get_item(&Self::metadata_key(digest)?)
            .ok()??;
        if text.len() > META_LIMIT.div_ceil(3) * 4 {
            return None;
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(text)
            .ok()?;
        (bytes.len() <= META_LIMIT).then_some(bytes)
    }
    fn write_metadata(&self, digest: &str, bytes: &[u8]) -> io::Result<()> {
        use base64::Engine;
        let key = Self::metadata_key(digest)
            .ok_or_else(|| io::Error::other("invalid metadata digest"))?;
        if bytes.len() > META_LIMIT {
            return Err(io::Error::other("metadata exceeds its byte limit"));
        }
        let storage = Self::storage().ok_or_else(|| io::Error::other("no localStorage"))?;
        if storage.get_item(&key).ok().flatten().is_some() {
            return Ok(());
        }
        storage
            .set_item(
                &key,
                &base64::engine::general_purpose::STANDARD.encode(bytes),
            )
            .map_err(|_| io::Error::other("localStorage refused match metadata"))
    }
    fn remove_metadata(&self, digest: &str) {
        if let (Some(storage), Some(key)) = (Self::storage(), Self::metadata_key(digest)) {
            let _ = storage.remove_item(&key);
        }
    }
}
