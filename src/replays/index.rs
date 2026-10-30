//! What the library knows of the replays it keeps, as the small text file that sits beside
//! them (`index.txt`): one row per replay, and the matches whose replay is still to be
//! fetched. Pure data and rules, no I/O: both stores (a folder, the browser's private
//! storage) read and write it the same way.

use std::collections::HashSet;

/// The first line of an index; a file that starts otherwise is not read.
const MAGIC: &str = "dbota-replays 1";
/// Rows an index holds at most, wanted ones included: a bound on a damaged file's work.
const MAX_ROWS: usize = 10_000;
/// Longest row read, in bytes.
const MAX_ROW_LEN: usize = 2048;
/// Longest text a row keeps of a replay's players or outcome, in characters.
pub const MAX_TEXT: usize = 120;
/// Longest a replay's mode is, in characters.
const MAX_MODE: usize = 24;
/// Days a replay still to be fetched is waited for: as long as the service keeps it.
pub const WANT_DAYS: u64 = 14;

/// The replays above this size are never kept: the longest a service serves.
pub const MAX_BYTES: u64 = 64 << 20;
/// A replay below this size holds no match: the file a game server writes before its match
/// starts is a header and nothing else.
pub const MIN_BYTES: u64 = 1024;

/// One kept replay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The file's name: `20261030-153045-solo_mid-4711.brp`.
    pub name: String,
    /// When the match ended, in seconds since 1970 (UTC).
    pub when: u64,
    pub bytes: u64,
    /// The ruleset of an online match, `local` for a lobby on this machine, `demo` for Demo
    /// Hero, `file` for a replay that came from elsewhere.
    pub mode: String,
    /// The service's id of the match, for online ones.
    pub match_id: Option<u64>,
    /// Who played, as the match's result told it; empty when it did not.
    pub players: String,
    /// How it ended, as the result told it.
    pub outcome: String,
}

/// A match played whose replay is to be fetched from the service: kept in the index so a
/// client closed before the fetch (or before the match ended) still gets it next time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub match_id: u64,
    /// When the match was entered, in seconds since 1970.
    pub since: u64,
    pub mode: String,
}

/// What the player lets the library keep.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    /// Whether replays are saved at all.
    pub keep: bool,
    /// The most replays kept; the oldest go first.
    pub max_count: usize,
    /// The most bytes all of them take; the oldest go first.
    pub max_bytes: u64,
}

impl Default for Policy {
    /// Saving on. A desktop keeps fifty replays in a gigabyte; a browser, which shares its
    /// storage with the copy of the assets, twenty-five in a quarter of it.
    fn default() -> Policy {
        if cfg!(target_arch = "wasm32") {
            Policy {
                keep: true,
                max_count: 25,
                max_bytes: 256 << 20,
            }
        } else {
            Policy {
                keep: true,
                max_count: 50,
                max_bytes: 1 << 30,
            }
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Index {
    /// Newest first.
    pub records: Vec<Record>,
    pub wanted: Vec<Wanted>,
}

/// Text that is safe in a row: one line, no tabs, bounded.
pub fn clean(text: &str, max: usize) -> String {
    crate::main_menu::line(text, max).replace('\t', " ")
}

/// A mode as it goes into a file name: `[a-z0-9_]`, bounded.
pub fn mode_name(mode: &str) -> String {
    let name: String = mode
        .chars()
        .map(|c| c.to_ascii_lowercase())
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .take(MAX_MODE)
        .collect();
    if name.is_empty() { "game".into() } else { name }
}

impl Index {
    pub fn parse(text: &str) -> Index {
        let mut index = Index::default();
        let mut lines = text.lines();
        if lines.next() != Some(MAGIC) {
            return index;
        }
        for line in lines.take(MAX_ROWS) {
            if line.len() > MAX_ROW_LEN {
                continue;
            }
            let cols: Vec<&str> = line.split('\t').collect();
            match cols.as_slice() {
                ["r", when, bytes, match_id, mode, name, players, outcome] => {
                    let (Ok(when), Ok(bytes)) = (when.parse(), bytes.parse()) else {
                        continue;
                    };
                    if !valid_name(name) {
                        continue;
                    }
                    index.records.push(Record {
                        name: (*name).to_owned(),
                        when,
                        bytes,
                        mode: mode_name(mode),
                        match_id: match_id.parse().ok(),
                        players: clean(players, MAX_TEXT),
                        outcome: clean(outcome, MAX_TEXT),
                    });
                }
                ["w", match_id, since, mode] => {
                    if let (Ok(match_id), Ok(since)) = (match_id.parse(), since.parse()) {
                        index.wanted.push(Wanted {
                            match_id,
                            since,
                            mode: mode_name(mode),
                        });
                    }
                }
                _ => {}
            }
        }
        index.sort();
        index
    }

    pub fn render(&self) -> String {
        let mut out = format!("{MAGIC}\n");
        for r in &self.records {
            let id = r
                .match_id
                .map_or_else(|| "-".to_owned(), |id| id.to_string());
            out.push_str(&format!(
                "r\t{}\t{}\t{id}\t{}\t{}\t{}\t{}\n",
                r.when,
                r.bytes,
                r.mode,
                r.name,
                clean(&r.players, MAX_TEXT),
                clean(&r.outcome, MAX_TEXT)
            ));
        }
        for w in &self.wanted {
            out.push_str(&format!("w\t{}\t{}\t{}\n", w.match_id, w.since, w.mode));
        }
        out
    }

    fn sort(&mut self) {
        self.records
            .sort_by(|a, b| b.when.cmp(&a.when).then_with(|| b.name.cmp(&a.name)));
    }

    pub fn find(&self, name: &str) -> Option<&Record> {
        self.records.iter().find(|r| r.name == name)
    }

    /// The kept replay of an online match.
    pub fn of_match(&self, match_id: u64) -> Option<&Record> {
        self.records.iter().find(|r| r.match_id == Some(match_id))
    }

    /// Takes in a replay (one of the same name is replaced) and drops the oldest ones past the
    /// policy's caps; the names of the replays dropped, whose files the caller removes.
    pub fn insert(&mut self, record: Record, policy: &Policy) -> Vec<String> {
        self.records.retain(|r| r.name != record.name);
        if let Some(id) = record.match_id {
            self.wanted.retain(|w| w.match_id != id);
        }
        self.records.push(record);
        self.sort();
        self.prune(policy)
    }

    /// Drops the oldest replays until the count and the bytes fit the policy.
    pub fn prune(&mut self, policy: &Policy) -> Vec<String> {
        let mut kept = 0usize;
        let mut bytes = 0u64;
        let mut dropped = Vec::new();
        // Newest first: once a replay does not fit, no older one is kept either.
        let mut full = false;
        self.records.retain(|r| {
            full = full
                || kept >= policy.max_count
                || bytes.saturating_add(r.bytes) > policy.max_bytes;
            if full {
                dropped.push(r.name.clone());
            } else {
                kept += 1;
                bytes += r.bytes;
            }
            !full
        });
        dropped
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let before = self.records.len();
        self.records.retain(|r| r.name != name);
        self.records.len() != before
    }

    /// Notes a match whose replay is to be fetched; one noted already stays as it was.
    pub fn want(&mut self, wanted: Wanted) {
        if self.wanted.iter().any(|w| w.match_id == wanted.match_id)
            || self
                .records
                .iter()
                .any(|r| r.match_id == Some(wanted.match_id))
        {
            return;
        }
        if self.wanted.len() >= MAX_WANTED {
            self.wanted.remove(0);
        }
        self.wanted.push(wanted);
    }

    pub fn unwant(&mut self, match_id: u64) {
        self.wanted.retain(|w| w.match_id != match_id);
    }

    /// Drops the matches waited for longer than the service keeps their replay.
    pub fn expire(&mut self, now: u64) {
        self.wanted
            .retain(|w| now.saturating_sub(w.since) <= WANT_DAYS * 86_400);
    }

    /// Makes the index agree with the files that are there, `(name, bytes, modified)` each: a
    /// row without a file goes, a file without a row is taken in under its own time, and a
    /// row's size is the file's. Whether anything changed.
    pub fn reconcile(&mut self, files: &[(String, u64, u64)]) -> bool {
        let before = self.clone();
        let there: HashSet<&str> = files.iter().map(|(name, ..)| name.as_str()).collect();
        self.records.retain(|r| there.contains(r.name.as_str()));
        for (name, bytes, modified) in files {
            match self.records.iter_mut().find(|r| &r.name == name) {
                Some(row) => row.bytes = *bytes,
                None => self.records.push(Record {
                    name: name.clone(),
                    when: *modified,
                    bytes: *bytes,
                    mode: "file".to_owned(),
                    match_id: None,
                    players: String::new(),
                    outcome: String::new(),
                }),
            }
        }
        self.sort();
        *self != before
    }
}

/// Wanted matches held at most.
const MAX_WANTED: usize = 16;

/// Whether `name` is a replay's file name as this library makes them: no path in it, `.brp` at
/// its end, and nothing a file system or a page's download would take for more.
pub fn valid_name(name: &str) -> bool {
    name.len() <= 96
        && name.ends_with(".brp")
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.contains("..")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(name: &str, when: u64, bytes: u64) -> Record {
        Record {
            name: name.to_owned(),
            when,
            bytes,
            mode: "solo_mid".to_owned(),
            match_id: Some(when),
            players: "a vs b".to_owned(),
            outcome: "a won, 1:00".to_owned(),
        }
    }

    /// The caps drop the oldest replays, by count and by bytes, and the newest always stays.
    #[test]
    fn the_caps_drop_the_oldest_first() {
        let mut index = Index::default();
        let by_count = Policy {
            max_count: 3,
            ..Policy::default()
        };
        for at in 1..=5 {
            let dropped = index.insert(record(&format!("{at}.brp"), at, 10), &by_count);
            assert_eq!(dropped.len(), usize::from(at > 3), "at {at}");
        }
        let names: Vec<&str> = index.records.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["5.brp", "4.brp", "3.brp"]);
        let by_bytes = Policy {
            max_bytes: 25,
            ..Policy::default()
        };
        assert_eq!(index.prune(&by_bytes), ["3.brp"]);
        assert_eq!(index.records.len(), 2);
    }

    #[test]
    fn the_index_reads_back_and_a_damaged_one_reads_as_far_as_it_is_sound() {
        let mut index = Index::default();
        index.insert(record("a.brp", 7, 5), &Policy::default());
        index.want(Wanted {
            match_id: 9,
            since: 3,
            mode: "solo_mid".to_owned(),
        });
        assert_eq!(Index::parse(&index.render()), index);
        let damaged = format!(
            "{}r\tnot a number\t1\t-\tx\ty.brp\t\t\nr\t1\t1\t-\tx\t../evil.brp\t\t\nw\t1\n",
            index.render()
        );
        assert_eq!(Index::parse(&damaged), index);
        assert_eq!(
            Index::parse("something else\nr\t1\t1\t-\tx\ty.brp\t\t\n"),
            Index::default()
        );
    }

    #[test]
    fn a_replay_in_the_index_drops_its_wait_and_files_that_come_and_go_are_followed() {
        let mut index = Index::default();
        index.want(Wanted {
            match_id: 4,
            since: 1,
            mode: "m".to_owned(),
        });
        index.insert(record("x.brp", 4, 5), &Policy::default());
        assert!(index.wanted.is_empty());
        assert!(index.reconcile(&[("y.brp".to_owned(), 9, 100)]));
        assert_eq!(index.records.len(), 1);
        assert_eq!(
            (index.records[0].name.as_str(), index.records[0].when),
            ("y.brp", 100)
        );
        assert!(!index.reconcile(&[("y.brp".to_owned(), 9, 100)]));
    }
}
