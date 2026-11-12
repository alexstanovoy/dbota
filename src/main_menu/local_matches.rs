//! Durable, local metadata for the matches this client played. Replay bytes may be pruned
//! independently; personal attribution is an explicit slot, never a nickname or seat guess.
use super::{
    history::{Match, ResultKind},
    store::SettingsStore,
};
use crate::{postgame::summary::MatchSummary, replays::Record};
use bota_proto::SlotId;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc};

pub const MAX_HISTORY_BYTES: usize = 4 << 20;
const MAX_MATCHES: usize = super::archive_storage::MAX_MATCHES;

#[derive(Clone, Serialize, Deserialize)]
pub struct CachedCard {
    row: Match,
    team: Option<bota_proto::Team>,
    #[serde(default)]
    finished: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Saved {
    pub key: String,
    pub file: Option<String>,
    pub when: u64,
    pub mode: String,
    pub owner: Option<SlotId>,
    #[serde(skip)]
    pub metadata: Vec<u8>,
    #[serde(skip)]
    pub metadata_key: Option<String>,
    #[serde(default)]
    pub cached: Option<CachedCard>,
    /// The one outcome delivered to this participant, retained locally for result details.
    pub result: Option<bota_radovid::proto::MatchResult>,
    #[serde(skip)]
    pub summary: Option<Arc<MatchSummary>>,
    /// Only a locally created metadata recording, removed after durable archival.
    #[serde(skip)]
    pub temporary: Option<std::path::PathBuf>,
}

impl Saved {
    fn prepare(&mut self) -> bool {
        if self.key.len() > 8192
            || self.mode.len() > 96
            || self
                .file
                .as_ref()
                .is_some_and(|file| !crate::replays::valid_name(file))
            || self
                .owner
                .is_some_and(|slot| usize::from(slot.0) >= bota_proto::limits::MAX_SEATS)
        {
            return false;
        }
        if self.result.as_ref().is_some_and(|result| {
            result.seats.len() > bota_proto::limits::MAX_SEATS
                || result.seats.iter().any(|seat| {
                    seat.before
                        .as_ref()
                        .into_iter()
                        .chain(seat.after.as_ref())
                        .any(|rating| !rating.rating.is_finite() || !rating.deviation.is_finite())
                })
        }) {
            return false;
        }
        if let Some(result) = &mut self.result {
            result.game_report.clear();
        }
        if self.metadata.is_empty() {
            return self.cached.is_some();
        }
        let Ok(meta) = bota_replay::meta::decode(&self.metadata) else {
            return false;
        };
        if meta.header.map == bota_core::DEMO_ID.0
            || self
                .result
                .as_ref()
                .is_some_and(|result| result.match_id.0 != meta.header.match_id)
        {
            return false;
        }
        if self.summary.is_none() {
            self.summary = MatchSummary::from_meta(&meta).map(Arc::new);
        }
        self.refresh_card();
        self.summary.is_some()
    }

    fn refresh_card(&mut self) {
        if self.summary.is_some() {
            self.cached = Some(CachedCard {
                row: self.card(&[]),
                team: self.owned_team(),
                finished: self
                    .summary
                    .as_ref()
                    .is_some_and(|summary| summary.winner.is_some()),
            });
        }
    }

    pub fn owned_hero(&self) -> Option<bota_proto::HeroId> {
        let owner = self.owner?;
        match &self.summary {
            Some(summary) => summary
                .rows
                .iter()
                .find(|row| row.slot == owner)
                .map(|row| row.hero),
            None => self.cached.as_ref().and_then(|cached| cached.row.hero),
        }
    }

    pub fn owned_team(&self) -> Option<bota_proto::Team> {
        let owner = self.owner?;
        match &self.summary {
            Some(summary) => summary
                .rows
                .iter()
                .find(|row| row.slot == owner)
                .map(|row| row.team),
            None => self.cached.as_ref().and_then(|cached| cached.team),
        }
    }

    pub fn completed_team(&self) -> Option<bota_proto::Team> {
        let finished = self.summary.as_ref().map_or_else(
            || self.cached.as_ref().is_some_and(|cached| cached.finished),
            |summary| summary.winner.is_some(),
        );
        if finished { self.owned_team() } else { None }
    }

    /// Only a finished recording opens every seat's private post-game facts.
    pub fn details(&self) -> Option<MatchSummary> {
        let mut summary = self.summary.as_deref()?.clone();
        if summary.winner.is_none() {
            summary.rows.retain(|row| Some(row.slot) == self.owner);
            summary.points.clear();
            summary.structures.clear();
            if let Some(recording) = &summary.recording {
                summary.recording = Some(Arc::new(
                    recording.restricted_to(summary.rows.iter().map(|row| row.slot)),
                ));
            }
        }
        Some(summary)
    }

    pub fn card(&self, records: &[Record]) -> Match {
        let watchable = self.file.as_ref().is_some_and(|file| {
            records
                .iter()
                .any(|record| record.name == *file && record.bytes >= crate::replays::MIN_BYTES)
        });
        if self.summary.is_none()
            && let Some(cached) = &self.cached
        {
            let mut row = cached.row.clone();
            row.file = self.key.clone();
            row.when = self.when;
            row.mode = self.mode.clone();
            row.watchable = watchable;
            row.details_available = false;
            if self.owner.is_none() {
                row.hero = None;
                row.kda = None;
                row.net_worth = None;
            }
            return row;
        }
        let summary = self.summary.as_deref();
        let row = summary.and_then(|summary| {
            self.owner
                .and_then(|slot| summary.rows.iter().find(|row| row.slot == slot))
        });
        let result = match (summary.and_then(|summary| summary.winner), row) {
            (Some(bota_proto::Team::Neutral), _) => ResultKind::Draw,
            (Some(winner), Some(row)) if winner == row.team => ResultKind::Won,
            (Some(_), Some(_)) => ResultKind::Lost,
            _ => ResultKind::Undecided,
        };
        Match {
            file: self.key.clone(),
            when: self.when,
            mode: self.mode.clone(),
            hero: row.map(|row| row.hero),
            result,
            seconds: summary.map(|summary| summary.seconds),
            kda: row.map(|row| (row.kills, row.deaths, row.assists)),
            net_worth: row.and_then(|row| row.net_worth),
            watchable,
            details_available: self.summary.is_some(),
        }
    }
}

#[derive(Default)]
pub struct Archive {
    entries: Vec<Saved>,
    dirty: bool,
    retry_save: Option<bevy::platform::time::Instant>,
    #[cfg(not(target_arch = "wasm32"))]
    loading: Option<std::sync::mpsc::Receiver<Vec<Saved>>>,
    revision: u64,
    cards: std::cell::RefCell<CardCache>,
}

#[derive(Default)]
struct CardCache {
    revision: u64,
    records: Vec<Record>,
    cards: Arc<Vec<Match>>,
}

impl Archive {
    pub fn load(store: &SettingsStore, max_count: usize) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            let store = store.clone();
            let spawned = std::thread::Builder::new()
                .name("local-match-history".to_owned())
                .spawn(move || {
                    let entries = Self::read(&store, max_count);
                    let _ = send.send(entries);
                });
            if let Err(why) = spawned {
                bevy::log::warn!("Could not load local match history: {why}");
                return Self::default();
            }
            Self {
                loading: Some(receive),
                ..Default::default()
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            // Browser records are bounded to the same fifty-entry policy. localStorage is
            // synchronous; no worker can access it. No replay bytes are read here.
            Self {
                entries: Self::read(store, max_count),
                revision: 1,
                ..Default::default()
            }
        }
    }

    fn read(store: &SettingsStore, max_count: usize) -> Vec<Saved> {
        let mut seen = HashSet::new();
        let mut entries: Vec<_> = super::archive_storage::load(store, max_count)
            .into_iter()
            .filter_map(|mut entry| {
                (entry.prepare() && entry.mode != "demo" && seen.insert(entry.key.clone()))
                    .then_some(entry)
            })
            .collect();
        entries.sort_by_key(|entry| std::cmp::Reverse(entry.when));
        entries
    }

    fn poll(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(loading) = &self.loading {
            match loading.try_recv() {
                Ok(entries) => {
                    self.loading = None;
                    if self.entries.is_empty() {
                        self.entries = entries;
                        self.revision = self.revision.wrapping_add(1);
                    } else {
                        let recent = std::mem::replace(&mut self.entries, entries);
                        self.revision = self.revision.wrapping_add(1);
                        for entry in recent {
                            self.insert(entry, MAX_MATCHES);
                        }
                        self.dirty = true;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.loading = None,
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    pub fn insert(&mut self, mut entry: Saved, max_count: usize) {
        if entry.mode == "demo" || !entry.prepare() {
            return;
        }
        // The replay and its separately delivered metadata share an explicit file or service
        // key. Equal stats are not identity: two independent matches can have the same bytes.
        let candidates: Vec<_> = self
            .entries
            .iter()
            .enumerate()
            .filter(|(_, previous)| {
                previous.key == entry.key
                    || entry
                        .file
                        .as_ref()
                        .is_some_and(|file| previous.file.as_ref() == Some(file))
            })
            .map(|(index, _)| index)
            .collect();
        self.preserve_previous(&mut entry, &candidates);
        entry.refresh_card();
        if let Some(result) = &entry.result {
            entry.when = result.finished_at.0 / 1000;
        }
        if let [index] = candidates.as_slice() {
            let previous = &self.entries[*index];
            if previous.metadata == entry.metadata
                && previous.file == entry.file
                && previous.owner == entry.owner
                && previous.result == entry.result
            {
                return;
            }
        }
        for index in candidates.into_iter().rev() {
            self.entries.remove(index);
        }
        self.entries.push(entry);
        self.entries
            .sort_by_key(|entry| std::cmp::Reverse(entry.when));
        self.entries.truncate(max_count.min(MAX_MATCHES));
        self.dirty = true;
        self.revision = self.revision.wrapping_add(1);
    }

    fn preserve_previous(&self, entry: &mut Saved, candidates: &[usize]) {
        for &index in candidates {
            let previous = &self.entries[index];
            if entry.result.is_none() {
                entry.result = previous.result.clone();
                entry.key = previous.key.clone();
            }
            if entry.file.is_none() {
                entry.file = previous.file.clone();
            }
            if previous
                .summary
                .as_ref()
                .is_some_and(|summary| summary.winner.is_some())
                && entry
                    .summary
                    .as_ref()
                    .is_none_or(|summary| summary.winner.is_none())
            {
                entry.metadata = previous.metadata.clone();
                entry.summary = previous.summary.clone();
            }
        }
    }

    pub fn cards(&self, records: &[Record]) -> Arc<Vec<Match>> {
        let mut cache = self.cards.borrow_mut();
        if cache.revision != self.revision || cache.records != records {
            cache.cards = Arc::new(
                self.entries
                    .iter()
                    .map(|entry| entry.card(records))
                    .collect(),
            );
            cache.revision = self.revision;
            cache.records = records.to_vec();
        }
        Arc::clone(&cache.cards)
    }

    pub(super) fn revision(&self) -> u64 {
        self.revision
    }

    pub fn entries(&self) -> &[Saved] {
        &self.entries
    }

    pub fn find(&self, key: &str) -> Option<&Saved> {
        self.entries.iter().find(|entry| entry.key == key)
    }

    /// No frame-by-frame writes: one bounded snapshot after a match or a library change.
    pub fn save(&mut self, store: &SettingsStore) -> std::io::Result<()> {
        self.poll();
        #[cfg(not(target_arch = "wasm32"))]
        if self.loading.is_some() {
            return Ok(());
        }
        let now = bevy::platform::time::Instant::now();
        if !self.dirty || self.retry_save.is_some_and(|retry| now < retry) {
            return Ok(());
        }
        let result = super::archive_storage::save(store, &self.entries);
        if result.is_ok() && store.available() {
            for entry in &mut self.entries {
                if let Some(path) = entry.temporary.take()
                    && let Err(why) = std::fs::remove_file(path)
                {
                    bevy::log::warn!("Could not remove archived temporary metadata: {why}");
                }
            }
        }
        self.dirty = result.is_err();
        self.retry_save = result
            .as_ref()
            .err()
            .map(|_| now + std::time::Duration::from_secs(30));
        result
    }
}

impl super::core::MenuCore {
    /// The newest readable local match with an explicitly owned hero. Spectated and
    /// unattributed entries are skipped; unreadable recordings never enter the archive.
    /// Sandbox sessions never supply a fallback.
    pub fn last_played_hero(&self) -> Option<bota_proto::HeroId> {
        self.local_history
            .entries
            .iter()
            .find_map(Saved::owned_hero)
    }

    pub(super) fn update_local_history(&mut self) {
        self.local_meta.retain(self.library.records());
        #[cfg(not(target_arch = "wasm32"))]
        for record in self.library.records() {
            self.local_meta
                .request(record, || self.library.path(&record.name));
        }
        for entry in self.local_meta.poll() {
            self.local_history
                .insert(entry, self.library.policy().max_count);
        }
        if let Err(why) = self.local_history.save(&self.store) {
            bevy::log::warn!("Could not save this machine's match history: {why}");
        }
    }
}

/// Match ids belong to a service, so two services' match #1 cannot overwrite each other.
pub(super) fn service_key(service: &str, match_id: u64) -> String {
    format!("service:{}:{service}:{match_id}", service.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait_loaded(archive: &mut Archive) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while archive.loading.is_some() {
            archive.poll();
            assert!(
                std::time::Instant::now() < deadline,
                "archive reader finished"
            );
            std::thread::yield_now();
        }
    }
    fn recorded(map: u16, owner: Option<SlotId>) -> Saved {
        let (_, file) = crate::postgame::recorded_match_for_menu();
        let mut meta = bota_replay::meta::decode(&std::fs::read(file).unwrap()).unwrap();
        meta.header.map = map;
        let mut writer = bota_replay::meta::Writer::default();
        assert!(writer.push(&bota_replay::meta::MetaRecord::Header(meta.header), false));
        for record in meta.records {
            assert!(writer.push(&record, true));
        }
        Saved {
            key: "recorded".to_owned(),
            file: None,
            when: 100,
            mode: "file".to_owned(),
            owner,
            metadata: writer.into_bytes(),
            metadata_key: None,
            cached: None,
            result: None,
            summary: None,
            temporary: None,
        }
    }
    fn delivered(service: &str) -> Saved {
        use bota_radovid::proto::{EndReason, MatchId, MatchResult, Outcome, RulesetId, UnixMs};
        let mut entry = recorded(2, Some(SlotId(0)));
        let id = bota_replay::meta::decode(&entry.metadata)
            .unwrap()
            .header
            .match_id;
        entry.key = service_key(service, id);
        entry.result = Some(MatchResult {
            match_id: MatchId(id),
            ruleset: RulesetId("solo_mid".to_owned()),
            rated: false,
            outcome: Outcome::Winner { team: 0 },
            reason: EndReason::Decided,
            duration_ms: 60000,
            game_report: Vec::new(),
            seats: Vec::new(),
            finished_at: UnixMs(321000),
        });
        entry
    }
    #[test]
    fn independent_services_never_merge_matches_just_because_the_ids_and_stats_agree() {
        let mut archive = Archive::default();
        archive.insert(delivered("first"), 100);
        archive.insert(delivered("second"), 100);
        assert_eq!(archive.entries.len(), 2);
        assert_ne!(archive.entries[0].key, archive.entries[1].key);
    }
    #[test]
    fn attaching_a_replay_preserves_its_delivered_outcome_without_duplicating_the_match() {
        let mut archive = Archive::default();
        archive.insert(delivered("first"), 100);
        let mut attached = delivered("first");
        attached.file = Some("kept.brp".to_owned());
        archive.insert(attached.clone(), 100);
        let key = attached.key.clone();
        attached.key = "kept.brp".to_owned();
        attached.result = None; // A native library scan knows the file, not the service outcome.
        archive.insert(attached, 100);
        assert_eq!(archive.entries.len(), 1);
        assert_eq!(archive.entries[0].key, key);
        assert!(archive.entries[0].result.is_some());
        assert_eq!(archive.entries[0].when, 321);
    }
    #[test]
    fn card_projection_is_shared_until_history_or_replay_availability_changes() {
        let mut archive = Archive::default();
        let mut entry = recorded(2, Some(SlotId(0)));
        entry.file = Some("kept.brp".to_owned());
        archive.insert(entry, 50);
        let first = archive.cards(&[]);
        assert!(Arc::ptr_eq(&first, &archive.cards(&[])));
        let record = crate::replays::Record {
            name: "kept.brp".to_owned(),
            when: 100,
            bytes: crate::replays::MIN_BYTES,
            mode: "local".to_owned(),
            match_id: None,
            players: String::new(),
            outcome: String::new(),
            owner: Some(SlotId(0)),
        };
        let with_replay = archive.cards(std::slice::from_ref(&record));
        assert!(!Arc::ptr_eq(&first, &with_replay));
        assert!(!first[0].watchable);
        assert!(with_replay[0].watchable);
        assert!(Arc::ptr_eq(&with_replay, &archive.cards(&[record])));
        let mut later = recorded(2, Some(SlotId(0)));
        later.key = "later".to_owned();
        archive.insert(later, 50);
        assert_eq!(archive.cards(&[]).len(), 2);
    }
    #[test]
    fn an_archive_arriving_after_an_identical_live_record_invalidates_the_card_cache() {
        let recent = recorded(2, Some(SlotId(0)));
        let mut older = recent.clone();
        older.key = "older-record".to_owned();
        older.when = 1;
        assert!(older.prepare());
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        let mut archive = Archive {
            loading: Some(receive),
            ..Default::default()
        };
        archive.insert(recent.clone(), 50);
        let first = archive.cards(&[]);
        assert_eq!(first.len(), 1);
        send.send(vec![recent, older]).unwrap();
        archive.poll();
        assert_eq!(archive.entries.len(), 2, "the retained rows arrived");
        let loaded = archive.cards(&[]);
        assert_eq!(
            loaded.len(),
            2,
            "arrival of older history invalidates the cached projection"
        );
        assert!(!Arc::ptr_eq(&first, &loaded));
    }
    #[test]
    fn a_policy_sized_archive_never_reloads_more_than_the_library_keeps() {
        let directory = crate::test_support::scratch_path("archive_policy_limit");
        if directory.exists() {
            std::fs::remove_dir_all(&directory).unwrap();
        }
        let store = SettingsStore::new(Some(directory.join("settings.conf")));
        let mut archive = Archive::default();
        for number in 0..50 {
            let mut entry = recorded(2, Some(SlotId(0)));
            entry.key = format!("policy-{number}");
            archive.insert(entry, 50);
        }
        archive.save(&store).unwrap();
        let mut library = crate::replays::Library::none();
        library.set_policy(crate::replays::Policy {
            max_count: 25,
            ..Default::default()
        });
        let mut core =
            super::super::core::MenuCore::new(store, bota_radovid::Tls::webpki(), None, library);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while core.local_history.loading.is_some() {
            core.update();
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(
            core.local_history.entries.len(),
            25,
            "loading respects the library policy"
        );
    }

    #[test]
    fn a_browser_blob_quota_failure_still_commits_the_history_index() {
        let directory = crate::test_support::scratch_path("archive_browser_quota");
        if directory.exists() {
            std::fs::remove_dir_all(&directory).unwrap();
        }
        let store = SettingsStore::new(Some(directory.join("settings.conf")));
        let mut entry = recorded(2, Some(SlotId(0)));
        assert!(entry.prepare());
        let saved = super::super::archive_storage::write_snapshot(
            &[entry],
            true,
            |_, _| Err(std::io::Error::other("browser metadata quota exceeded")),
            |text| store.write(super::super::store::Entry::MatchHistory, text),
        );
        assert!(
            saved.is_ok(),
            "a failed blob must not prevent the small index save: {saved:?}"
        );
        let mut restored = Archive::load(&store, 50);
        wait_loaded(&mut restored);
        let cards = restored.cards(&[]);
        assert_eq!(cards.len(), 1, "the history row survives the missing blob");
        assert_eq!(cards[0].hero, Some(bota_proto::HeroId::SHADOW_FIEND));
        assert!(restored.entries[0].details().is_none());
    }
    #[test]
    fn a_native_blob_failure_does_not_replace_the_previous_index() {
        let mut entry = recorded(2, Some(SlotId(0)));
        assert!(entry.prepare());
        let mut wrote_index = false;
        let saved = super::super::archive_storage::write_snapshot(
            &[entry],
            false,
            |_, _| Err(std::io::Error::other("native write failed")),
            |_| {
                wrote_index = true;
                Ok(())
            },
        );
        assert!(saved.is_err());
        assert!(
            !wrote_index,
            "native storage retains its previous complete snapshot"
        );
    }
    #[test]
    fn fifty_full_length_recordings_survive_without_replay_files() {
        let seed = recorded(2, Some(SlotId(0)));
        let bytes = full_length_metadata();
        let directory = crate::test_support::scratch_path("full_history");
        if directory.exists() {
            std::fs::remove_dir_all(&directory).unwrap();
        }
        let store = SettingsStore::new(Some(directory.join("settings.conf")));
        let mut archive = Archive::default();
        for index in 0..50 {
            let mut entry = seed.clone();
            entry.key = format!("full-{index}");
            entry.when = index;
            entry.metadata = with_match_id(&bytes, index + 1);
            archive.insert(entry, 50);
        }
        archive.save(&store).unwrap();
        assert_eq!(
            archive.entries.len(),
            50,
            "the byte cap never silently drops matches"
        );
        let mut restored = Archive::load(&store, 50);
        wait_loaded(&mut restored);
        assert_eq!(restored.entries.len(), 50);
        let ids: std::collections::HashSet<_> = restored
            .entries
            .iter()
            .map(|entry| {
                bota_replay::meta::decode(&entry.metadata)
                    .unwrap()
                    .header
                    .match_id
            })
            .collect();
        assert_eq!(ids.len(), 50);
        assert_eq!(
            std::fs::read_dir(directory.join("match_metadata"))
                .unwrap()
                .count(),
            50
        );
        assert!(
            std::fs::metadata(directory.join("matches.json"))
                .unwrap()
                .len()
                < 4 << 20
        );
    }
    fn full_length_metadata() -> Vec<u8> {
        use bota_replay::meta::MetaRecord;
        let seed = recorded(2, Some(SlotId(0)));
        let meta = bota_replay::meta::decode(&seed.metadata).unwrap();
        let stats = meta
            .records
            .iter()
            .find_map(|record| match record {
                MetaRecord::Sample { stats, .. } => Some(stats.clone()),
                _ => None,
            })
            .unwrap();
        let mut writer = bota_replay::meta::Writer::default();
        let mut header = meta.header;
        header.pregame_ticks = 0;
        header.map = 0;
        assert!(writer.push(&MetaRecord::Header(header), false));
        for slot in 0..10 {
            assert!(writer.push(
                &MetaRecord::Seat {
                    slot,
                    team: slot % 2,
                    hero: 1
                },
                false
            ));
        }
        for minute in 0..=bota_replay::limits::MAX_MINUTES {
            for slot in 0..10 {
                if !writer.push(
                    &MetaRecord::Sample {
                        tick: minute * 1800,
                        slot,
                        stats: stats.clone(),
                    },
                    false,
                ) {
                    break;
                }
            }
        }
        assert!(writer.push(
            &MetaRecord::End {
                tick: 360 * 1800,
                winner: Some(0)
            },
            true
        ));
        let bytes = writer.into_bytes();
        assert!(
            bytes.len() > 100_000,
            "a full recording exercises the storage budget"
        );
        bytes
    }

    fn with_match_id(bytes: &[u8], match_id: u64) -> Vec<u8> {
        use bota_replay::meta::MetaRecord;
        let mut meta = bota_replay::meta::decode(bytes).unwrap();
        meta.header.match_id = match_id;
        let mut writer = bota_replay::meta::Writer::default();
        assert!(writer.push(&MetaRecord::Header(meta.header), false));
        for record in meta.records {
            assert!(writer.push(&record, true));
        }
        writer.into_bytes()
    }
    #[test]
    fn opaque_game_reports_are_not_duplicated_in_the_small_index() {
        let mut archive = Archive::default();
        let old = delivered("old");
        let old_key = old.key.clone();
        archive.insert(old, 100);
        let mut large = delivered("large");
        large.result.as_mut().unwrap().game_report = vec![255; 1 << 20];
        large.result.as_mut().unwrap().finished_at = bota_radovid::proto::UnixMs(999000);
        archive.insert(large, 100);
        archive.save(&SettingsStore::default()).unwrap();
        assert_eq!(archive.entries.len(), 2);
        assert!(archive.entries.iter().any(|entry| entry.key == old_key));
        assert!(
            archive.entries.iter().all(|entry| entry
                .result
                .as_ref()
                .unwrap()
                .game_report
                .is_empty())
        );
    }
    #[test]
    fn invalid_optional_ratings_cannot_make_the_whole_saved_history_unreadable() {
        use bota_radovid::proto::{AccountId, AccountKind, Rating, SeatResult};
        let (_, meta) = crate::postgame::recorded_match_for_menu();
        let store = SettingsStore::new(Some(
            meta.parent()
                .unwrap()
                .join("archive_invalid_rating/settings.conf"),
        ));
        let mut archive = Archive::default();
        let old = delivered("valid");
        let key = old.key.clone();
        archive.insert(old, 100);
        let mut bad = delivered("invalid");
        bad.result.as_mut().unwrap().seats.push(SeatResult {
            slot: 0,
            team: 0,
            account: AccountId(1),
            nickname: "player".to_owned(),
            kind: AccountKind::Human,
            before: Some(Rating {
                rating: f64::NAN,
                deviation: 100.0,
            }),
            after: None,
        });
        archive.insert(bad, 100);
        archive.save(&store).unwrap();
        let mut restored = Archive::load(&store, 50);
        wait_loaded(&mut restored);
        assert!(
            restored.entries.iter().any(|entry| entry.key == key),
            "a damaged optional rating must not erase a valid match on restart"
        );
    }
    #[test]
    fn an_imported_demo_never_counts_as_a_personal_match() {
        let mut archive = Archive::default();
        archive.insert(recorded(1, Some(SlotId(0))), 100);
        assert!(
            archive.entries.is_empty(),
            "the metadata map, not the filename or user label, identifies Demo"
        );
    }
    #[test]
    fn an_unknown_owner_is_not_inferred_from_a_hero_or_first_slot() {
        let mut archive = Archive::default();
        archive.insert(recorded(2, None), 100);
        let cards = archive.cards(&[]);
        assert_eq!(cards.len(), 1);
        assert!(cards[0].hero.is_none());
        assert!(super::super::history::heroes(&cards).is_empty());
    }
}

#[cfg(test)]
mod details_privacy_tests {
    use super::*;
    use bota_replay::meta::{MetaRecord, Writer};
    #[test]
    fn every_details_consumer_gets_only_the_unfinished_owners_timed_facts() {
        let meta = bota_replay::meta::decode(&crate::postgame::preview::metadata("match_details"))
            .expect("fixture");
        let mut writer = Writer::default();
        assert!(writer.push(&MetaRecord::Header(meta.header), false));
        for record in meta.records {
            if !matches!(record, MetaRecord::End { .. }) {
                assert!(writer.push(&record, false));
            }
        }
        let metadata = writer.into_bytes();
        let summary = Arc::new(
            MatchSummary::from_meta(&bota_replay::meta::decode(&metadata).expect("metadata"))
                .expect("summary"),
        );
        for owner in [Some(SlotId(0)), None] {
            let saved = Saved {
                key: "unfinished".into(),
                file: None,
                when: 0,
                mode: "all_pick".into(),
                owner,
                metadata: metadata.clone(),
                metadata_key: None,
                cached: None,
                result: None,
                summary: Some(summary.clone()),
                temporary: None,
            };
            let details = saved.details().expect("details");
            let recording = details.recording.expect("timed facts");
            assert_eq!(
                recording.samples.len(),
                usize::from(owner.is_some()),
                "the privacy boundary includes timed samples"
            );
            assert!(
                recording
                    .purchases
                    .iter()
                    .all(|event| Some(event.slot) == owner)
            );
            assert!(
                recording
                    .levels
                    .iter()
                    .all(|event| Some(event.slot) == owner)
            );
            assert!(
                recording
                    .kills
                    .iter()
                    .all(|event| event.killer == owner || event.victim == owner)
            );
            assert!(recording.restricted);
        }
        assert_eq!(
            summary.recording.as_ref().expect("source").samples.len(),
            10,
            "stored metadata stays intact"
        );
    }
}
