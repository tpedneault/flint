//! The candidate store and the search over it.
//!
//! Each provider's candidates are replaced as a whole when it reports new
//! ones. A search scores every candidate in scope with `nucleo-matcher`
//! (smart case, Unicode normalisation, a bonus for matching at the start),
//! adds frecency and the provider's boost, and returns the best `limit`
//! with match positions for highlighting.
//!
//! Several things keep a keystroke inside the 16 ms budget at 100k
//! candidates:
//!
//! - Haystacks are converted to nucleo's UTF-32 form once, when a provider
//!   reports them, not per search.
//! - Typing one more character usually narrows the query, so when the new
//!   text extends the previous search's (same candidates, same scope,
//!   nothing appended that nucleo's pattern syntax treats specially), only
//!   the previous matches are scored again.
//! - Frecency is resolved to candidate positions once per search (at most
//!   5,000 entries), instead of a string-keyed lookup per candidate.
//! - A wide search is split across threads, each with its own matcher;
//!   below [`PARALLEL_FROM`] candidates threads cost more than they save.
//! - Ranking selects the top `limit` without sorting everything, and
//!   highlights are computed only for those.
//!
//! The score, as DESIGN.md writes it:
//! `match × W_MATCH + ln(1 + frecency) × W_FRECENCY + provider boost`.
//! The title is matched fuzzily. Keywords and subtitle are matched by
//! substring (each word of the query must appear in them as typed, case
//! aside) and count at [`SECONDARY_WEIGHT`]: fuzzy matching across a
//! subtitle's many words finds nearly anything in it (`fire` in "flint:
//! read settings again"), which is noise, while typing `browser` and
//! finding Edge by its keyword is the point. Context boosts arrive with
//! Fenix's context in M7. Ties go to the shorter title, then
//! alphabetical, so the order never depends on the order providers
//! reported in.

use std::cmp::Ordering;
use std::collections::HashMap;

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32String};

use crate::frecency::{self, Frecency};

/// Weight of nucleo's match score.
pub const W_MATCH: f64 = 1.0;
/// Weight of `ln(1 + frecency)`: one recent pick is worth about a good
/// match's bonus for a prefix (~40), ten are worth more than the gap
/// between a fair and an excellent match.
pub const W_FRECENCY: f64 = 60.0;
/// What a match on keywords or subtitle counts for, against the title.
pub const SECONDARY_WEIGHT: f64 = 0.5;
/// Searches over at least this many candidates are split across threads.
pub const PARALLEL_FROM: usize = 20_000;

/// Something that can be found.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Stable across sessions; frecency is keyed on it.
    pub id: String,
    /// The provider's id, which scopes match against.
    pub provider: String,
    /// Matched in full and highlighted.
    pub title: String,
    /// Keywords and subtitle, matched at a lower weight, not highlighted.
    pub secondary: String,
    /// Added to the score: how much this provider's items should rise.
    pub boost: f64,
}

/// A search result.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// The candidate's id.
    pub id: String,
    /// The final score, higher first.
    pub score: f64,
    /// Character positions in the title that matched, ascending.
    pub highlights: Vec<u32>,
}

/// What to search for.
#[derive(Debug, Clone, Copy)]
pub struct Search<'a> {
    /// The query text (after any scope keyword).
    pub text: &'a str,
    /// Only this provider's candidates, when scoped.
    pub scope: Option<&'a str>,
    /// How many hits to return.
    pub limit: usize,
    /// Now, in seconds since the Unix epoch, for frecency.
    pub now: u64,
}

struct Entry {
    candidate: Candidate,
    title: Utf32String,
    secondary: Utf32String,
}

/// What the previous search matched, for narrowing.
struct Previous {
    generation: u64,
    scope: Option<String>,
    text: String,
    matched: Vec<u32>,
}

/// The candidates of every provider.
pub struct Index {
    entries: Vec<Entry>,
    /// Each candidate's position in `entries`, by id.
    positions: HashMap<String, u32>,
    generation: u64,
    /// One matcher per thread a search may use; a matcher holds scratch
    /// space and isn't shared.
    matchers: Vec<Matcher>,
    previous: Option<Previous>,
}

fn matcher() -> Matcher {
    let mut config = Config::DEFAULT;
    config.prefer_prefix = true;
    Matcher::new(config)
}

impl Default for Index {
    fn default() -> Self {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, 8);
        Index {
            entries: Vec::new(),
            positions: HashMap::new(),
            generation: 0,
            matchers: (0..threads).map(|_| matcher()).collect(),
            previous: None,
        }
    }
}

impl Index {
    /// An empty index.
    pub fn new() -> Self {
        Self::default()
    }

    /// How many candidates there are.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are none.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Replaces all of `provider`'s candidates with `candidates`.
    pub fn set_provider(&mut self, provider: &str, candidates: Vec<Candidate>) {
        self.entries.retain(|e| e.candidate.provider != provider);
        self.entries
            .extend(candidates.into_iter().map(|mut candidate| {
                candidate.provider = provider.to_owned();
                Entry {
                    title: Utf32String::from(candidate.title.as_str()),
                    secondary: Utf32String::from(candidate.secondary.as_str()),
                    candidate,
                }
            }));
        self.positions = self
            .entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.candidate.id.clone(), i as u32))
            .collect();
        self.generation += 1;
        self.previous = None;
    }

    /// The candidate with this id, if any.
    pub fn get(&self, id: &str) -> Option<&Candidate> {
        let &i = self.positions.get(id)?;
        Some(&self.entries[i as usize].candidate)
    }

    /// Searches, best first.
    pub fn search(&mut self, search: Search<'_>, frecency: &Frecency) -> Vec<Hit> {
        let pattern = Pattern::parse(search.text, CaseMatching::Smart, Normalization::Smart);
        let secondary = Pattern::new(
            search.text,
            CaseMatching::Smart,
            Normalization::Smart,
            AtomKind::Substring,
        );
        let empty = search.text.trim().is_empty();

        // Frecency by position: at most CAP lookups, not one per candidate.
        let mut frecency_at = vec![0.0f64; self.entries.len()];
        let mut place = |id: &str, value: f64| {
            if let Some(&i) = self.positions.get(id) {
                frecency_at[i as usize] = value;
            }
        };
        if empty {
            for (id, value) in frecency.totals(search.now) {
                place(id, value);
            }
        } else {
            for (id, value) in frecency.for_key(&frecency::key(search.text), search.now) {
                place(id, value);
            }
        }

        let narrowed = self
            .narrowable(search)
            .map(|p| std::mem::take(&mut p.matched));
        let all: Vec<u32>;
        let indices: &[u32] = match &narrowed {
            Some(matched) => matched,
            None => {
                all = (0..self.entries.len() as u32).collect();
                &all
            }
        };

        let scan = Scan {
            entries: &self.entries,
            pattern: &pattern,
            secondary: &secondary,
            scope: search.scope,
            frecency: &frecency_at,
        };
        let threads = self.matchers.len();
        let (matched, mut scored) = if indices.len() >= PARALLEL_FROM && threads > 1 {
            let chunk = indices.len().div_ceil(threads);
            let parts: Vec<Scanned> = std::thread::scope(|scope| {
                let workers: Vec<_> = indices
                    .chunks(chunk)
                    .zip(self.matchers.iter_mut())
                    .map(|(part, matcher)| {
                        let scan = &scan;
                        scope.spawn(move || scan.run(part, matcher))
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|w| w.join().unwrap_or_default())
                    .collect()
            });
            let mut matched = Vec::with_capacity(parts.iter().map(|p| p.0.len()).sum());
            let mut scored = Vec::with_capacity(matched.capacity());
            for (m, s) in parts {
                matched.extend(m);
                scored.extend(s);
            }
            (matched, scored)
        } else {
            scan.run(indices, &mut self.matchers[0])
        };

        self.previous = Some(Previous {
            generation: self.generation,
            scope: search.scope.map(str::to_owned),
            text: search.text.to_owned(),
            matched,
        });

        let entries = &self.entries;
        let order = |a: &(f64, u32), b: &(f64, u32)| -> Ordering {
            let (ca, cb) = (
                &entries[a.1 as usize].candidate,
                &entries[b.1 as usize].candidate,
            );
            b.0.total_cmp(&a.0)
                .then_with(|| ca.title.len().cmp(&cb.title.len()))
                .then_with(|| ca.title.cmp(&cb.title))
                .then_with(|| ca.id.cmp(&cb.id))
        };
        if scored.len() > search.limit && search.limit > 0 {
            scored.select_nth_unstable_by(search.limit - 1, order);
            scored.truncate(search.limit);
        }
        scored.truncate(search.limit);
        scored.sort_unstable_by(order);

        scored
            .into_iter()
            .map(|(score, i)| {
                let entry = &self.entries[i as usize];
                let mut highlights = Vec::new();
                if !empty {
                    let _ = pattern.indices(
                        entry.title.slice(..),
                        &mut self.matchers[0],
                        &mut highlights,
                    );
                    highlights.sort_unstable();
                    highlights.dedup();
                }
                Hit {
                    id: entry.candidate.id.clone(),
                    score,
                    highlights,
                }
            })
            .collect()
    }

    /// The previous search, when this one only narrows it.
    fn narrowable(&mut self, search: Search<'_>) -> Option<&mut Previous> {
        let previous = self.previous.as_mut()?;
        let appended = search.text.strip_prefix(previous.text.as_str())?;
        let same = previous.generation == self.generation
            && previous.scope.as_deref() == search.scope
            && !previous.text.trim().is_empty()
            && !previous.text.ends_with('\\');
        // nucleo's pattern syntax: `'` exact, `^` prefix, `$` suffix, `!`
        // negation, `\` escape. Any of them can change what earlier atoms
        // mean, so their arrival means a full search.
        let plain = !appended.contains(['\'', '^', '$', '!', '\\']);
        (same && plain).then_some(previous)
    }
}

/// What a scan found: the positions that matched, and each one's score.
type Scanned = (Vec<u32>, Vec<(f64, u32)>);

/// One search's scoring of a slice of candidates, shareable between
/// threads.
struct Scan<'a> {
    entries: &'a [Entry],
    pattern: &'a Pattern,
    secondary: &'a Pattern,
    scope: Option<&'a str>,
    frecency: &'a [f64],
}

impl Scan<'_> {
    /// The positions that matched, and their scores.
    fn run(&self, indices: &[u32], matcher: &mut Matcher) -> Scanned {
        let mut matched = Vec::new();
        let mut scored = Vec::new();
        for &i in indices {
            let entry = &self.entries[i as usize];
            if self.scope.is_some_and(|s| s != entry.candidate.provider) {
                continue;
            }
            let quality = match self.pattern.score(entry.title.slice(..), matcher) {
                Some(score) => f64::from(score),
                None => match self.secondary.score(entry.secondary.slice(..), matcher) {
                    Some(score) => f64::from(score) * SECONDARY_WEIGHT,
                    None => continue,
                },
            };
            matched.push(i);
            let frecency = self.frecency[i as usize];
            let score = quality * W_MATCH + frecency.ln_1p() * W_FRECENCY + entry.candidate.boost;
            scored.push((score, i));
        }
        (matched, scored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(provider: &str, id: &str, title: &str) -> Candidate {
        Candidate {
            id: id.into(),
            provider: provider.into(),
            title: title.into(),
            secondary: String::new(),
            boost: 0.0,
        }
    }

    fn index() -> Index {
        let mut index = Index::new();
        index.set_provider(
            "apps",
            vec![
                candidate("apps", "apps:firefox", "Firefox"),
                candidate("apps", "apps:files", "Files"),
                candidate("apps", "apps:fenix", "Fenix"),
                Candidate {
                    secondary: "browser web".into(),
                    ..candidate("apps", "apps:edge", "Microsoft Edge")
                },
            ],
        );
        index.set_provider(
            "flint",
            vec![candidate("flint", "flint:quit", "Quit Flint")],
        );
        index
    }

    fn ids(hits: &[Hit]) -> Vec<&str> {
        hits.iter().map(|h| h.id.as_str()).collect()
    }

    fn search<'a>(text: &'a str, scope: Option<&'a str>) -> Search<'a> {
        Search {
            text,
            scope,
            limit: 10,
            now: 0,
        }
    }

    #[test]
    fn the_best_match_comes_first_with_highlights() {
        let mut idx = index();
        let hits = idx.search(search("frfx", None), &Frecency::default());
        assert_eq!(ids(&hits), ["apps:firefox"]);
        assert_eq!(hits[0].highlights, [0, 2, 4, 6]);
    }

    #[test]
    fn keywords_and_subtitles_match_at_lower_weight_without_highlights() {
        let mut idx = index();
        let hits = idx.search(search("browser", None), &Frecency::default());
        assert_eq!(ids(&hits), ["apps:edge"]);
        assert!(hits[0].highlights.is_empty());
        assert_eq!(
            ids(&idx.search(search("web brow", None), &Frecency::default())),
            ["apps:edge"]
        );
    }

    #[test]
    fn keywords_and_subtitles_need_the_words_as_typed() {
        let mut idx = index();
        // b-r-w-s scattered through "browser web" is no match there.
        assert!(idx
            .search(search("brws", None), &Frecency::default())
            .is_empty());
    }

    #[test]
    fn a_scope_keeps_one_provider() {
        let mut idx = index();
        let hits = idx.search(search("f", Some("flint")), &Frecency::default());
        assert_eq!(ids(&hits), ["flint:quit"]);
    }

    #[test]
    fn frecency_lifts_what_was_chosen_for_queries_starting_that_way() {
        let mut idx = index();
        let mut f = Frecency::default();
        let before = idx.search(search("fi", None), &f);
        assert_eq!(before[0].id, "apps:files", "shorter title wins the tie");
        f.record("apps:firefox", "fi", 0);
        assert_eq!(idx.search(search("fi", None), &f)[0].id, "apps:firefox");
        assert_eq!(
            idx.search(search("fil", None), &f)[0].id,
            "apps:files",
            "only `fi…` queries"
        );
    }

    #[test]
    fn an_empty_query_lists_everything_most_frecent_first() {
        let mut idx = index();
        let mut f = Frecency::default();
        f.record("flint:quit", "q", 0);
        let hits = idx.search(search("", None), &f);
        assert_eq!(hits.len(), 5);
        assert_eq!(hits[0].id, "flint:quit");
        assert!(hits.iter().all(|h| h.highlights.is_empty()));
        // Then alphabetically, shortest first among equals.
        assert_eq!(ids(&hits)[1..3], ["apps:fenix", "apps:files"]);
    }

    #[test]
    fn the_limit_keeps_the_best() {
        let mut idx = index();
        let hits = idx.search(
            Search {
                limit: 2,
                ..search("f", None)
            },
            &Frecency::default(),
        );
        let all = idx.search(search("f", None), &Frecency::default());
        assert_eq!(hits, all[..2]);
    }

    #[test]
    fn narrowing_gives_the_same_results_as_a_full_search() {
        let mut idx = index();
        let f = Frecency::default();
        for text in ["f", "fi", "fir", "fire", "fire x", "Fire"] {
            let narrowed = idx.search(search(text, None), &f);
            let mut fresh = index();
            assert_eq!(narrowed, fresh.search(search(text, None), &f), "{text}");
        }
    }

    #[test]
    fn special_characters_and_changed_candidates_search_afresh() {
        let mut idx = index();
        let f = Frecency::default();
        idx.search(search("fi", None), &f);
        // `!fox` excludes Firefox: a narrowed search would wrongly
        // keep looking only at what `fi` matched, which is fine here,
        // but `^` and `$` can match what `fi` alone didn't rank.
        assert!(idx.narrowable(search("fi !fox", None)).is_none());
        assert!(idx.narrowable(search("fix", None)).is_some());
        idx.set_provider("flint", vec![]);
        assert!(idx.narrowable(search("fix", None)).is_none());
    }
}
