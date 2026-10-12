//! The `SPC` leader: Fenix's key trie over the user's `[leader]` map, and
//! what the which-key popup shows at each step.
//!
//! The trie is `fenix_keymap::KeyTrie`, so a sequence matches, waits or
//! fails exactly as it does in Fenix. Its labels are `&'static str`,
//! because Fenix's bindings are compiled in; Flint's come from
//! `settings.toml`. Until `fenix-keymap` takes owned labels (F10), the
//! labels live in an arena owned next to the trie, which is rebuilt (not
//! grown) when settings are reloaded: a new [`Leader`] replaces the old one
//! whole, and with it its arena. Nothing borrowed from the arena leaves
//! this type; hints are handed out as owned strings.
//!
//! The trie maps sequences to an index into the daemon's own list of
//! bindings: what a binding *does* (scope a query, run a command) is the
//! daemon's business, not the window's.

use fenix_keymap::{KeyPress, KeyTrie, Matcher, Step};

/// One binding or group label, as the daemon resolved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaderEntry {
    /// The keys after `SPC`.
    pub keys: Vec<char>,
    /// What the popup says.
    pub label: String,
    /// The daemon's index for it; `None` for a group label.
    pub target: Option<usize>,
}

/// One line of the which-key popup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    /// The key to press.
    pub key: char,
    /// What it does, or the group's name.
    pub label: String,
    /// It opens more keys rather than running something.
    pub group: bool,
    /// For a group, how many bindings are under it.
    pub count: usize,
}

/// What a key did to a pending sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeaderStep {
    /// The sequence names binding `n`.
    Matched(usize),
    /// Valid so far; more keys to come.
    Pending,
    /// Not bound.
    NoMatch,
}

/// The leader map.
pub struct Leader {
    // Declared first, so dropped first: it borrows from `_arena`.
    trie: KeyTrie<usize>,
    /// The labels the trie points into. Each `Box<str>`'s contents stay
    /// put when the `Vec` grows, which is what makes the `'static` borrows
    /// in `trie` sound for as long as `self` lives.
    _arena: Vec<Box<str>>,
    bindings: usize,
}

impl Default for Leader {
    fn default() -> Self {
        Leader::new(Vec::new())
    }
}

impl std::fmt::Debug for Leader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Leader")
            .field("bindings", &self.bindings)
            .finish()
    }
}

fn press(c: char) -> KeyPress {
    KeyPress::char(c)
}

impl Leader {
    /// Builds the trie from the daemon's entries.
    pub fn new(entries: Vec<LeaderEntry>) -> Leader {
        let mut arena: Vec<Box<str>> = Vec::with_capacity(entries.len());
        let mut trie = KeyTrie::new();
        let mut bindings = 0;
        for entry in entries {
            arena.push(entry.label.into_boxed_str());
            let stored: &str = arena.last().map_or("", |b| b);
            // SAFETY: the string lives in a `Box<str>` in `arena`, which is
            // dropped after `trie` (field order) and never mutated or
            // shrunk; the borrow never escapes `Leader` (hints are copied
            // out as `String`s).
            let label: &'static str = unsafe { &*(stored as *const str) };
            let keys: Vec<KeyPress> = entry.keys.iter().copied().map(press).collect();
            match entry.target {
                Some(target) => {
                    trie.insert(&keys, label, target);
                    bindings += 1;
                }
                None => trie.label_group(&keys, label),
            }
        }
        Leader {
            trie,
            _arena: arena,
            bindings,
        }
    }

    /// Whether anything is bound.
    pub fn is_empty(&self) -> bool {
        self.bindings == 0
    }

    fn matcher_at(&self, path: &[char]) -> Option<Matcher<'_, usize>> {
        let mut matcher = self.trie.matcher();
        for &key in path {
            match matcher.feed(press(key)) {
                Step::Pending(_) => {}
                _ => return None,
            }
        }
        Some(matcher)
    }

    /// Feeds `key` after `path` (the keys already pressed after `SPC`).
    pub fn step(&self, path: &[char], key: char) -> LeaderStep {
        let Some(mut matcher) = self.matcher_at(path) else {
            return LeaderStep::NoMatch;
        };
        match matcher.feed(press(key)) {
            Step::Matched(&target) => LeaderStep::Matched(target),
            Step::Pending(_) => LeaderStep::Pending,
            Step::NoMatch => LeaderStep::NoMatch,
        }
    }

    /// What the popup lists after `path`, in key order (case-insensitive,
    /// lowercase first), so a key never moves around between opens.
    pub fn hints(&self, path: &[char]) -> Vec<Hint> {
        let Some(matcher) = self.matcher_at(path) else {
            return Vec::new();
        };
        let mut hints: Vec<Hint> = matcher
            .pending_hints()
            .into_iter()
            .filter_map(|h| match h.key.code {
                fenix_keymap::KeyCode::Char(key) => Some(Hint {
                    key,
                    label: h.label.to_owned(),
                    group: h.group,
                    count: h.count,
                }),
                fenix_keymap::KeyCode::Named(_) => None,
            })
            .collect();
        hints.sort_by_key(|h| (h.key.to_ascii_lowercase(), h.key.is_uppercase()));
        hints
    }

    /// The name of the group `path` is in, when it has one.
    pub fn group(&self, path: &[char]) -> Option<String> {
        self.matcher_at(path)
            .and_then(|m| m.label())
            .filter(|l| !l.is_empty())
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(keys: &str, label: &str, target: Option<usize>) -> LeaderEntry {
        LeaderEntry {
            keys: keys.chars().filter(|c| *c != ' ').collect(),
            label: label.into(),
            target,
        }
    }

    fn leader() -> Leader {
        Leader::new(vec![
            entry("p", "Projects", Some(0)),
            entry("f r", "Recent files", Some(1)),
            entry("f f", "Find file", Some(2)),
            entry("f", "Fenix", None),
            entry("P", "Pinned", Some(3)),
        ])
    }

    #[test]
    fn sequences_match_wait_or_fail_like_fenix() {
        let l = leader();
        assert_eq!(l.step(&[], 'p'), LeaderStep::Matched(0));
        assert_eq!(l.step(&[], 'f'), LeaderStep::Pending);
        assert_eq!(l.step(&['f'], 'r'), LeaderStep::Matched(1));
        assert_eq!(l.step(&['f'], 'x'), LeaderStep::NoMatch);
        assert_eq!(l.step(&[], 'z'), LeaderStep::NoMatch);
        assert_eq!(
            l.step(&['z'], 'r'),
            LeaderStep::NoMatch,
            "a path that went nowhere"
        );
    }

    #[test]
    fn hints_list_keys_in_a_stable_order_with_groups_counted() {
        let l = leader();
        let top = l.hints(&[]);
        let keys: Vec<char> = top.iter().map(|h| h.key).collect();
        assert_eq!(keys, ['f', 'p', 'P']);
        assert!(top[0].group);
        assert_eq!((top[0].label.as_str(), top[0].count), ("Fenix", 2));
        assert_eq!(l.hints(&['f']).len(), 2);
        assert_eq!(l.group(&['f']).as_deref(), Some("Fenix"));
    }

    #[test]
    fn labels_outlive_a_rebuild() {
        let mut l = leader();
        let before = l.hints(&[]);
        l = Leader::new(vec![entry("x", "Rebuilt", Some(9))]);
        assert_eq!(l.hints(&[])[0].label, "Rebuilt");
        assert_eq!(before[1].label, "Projects", "hints are owned copies");
        assert!(Leader::default().is_empty());
    }
}
