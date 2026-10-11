//! Everything Flint can find, and what the window currently shows.
//!
//! The catalog holds each provider's items (as `flint_protocol::Item`s),
//! feeds their titles, keywords and subtitles to `flint-index`, and maps
//! hits back to items and rows. It owns the scope keywords: the built-in
//! providers' prefixes first, then each quicklink's keyword, a quicklink
//! that collides with a built-in losing its keyword (reported; the link is
//! still found by name). And it keeps the frecency table, so recording a
//! pick and ranking by it can't disagree about the key.
//!
//! No window, no OS: the daemon calls it from the event loop and hands
//! its rows to the GUI; tests call it directly.

use std::collections::HashMap;

use flint_config::Quicklink;
use flint_gui::{Chip, Row};
use flint_index::{Candidate, Frecency, Hit, Index, Search};
use flint_protocol::Item;
use flint_providers::{apps, commands, quicklinks};

/// How many results a search returns; the window scrolls through them.
pub const LIMIT: usize = 100;

/// A provider as the catalog knows it.
#[derive(Debug, Clone, Copy)]
struct Provider {
    id: &'static str,
    title: &'static str,
    prefix: Option<&'static str>,
    /// Added to its items' scores. Flint's own commands sit a little
    /// below apps of the same match, so `q` finds an app called Quake
    /// before Quit Flint.
    boost: f64,
}

const PROVIDERS: [Provider; 3] = [
    Provider {
        id: apps::ID,
        title: apps::TITLE,
        prefix: Some(apps::PREFIX),
        boost: 0.0,
    },
    Provider {
        id: commands::ID,
        title: commands::TITLE,
        prefix: Some(commands::PREFIX),
        boost: -5.0,
    },
    Provider {
        id: quicklinks::ID,
        title: quicklinks::TITLE,
        prefix: None,
        boost: 0.0,
    },
];

fn provider(id: &str) -> Option<&'static Provider> {
    PROVIDERS.iter().find(|p| p.id == id)
}

/// What a scope keyword limits the query to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    Provider(&'static str),
    Quicklink(Quicklink),
}

/// A row the window shows, with the item behind it.
#[derive(Debug, Clone)]
pub struct Shown {
    /// The item.
    pub item: Item,
    /// Its provider's id.
    pub provider: &'static str,
    /// Matched character positions in the title.
    pub highlights: Vec<u32>,
}

impl Shown {
    fn row(&self) -> Row {
        Row {
            title: self.item.title.clone(),
            subtitle: self.item.subtitle.clone().unwrap_or_default(),
            badge: provider(self.provider).map_or("", |p| p.title).to_owned(),
            highlights: self.highlights.clone(),
        }
    }
}

/// The catalog.
pub struct Catalog {
    index: Index,
    items: HashMap<String, (Item, &'static str)>,
    keywords: Vec<(String, String, Scope)>,
    frecency: Frecency,
    shown: Vec<Shown>,
    /// The text the shown rows were found for, for frecency.
    shown_for: String,
}

impl Catalog {
    /// A catalog with Flint's own commands and no apps or links yet.
    pub fn new(frecency: Frecency) -> Self {
        let mut catalog = Catalog {
            index: Index::new(),
            items: HashMap::new(),
            keywords: Vec::new(),
            frecency,
            shown: Vec::new(),
            shown_for: String::new(),
        };
        catalog.set_items(commands::ID, commands::items());
        catalog.set_quicklinks(&[]);
        catalog
    }

    /// Replaces a provider's items. Items that break the protocol's limits
    /// are dropped and named in the returned lines.
    pub fn set_items(&mut self, provider_id: &str, items: Vec<Item>) -> Vec<String> {
        let Some(p) = provider(provider_id) else {
            return vec![format!("no provider `{provider_id}`")];
        };
        self.items.retain(|_, (_, owner)| *owner != p.id);
        let mut problems = Vec::new();
        let mut candidates = Vec::with_capacity(items.len());
        for item in items {
            if let Err(why) = item.check() {
                problems.push(format!("{}: {why}", p.title));
                continue;
            }
            let mut secondary = item.keywords.join(" ");
            if let Some(subtitle) = &item.subtitle {
                secondary.push(' ');
                secondary.push_str(subtitle);
            }
            candidates.push(Candidate {
                id: item.id.clone(),
                provider: p.id.to_owned(),
                title: item.title.clone(),
                secondary,
                boost: p.boost,
            });
            self.items.insert(item.id.clone(), (item, p.id));
        }
        self.index.set_provider(p.id, candidates);
        problems
    }

    /// Uses these quicklinks; returns a line for each keyword that
    /// collides with a built-in prefix.
    pub fn set_quicklinks(&mut self, links: &[Quicklink]) -> Vec<String> {
        self.set_items(quicklinks::ID, links.iter().map(quicklinks::item).collect());
        self.keywords = PROVIDERS
            .iter()
            .filter_map(|p| {
                p.prefix
                    .map(|k| (k.to_owned(), p.title.to_owned(), Scope::Provider(p.id)))
            })
            .collect();
        let mut problems = Vec::new();
        for link in links {
            if let Some((_, owner, _)) = self.keywords.iter().find(|(k, ..)| *k == link.keyword) {
                problems.push(format!(
                    "quicklink `{}` can't use keyword `{}`, which {owner} has; find it by name instead",
                    link.name, link.keyword
                ));
                continue;
            }
            self.keywords.push((
                link.keyword.clone(),
                link.name.clone(),
                Scope::Quicklink(link.clone()),
            ));
        }
        problems
    }

    /// When `text` starts with a scope keyword and a space, the chip for
    /// it and the rest of the text.
    pub fn split_scope<'t>(&self, text: &'t str) -> Option<(Chip, &'t str)> {
        let (keyword, rest) =
            flint_index::split_scope(text, self.keywords.iter().map(|(k, ..)| k.as_str()))?;
        let label = self
            .keywords
            .iter()
            .find(|(k, ..)| k == keyword)
            .map(|(_, l, _)| l.clone())?;
        Some((
            Chip {
                keyword: keyword.to_owned(),
                label,
            },
            rest,
        ))
    }

    /// Searches `text` within `scope` (a keyword), remembering the results
    /// so a row can be acted on, and returns the rows.
    pub fn search(&mut self, scope: Option<&str>, text: &str, now: u64) -> Vec<Row> {
        let scope = scope
            .and_then(|k| self.keywords.iter().find(|(kw, ..)| kw == k))
            .map(|(_, _, s)| s.clone());
        self.shown_for = text.to_owned();
        self.shown = match scope {
            Some(Scope::Quicklink(link)) => vec![Shown {
                item: quicklinks::filled(&link, text),
                provider: quicklinks::ID,
                highlights: vec![],
            }],
            Some(Scope::Provider(id)) => self.hits(text, Some(id), now),
            None => self.hits(text, None, now),
        };
        self.rows()
    }

    /// New results for the same query while its rows are on screen: the
    /// rows from the top to `selected` keep their places (DESIGN.md,
    /// "Stable merge").
    pub fn merge(
        &mut self,
        scope: Option<&str>,
        text: &str,
        selected: usize,
        now: u64,
    ) -> Vec<Row> {
        let before: Vec<String> = self.shown.iter().map(|s| s.item.id.clone()).collect();
        self.search(scope, text, now);
        let fresh: Vec<Hit> = self
            .shown
            .iter()
            .map(|s| Hit {
                id: s.item.id.clone(),
                score: 0.0,
                highlights: vec![],
            })
            .collect();
        let order = flint_index::stable_merge(&before, selected, fresh);
        let mut by_id: HashMap<String, Shown> = self
            .shown
            .drain(..)
            .map(|s| (s.item.id.clone(), s))
            .collect();
        self.shown = order
            .into_iter()
            .filter_map(|h| by_id.remove(&h.id))
            .collect();
        self.rows()
    }

    /// The rows for what is shown.
    pub fn rows(&self) -> Vec<Row> {
        self.shown.iter().map(Shown::row).collect()
    }

    /// The item behind row `row`.
    pub fn shown(&self, row: usize) -> Option<&Shown> {
        self.shown.get(row)
    }

    /// Records that `id` was chosen for the query the rows were found for.
    pub fn record(&mut self, id: &str, now: u64) {
        let text = self.shown_for.clone();
        self.frecency.record(id, &text, now);
    }

    /// The frecency table, for storing.
    pub fn frecency(&self) -> &Frecency {
        &self.frecency
    }

    /// How many items a provider has.
    pub fn count(&self, provider_id: &str) -> usize {
        self.items
            .values()
            .filter(|(_, p)| *p == provider_id)
            .count()
    }

    fn hits(&mut self, text: &str, scope: Option<&'static str>, now: u64) -> Vec<Shown> {
        let hits = self.index.search(
            Search {
                text,
                scope,
                limit: LIMIT,
                now,
            },
            &self.frecency,
        );
        hits.into_iter()
            .filter_map(|h| {
                let (item, provider) = self.items.get(&h.id)?;
                Some(Shown {
                    item: item.clone(),
                    provider,
                    highlights: h.highlights,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flint_protocol::item::{Action, ActionKind};

    fn app(name: &str) -> Item {
        Item {
            id: format!("apps:{}", name.to_lowercase()),
            title: name.into(),
            subtitle: None,
            icon: None,
            keywords: vec![],
            actions: vec![Action {
                id: "launch".into(),
                title: "Launch".into(),
                key: None,
                kind: ActionKind::Open {
                    target: format!("C:\\{name}.lnk"),
                },
            }],
        }
    }

    fn jira() -> Quicklink {
        Quicklink {
            keyword: "j".into(),
            name: "Jira issue".into(),
            url: "https://jira/browse/{query}".into(),
        }
    }

    fn catalog() -> Catalog {
        let mut c = Catalog::new(Frecency::default());
        c.set_items(apps::ID, vec![app("Firefox"), app("Files"), app("Quake")]);
        c.set_quicklinks(&[jira()]);
        c
    }

    fn titles(rows: &[Row]) -> Vec<&str> {
        rows.iter().map(|r| r.title.as_str()).collect()
    }

    #[test]
    fn unscoped_search_mixes_providers_with_badges() {
        let mut c = catalog();
        let rows = c.search(None, "fi", 0);
        assert_eq!(titles(&rows)[..2], ["Files", "Firefox"]);
        assert_eq!(rows[0].badge, "Apps");
        let rows = c.search(None, "jira", 0);
        assert_eq!(titles(&rows), ["Jira issue"]);
        assert_eq!(rows[0].badge, "Link");
    }

    #[test]
    fn apps_rank_above_flint_commands_for_the_same_match() {
        let mut c = catalog();
        let rows = c.search(None, "q", 0);
        assert_eq!(titles(&rows)[..2], ["Quake", "Quit Flint"]);
    }

    #[test]
    fn scope_keywords_turn_into_chips() {
        let c = catalog();
        let (chip, rest) = c.split_scope("a fire").unwrap();
        assert_eq!(
            (chip.keyword.as_str(), chip.label.as_str(), rest),
            ("a", "Apps", "fire")
        );
        let (chip, rest) = c.split_scope("j ABC-1").unwrap();
        assert_eq!((chip.label.as_str(), rest), ("Jira issue", "ABC-1"));
        assert!(c.split_scope("a").is_none());
        assert!(c.split_scope("x y").is_none());
    }

    #[test]
    fn a_scoped_search_keeps_to_its_provider() {
        let mut c = catalog();
        assert_eq!(titles(&c.search(Some("flint"), "", 0)).len(), 4);
        assert_eq!(titles(&c.search(Some("a"), "q", 0)), ["Quake"]);
    }

    #[test]
    fn a_quicklink_scope_offers_the_filled_link() {
        let mut c = catalog();
        let rows = c.search(Some("j"), "ABC-12", 0);
        assert_eq!(titles(&rows), ["Jira issue: ABC-12"]);
        let shown = c.shown(0).unwrap();
        assert!(
            matches!(&shown.item.actions[0].kind, ActionKind::Open { target } if target == "https://jira/browse/ABC-12")
        );
    }

    #[test]
    fn a_quicklink_cannot_take_a_builtin_prefix() {
        let mut c = Catalog::new(Frecency::default());
        let problems = c.set_quicklinks(&[Quicklink {
            keyword: "a".into(),
            name: "Atlassian".into(),
            url: "https://a".into(),
        }]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("which Apps has"), "{}", problems[0]);
        assert_eq!(c.split_scope("a x").unwrap().0.label, "Apps");
        assert_eq!(
            titles(&c.search(None, "atlas", 0)),
            ["Atlassian"],
            "still found by name"
        );
    }

    #[test]
    fn picks_raise_an_item_for_queries_starting_the_same_way() {
        let mut c = catalog();
        c.search(None, "fi", 0);
        c.record("apps:firefox", 0);
        assert_eq!(c.search(None, "fi", 0)[0].title, "Firefox");
        assert_eq!(c.search(None, "fil", 0)[0].title, "Files");
    }

    #[test]
    fn new_items_land_below_the_selection_while_rows_are_shown() {
        let mut c = catalog();
        let shown = c.search(None, "f", 0);
        assert_eq!(titles(&shown)[..2], ["Files", "Firefox"]);
        // A new app that would rank first arrives while row 1 is selected.
        c.set_items(
            apps::ID,
            vec![app("Firefox"), app("Files"), app("Quake"), app("F")],
        );
        let merged = c.merge(None, "f", 1, 0);
        assert_eq!(titles(&merged)[..3], ["Files", "Firefox", "F"]);
        assert_eq!(
            c.shown(2).unwrap().item.title,
            "F",
            "rows and items stay in step"
        );
        // A new keystroke ranks afresh.
        assert_eq!(c.search(None, "f", 0)[0].title, "F");
    }

    #[test]
    fn items_breaking_the_limits_are_dropped_and_named() {
        let mut c = catalog();
        let mut bad = app("Bad");
        bad.actions.clear();
        let problems = c.set_items(apps::ID, vec![app("Good"), bad]);
        assert_eq!(problems.len(), 1);
        assert!(problems[0].contains("no actions"));
        assert_eq!(c.count(apps::ID), 1);
    }
}
