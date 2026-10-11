//! Frecency: how often and how recently an item was chosen for a query
//! starting a given way (DESIGN.md, "Ranking").
//!
//! Keyed by the item's id and the first two characters of the query, so
//! choosing Firefox after typing `fi` makes it rank first for `fi` and
//! `fir`, without making it rank first for `ma`. Each pick adds 1, and
//! values halve every 7 days, so a habit fades within weeks once it stops.
//! At most [`CAP`] entries are kept, the least recently used dropped
//! first, which bounds the state file.
//!
//! Pure: time is passed in, in seconds since the Unix epoch, and storing
//! the entries is the caller's (the daemon writes them to
//! `state/frecency.json`).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// A pick's weight halves over this many seconds: 7 days.
pub const HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 3600.0;
/// Most entries kept.
pub const CAP: usize = 5000;

/// One `(item, query start)` pair, as stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// The item's id.
    pub id: String,
    /// The query's first two characters, lowercased; empty for a pick
    /// made with an empty query.
    pub key: String,
    /// The value at `at`.
    pub score: f64,
    /// When it was last picked, seconds since the Unix epoch.
    pub at: u64,
}

/// The query start an entry is keyed by.
pub fn key(query: &str) -> String {
    query
        .trim_start()
        .chars()
        .take(2)
        .flat_map(char::to_lowercase)
        .collect()
}

fn decayed(score: f64, at: u64, now: u64) -> f64 {
    let age = now.saturating_sub(at) as f64;
    score * 0.5f64.powf(age / HALF_LIFE_SECS)
}

/// The frecency table.
#[derive(Debug, Clone, Default)]
pub struct Frecency {
    /// key → id → (score, at).
    by_key: HashMap<String, HashMap<String, (f64, u64)>>,
    len: usize,
}

impl Frecency {
    /// A table from stored entries. Later duplicates win; a bad score
    /// (negative, not finite) is dropped.
    pub fn from_entries(entries: Vec<Entry>) -> Self {
        let mut f = Frecency::default();
        for e in entries {
            if e.score.is_finite() && e.score >= 0.0 {
                f.insert(e.key, e.id, e.score, e.at);
            }
        }
        f.prune();
        f
    }

    /// The entries, for storing.
    pub fn entries(&self) -> Vec<Entry> {
        let mut out: Vec<Entry> = self
            .by_key
            .iter()
            .flat_map(|(key, ids)| {
                ids.iter().map(|(id, &(score, at))| Entry {
                    id: id.clone(),
                    key: key.clone(),
                    score,
                    at,
                })
            })
            .collect();
        out.sort_by(|a, b| (&a.key, &a.id).cmp(&(&b.key, &b.id)));
        out
    }

    /// How many entries there are.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn insert(&mut self, key: String, id: String, score: f64, at: u64) {
        if self
            .by_key
            .entry(key)
            .or_default()
            .insert(id, (score, at))
            .is_none()
        {
            self.len += 1;
        }
    }

    /// Records that `id` was chosen for `query` at `now`.
    pub fn record(&mut self, id: &str, query: &str, now: u64) {
        let key = key(query);
        let current = self.value_for_key(id, &key, now);
        self.insert(key, id.to_owned(), current + 1.0, now);
        self.prune();
    }

    /// The value of `id` for queries starting like `query`.
    pub fn value(&self, id: &str, query: &str, now: u64) -> f64 {
        self.value_for_key(id, &key(query), now)
    }

    /// The value of `id` under an already-computed key; the search loop
    /// computes the key once.
    pub fn value_for_key(&self, id: &str, key: &str, now: u64) -> f64 {
        self.by_key
            .get(key)
            .and_then(|ids| ids.get(id))
            .map_or(0.0, |&(score, at)| decayed(score, at, now))
    }

    /// Every id with a value under `key`, and that value at `now`: what a
    /// search needs, without a lookup per candidate.
    pub fn for_key<'a>(&'a self, key: &str, now: u64) -> impl Iterator<Item = (&'a str, f64)> + 'a {
        self.by_key.get(key).into_iter().flat_map(move |ids| {
            ids.iter()
                .map(move |(id, &(score, at))| (id.as_str(), decayed(score, at, now)))
        })
    }

    /// Every id's value summed over all keys: how an empty query ranks.
    pub fn totals(&self, now: u64) -> HashMap<&str, f64> {
        let mut totals: HashMap<&str, f64> = HashMap::new();
        for ids in self.by_key.values() {
            for (id, &(score, at)) in ids {
                *totals.entry(id.as_str()).or_default() += decayed(score, at, now);
            }
        }
        totals
    }

    /// Drops the least recently picked entries beyond [`CAP`].
    fn prune(&mut self) {
        if self.len <= CAP {
            return;
        }
        let mut all: Vec<(u64, String, String)> = self
            .by_key
            .iter()
            .flat_map(|(key, ids)| {
                ids.iter()
                    .map(|(id, &(_, at))| (at, key.clone(), id.clone()))
            })
            .collect();
        all.sort();
        for (_, key, id) in all.into_iter().take(self.len - CAP) {
            if let Some(ids) = self.by_key.get_mut(&key) {
                ids.remove(&id);
                if ids.is_empty() {
                    self.by_key.remove(&key);
                }
            }
        }
        self.len = CAP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 24 * 3600;

    #[test]
    fn the_key_is_the_first_two_characters_lowercased() {
        assert_eq!(key("FIrefox"), "fi");
        assert_eq!(key("é"), "é");
        assert_eq!(key("  ma"), "ma");
        assert_eq!(key(""), "");
    }

    #[test]
    fn picks_add_up_and_halve_every_seven_days() {
        let mut f = Frecency::default();
        f.record("apps:firefox", "fi", 0);
        f.record("apps:firefox", "fire", 0);
        assert_eq!(f.value("apps:firefox", "fir", 0), 2.0);
        assert!((f.value("apps:firefox", "fi", 7 * DAY) - 1.0).abs() < 1e-9);
        assert!((f.value("apps:firefox", "fi", 14 * DAY) - 0.5).abs() < 1e-9);
        // A pick a week later adds to what is left.
        f.record("apps:firefox", "fi", 7 * DAY);
        assert!((f.value("apps:firefox", "fi", 7 * DAY) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn a_pick_counts_only_for_queries_starting_the_same_way() {
        let mut f = Frecency::default();
        f.record("apps:firefox", "fi", 0);
        assert_eq!(f.value("apps:firefox", "ma", 0), 0.0);
        assert_eq!(f.value("apps:mail", "fi", 0), 0.0);
        assert_eq!(f.totals(0)["apps:firefox"], 1.0);
    }

    #[test]
    fn entries_survive_storing() {
        let mut f = Frecency::default();
        f.record("a", "x", 10);
        f.record("b", "", 20);
        let restored = Frecency::from_entries(f.entries());
        assert_eq!(restored.entries(), f.entries());
        assert_eq!(restored.len(), 2);
    }

    #[test]
    fn bad_stored_scores_are_dropped() {
        let entries = vec![
            Entry {
                id: "a".into(),
                key: "x".into(),
                score: f64::NAN,
                at: 0,
            },
            Entry {
                id: "b".into(),
                key: "x".into(),
                score: -1.0,
                at: 0,
            },
            Entry {
                id: "c".into(),
                key: "x".into(),
                score: 1.0,
                at: 0,
            },
        ];
        assert_eq!(Frecency::from_entries(entries).len(), 1);
    }

    #[test]
    fn the_least_recent_go_first_past_the_cap() {
        let mut f = Frecency::default();
        for i in 0..CAP as u64 + 10 {
            f.record(&format!("item{i}"), "q", i);
        }
        assert_eq!(f.len(), CAP);
        assert_eq!(f.value("item0", "q", 0), 0.0, "oldest dropped");
        assert!(f.value(&format!("item{}", CAP + 9), "q", CAP as u64 + 9) > 0.0);
        assert_eq!(f.entries().len(), CAP);
    }
}
