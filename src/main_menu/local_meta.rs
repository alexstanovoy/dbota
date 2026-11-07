//! A bounded local-library metadata reader. File work and decoding happen off the frame;
//! unfinished recorder trailers get a short retry window, without an unbounded request queue.
use super::local_matches::Saved;
#[cfg(not(target_arch = "wasm32"))]
use std::{
    collections::HashMap,
    sync::mpsc,
    time::{Duration, Instant},
};

#[cfg(not(target_arch = "wasm32"))]
struct Seen {
    bytes: u64,
    attempts: u8,
    retry: Instant,
    ready: bool,
    #[cfg(test)]
    answered: bool,
}

#[cfg(not(target_arch = "wasm32"))]
struct Job {
    path: std::path::PathBuf,
    record: crate::replays::Record,
    temporary: bool,
}

#[derive(Default)]
pub struct Reader {
    #[cfg(not(target_arch = "wasm32"))]
    seen: HashMap<String, Seen>,
    #[cfg(not(target_arch = "wasm32"))]
    ask: Option<mpsc::SyncSender<Job>>,
    #[cfg(not(target_arch = "wasm32"))]
    failed_until: Option<Instant>,
    #[cfg(not(target_arch = "wasm32"))]
    answers: Option<mpsc::Receiver<(crate::replays::Record, bool, Option<Saved>)>>,
    #[cfg(not(target_arch = "wasm32"))]
    pending: std::collections::VecDeque<Job>,
}

impl Reader {
    #[cfg(all(test, not(target_arch = "wasm32")))]
    pub fn initial_scan_complete(&self, records: &[crate::replays::Record]) -> bool {
        records.iter().all(|record| {
            self.seen
                .get(&record.name)
                .is_some_and(|seen| seen.answered)
        })
    }

    pub fn retain(&mut self, records: &[crate::replays::Record]) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let names: std::collections::HashSet<_> =
                records.iter().map(|record| record.name.as_str()).collect();
            self.seen.retain(|name, _| names.contains(name.as_str()));
        }
        #[cfg(target_arch = "wasm32")]
        let _ = records;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn request(
        &mut self,
        record: &crate::replays::Record,
        path: impl FnOnce() -> Option<std::path::PathBuf>,
    ) {
        if record.mode == "demo" {
            return;
        }
        let now = Instant::now();
        if self.failed_until.is_some_and(|retry| now < retry) {
            return;
        }
        if self.seen.get(&record.name).is_some_and(|seen| {
            seen.bytes == record.bytes && (seen.ready || seen.attempts >= 20 || now < seen.retry)
        }) {
            return;
        }
        let Some(path) = path() else {
            return;
        };
        if !self.worker(now) {
            return;
        }
        if self.ask.as_ref().is_some_and(|ask| {
            ask.try_send(Job {
                path,
                record: record.clone(),
                temporary: false,
            })
            .is_ok()
        }) {
            let seen = self.seen.entry(record.name.clone()).or_insert(Seen {
                bytes: record.bytes,
                attempts: 0,
                retry: now,
                ready: false,
                #[cfg(test)]
                answered: false,
            });
            if seen.bytes != record.bytes {
                seen.bytes = record.bytes;
                seen.attempts = 0;
                seen.ready = false;
                #[cfg(test)]
                {
                    seen.answered = false;
                }
            }
            seen.attempts += 1;
            seen.retry = now + Duration::from_millis(500);
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn worker(&mut self, now: Instant) -> bool {
        if self.failed_until.is_some_and(|retry| now < retry) {
            return false;
        }
        if self.ask.is_none() {
            let (ask, jobs) = mpsc::sync_channel::<Job>(16);
            let (tell, answers) = mpsc::sync_channel(16);
            let spawned = std::thread::Builder::new()
                .name("local-match-metadata".to_owned())
                .spawn(move || {
                    for job in jobs {
                        let entry = read_job(&job);
                        if tell.send((job.record, job.temporary, entry)).is_err() {
                            break;
                        }
                    }
                });
            if let Err(why) = spawned {
                self.failed_until = Some(now + Duration::from_secs(30));
                bevy::log::warn!("Cannot read local match history: {why}");
                return false;
            }
            self.ask = Some(ask);
            self.answers = Some(answers);
        }
        true
    }

    /// A metadata-only local match uses the same bounded worker, without a replay-library row.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request_temporary(
        &mut self,
        path: std::path::PathBuf,
        mode: &str,
        owner: Option<bota_proto::SlotId>,
    ) {
        if self.pending.len() >= 16 {
            bevy::log::warn!("Local metadata queue is full; its temporary file is retained");
            return;
        }
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.pending.push_back(Job {
            path,
            temporary: true,
            record: crate::replays::Record {
                name,
                owner,
                when: crate::replays::now(),
                bytes: 0,
                mode: mode.to_owned(),
                match_id: None,
                players: String::new(),
                outcome: String::new(),
            },
        });
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn send_temporary(&mut self) {
        if self.pending.is_empty() || !self.worker(Instant::now()) {
            return;
        }
        if let Some(job) = self.pending.pop_front()
            && let Some(ask) = &self.ask
            && let Err(why) = ask.try_send(job)
        {
            match why {
                mpsc::TrySendError::Full(job) => self.pending.push_front(job),
                mpsc::TrySendError::Disconnected(job) => {
                    self.pending.push_front(job);
                    self.ask = None;
                }
            }
        }
    }

    pub fn poll(&mut self) -> Vec<Saved> {
        #[cfg_attr(
            target_arch = "wasm32",
            expect(unused_mut, reason = "Only native libraries read files on a worker")
        )]
        let mut loaded = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        self.send_temporary();
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(answers) = &self.answers {
            while let Ok((record, temporary, answer)) = answers.try_recv() {
                if temporary {
                    if let Some(answer) = answer {
                        loaded.push(answer);
                    }
                    continue;
                }
                let Some(seen) = self
                    .seen
                    .get_mut(&record.name)
                    .filter(|seen| seen.bytes == record.bytes)
                else {
                    continue;
                };
                #[cfg(test)]
                {
                    seen.answered = true;
                }
                if let Some(answer) = answer {
                    seen.ready = answer
                        .summary
                        .as_ref()
                        .is_some_and(|summary| summary.winner.is_some());
                    loaded.push(answer);
                }
            }
        }
        loaded
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read(path: &std::path::Path, record: &crate::replays::Record) -> Option<Saved> {
    use std::{io::Read, sync::Arc};
    let limit =
        (bota_replay::limits::MAX_META_BYTES + bota_replay::limits::END_RESERVE_BYTES) as u64;
    let sidecar = std::fs::File::open(path.with_extension("meta"))
        .ok()
        .and_then(|file| {
            let mut bytes = Vec::new();
            file.take(limit + 1).read_to_end(&mut bytes).ok()?;
            (bytes.len() as u64 <= limit && bota_replay::meta::decode(&bytes).is_ok())
                .then_some(bytes)
        });
    let metadata = sidecar.or_else(|| {
        let mut file = std::fs::File::open(path).ok()?;
        bota_client_core::meta_in_replay(&mut file).ok().flatten()
    })?;
    let summary = crate::postgame::summary::MatchSummary::from_meta(
        &bota_replay::meta::decode(&metadata).ok()?,
    )?;
    Some(Saved {
        key: record.name.clone(),
        file: Some(record.name.clone()),
        when: record.when,
        mode: record.mode.clone(),
        owner: record.owner,
        metadata,
        metadata_key: None,
        cached: None,
        result: None,
        summary: Some(Arc::new(summary)),
        temporary: None,
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn read_job(job: &Job) -> Option<Saved> {
    let mut entry = read(&job.path, &job.record);
    if job.temporary {
        for _ in 0..20 {
            if entry.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(500));
            entry = read(&job.path, &job.record);
        }
        if let Some(entry) = &mut entry {
            entry.file = None;
            entry.temporary = Some(job.path.clone());
        }
    }
    entry
}
