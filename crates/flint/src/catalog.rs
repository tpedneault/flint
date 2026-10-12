//! Everything Flint can find, and what the window currently shows.
//!
//! The catalog holds each provider's items (as `flint_protocol::Item`s),
//! feeds their titles, keywords and subtitles to `flint-index`, and maps
//! hits back to items and rows. Providers are the built-in ones and the
//! external ones that have registered (M5); an external provider's
//! *static* items live here like any other's, and its *dynamic* results
//! (answers to the current query) are kept for that query only and ranked
//! with the rest.
//!
//! It owns the scope keywords: the built-in providers' prefixes first, then
//! external providers' (a clash with an earlier one is refused, with the
//! reason), then each quicklink's keyword (likewise). And it keeps the
//! frecency table, so recording a pick and ranking by it can't disagree
//! about the key.
//!
//! No window, no OS, no threads: the daemon calls it from the event loop
//! and hands its rows to the GUI; tests call it directly.

use std::collections::HashMap;

use flint_config::Quicklink;
use flint_fenix::items as fenix;
use flint_gui::{Chip, MenuAction, Row};
use flint_index::{Candidate, Frecency, Hit, Index, Search};
use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;
use flint_providers::{apps, commands, quicklinks};

/// How many results a search returns; the window scrolls through them.
pub const LIMIT: usize = 100;

/// A provider as the catalog knows it.
#[derive(Debug, Clone, PartialEq)]
struct Provider {
    id: String,
    title: String,
    prefix: Option<String>,
    /// Added to its items' scores. Flint's own commands sit a little
    /// below apps of the same match, so `q` finds an app called Quake
    /// before Quit Flint.
    boost: f64,
    /// A provider process (M5), which carries out its own `provider`
    /// actions.
    external: bool,
}

fn builtins() -> Vec<Provider> {
    let builtin = |id: &str, title: &str, prefix: Option<&str>, boost: f64| Provider {
        id: id.to_owned(),
        title: title.to_owned(),
        prefix: prefix.map(str::to_owned),
        boost,
        external: false,
    };
    vec![
        builtin(apps::ID, apps::TITLE, Some(apps::PREFIX), 0.0),
        builtin(commands::ID, commands::TITLE, Some(commands::PREFIX), -5.0),
        builtin(quicklinks::ID, quicklinks::TITLE, None, 0.0),
        builtin(
            fenix::PROJECTS,
            fenix::PROJECTS_TITLE,
            Some(fenix::PROJECTS_PREFIX),
            0.0,
        ),
        builtin(
            fenix::RECENT,
            fenix::RECENT_TITLE,
            Some(fenix::RECENT_PREFIX),
            0.0,
        ),
    ]
}

/// What a scope keyword limits the query to.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Scope {
    Provider(String),
    Quicklink(Quicklink),
}

/// A row the window shows, with the item behind it.
#[derive(Debug, Clone)]
pub struct Shown {
    /// The item.
    pub item: Item,
    /// Its provider's id.
    pub provider: String,
    /// Matched character positions in the title.
    pub highlights: Vec<u32>,
    /// Its rank; dynamic results that didn't match the text sort last.
    score: f64,
}

/// Why this Flint can't carry out `action` of an item from `provider`
/// (`external` when it is a provider process), if it can't: the action
/// menu shows it disabled with that reason, and running it anyway (as
/// `Enter` might) gives the same reason.
pub fn unavailable(provider: &str, external: bool, action: &Action) -> Option<String> {
    match &action.kind {
        ActionKind::Fenix { method, .. } => flint_fenix::handoff::available(method).err(),
        ActionKind::Provider if provider != commands::ID && !external => Some(format!(
            "`{}` has no provider to carry it out",
            action.title
        )),
        ActionKind::Unknown => Some(format!(
            "`{}` is a kind of action this Flint doesn't know",
            action.title
        )),
        _ => None,
    }
}

/// The catalog.
pub struct Catalog {
    index: Index,
    providers: Vec<Provider>,
    items: HashMap<String, (Item, String)>,
    links: Vec<Quicklink>,
    keywords: Vec<(String, String, Scope)>,
    /// External providers whose prefix was refused, and why.
    prefix_problems: Vec<String>,
    /// Dynamic results for `dynamic_query`, in arrival order.
    dynamic: Vec<(String, Item)>,
    dynamic_query: u64,
    dynamic_index: Index,
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
            providers: builtins(),
            items: HashMap::new(),
            links: Vec::new(),
            keywords: Vec::new(),
            prefix_problems: Vec::new(),
            dynamic: Vec::new(),
            dynamic_query: 0,
            dynamic_index: Index::new(),
            frecency,
            shown: Vec::new(),
            shown_for: String::new(),
        };
        catalog.set_items(commands::ID, commands::items());
        catalog.rebuild_keywords();
        catalog
    }

    fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// Whether `id` is a provider process.
    pub fn is_external(&self, id: &str) -> bool {
        self.provider(id).is_some_and(|p| p.external)
    }

    /// A provider's title (its badge), or its id when unknown.
    pub fn title_of(&self, id: &str) -> String {
        self.provider(id)
            .map_or_else(|| id.to_owned(), |p| p.title.clone())
    }

    /// Replaces a provider's items. Items that break the protocol's limits
    /// are dropped and named in the returned lines.
    pub fn set_items(&mut self, provider_id: &str, items: Vec<Item>) -> Vec<String> {
        self.set_ranked(provider_id, items.into_iter().map(|i| (i, 0.0)).collect())
    }

    /// [`Catalog::set_items`], each item with its own boost on top of its
    /// provider's: a pinned project, a more recent file.
    pub fn set_ranked(&mut self, provider_id: &str, items: Vec<(Item, f64)>) -> Vec<String> {
        let Some(p) = self.provider(provider_id).cloned() else {
            return vec![format!("no provider `{provider_id}`")];
        };
        self.items.retain(|_, (_, owner)| *owner != p.id);
        let mut problems = Vec::new();
        let mut candidates = Vec::with_capacity(items.len());
        for (item, boost) in items {
            if let Err(why) = item.check() {
                problems.push(format!("{}: {why}", p.title));
                continue;
            }
            candidates.push(candidate(&item, &p.id, p.boost + boost));
            self.items.insert(item.id.clone(), (item, p.id.clone()));
        }
        self.index.set_provider(&p.id, candidates);
        problems
    }

    /// An external provider registered: its title and the scope keyword it
    /// should have (`[[providers]] prefix`, else its own). Returns a line
    /// when the keyword is taken.
    pub fn register_external(
        &mut self,
        id: &str,
        title: &str,
        prefix: Option<String>,
    ) -> Vec<String> {
        let entry = Provider {
            id: id.to_owned(),
            title: title.to_owned(),
            prefix,
            boost: 0.0,
            external: true,
        };
        match self.providers.iter_mut().find(|p| p.id == id) {
            Some(existing) if existing.external => *existing = entry,
            Some(_) => return vec![format!("provider `{id}` has a built-in provider's name")],
            None => self.providers.push(entry),
        }
        self.rebuild_keywords();
        self.prefix_problems
            .iter()
            .filter(|p| p.contains(&format!("`{id}`")))
            .cloned()
            .collect()
    }

    /// An external provider stopped or was removed: its items go.
    pub fn unregister_external(&mut self, id: &str) {
        if !self.is_external(id) {
            return;
        }
        self.items.retain(|_, (_, owner)| owner != id);
        self.index.set_provider(id, Vec::new());
        self.dynamic.retain(|(owner, _)| owner != id);
        self.reindex_dynamic();
        self.providers.retain(|p| p.id != id);
        self.rebuild_keywords();
    }

    /// Uses these quicklinks; returns a line for each keyword that is
    /// taken.
    pub fn set_quicklinks(&mut self, links: &[Quicklink]) -> Vec<String> {
        self.set_items(quicklinks::ID, links.iter().map(quicklinks::item).collect());
        self.links = links.to_vec();
        self.rebuild_keywords()
    }

    /// Lines for the providers and quicklinks whose keyword is taken.
    pub fn keyword_problems(&self) -> &[String] {
        &self.prefix_problems
    }

    fn rebuild_keywords(&mut self) -> Vec<String> {
        let mut keywords: Vec<(String, String, Scope)> = Vec::new();
        let mut problems = Vec::new();
        for p in &self.providers {
            let Some(prefix) = &p.prefix else { continue };
            match keywords.iter().find(|(k, ..)| k == prefix) {
                Some((_, owner, _)) => problems.push(format!(
                    "provider `{}` can't use prefix `{prefix}`, which {owner} has",
                    p.id
                )),
                None => keywords.push((
                    prefix.clone(),
                    p.title.clone(),
                    Scope::Provider(p.id.clone()),
                )),
            }
        }
        let mut link_problems = Vec::new();
        for link in &self.links {
            match keywords.iter().find(|(k, ..)| *k == link.keyword) {
                Some((_, owner, _)) => link_problems.push(format!(
                    "quicklink `{}` can't use keyword `{}`, which {owner} has; find it by name instead",
                    link.name, link.keyword
                )),
                None => keywords.push((link.keyword.clone(), link.name.clone(), Scope::Quicklink(link.clone()))),
            }
        }
        self.keywords = keywords;
        self.prefix_problems = problems;
        link_problems
    }

    /// The chip for a leader's `scope:` target: a provider's id (`apps`,
    /// `fenix.recent`, an external one) or `quicklink.<keyword>`.
    pub fn chip_for(&self, target: &str) -> Option<Chip> {
        let wanted = match target.strip_prefix("quicklink.") {
            Some(keyword) => keyword.to_owned(),
            None => self.provider(target)?.prefix.clone()?,
        };
        self.keywords
            .iter()
            .find(|(k, ..)| *k == wanted)
            .map(|(keyword, label, _)| Chip {
                keyword: keyword.clone(),
                label: label.clone(),
            })
    }

    /// The provider a scope keyword names, if it names one.
    pub fn provider_for(&self, keyword: &str) -> Option<String> {
        self.keywords.iter().find_map(|(k, _, scope)| match scope {
            Scope::Provider(id) if k == keyword => Some(id.clone()),
            _ => None,
        })
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

    /// A new query was sent to the providers: earlier dynamic results go.
    pub fn begin_query(&mut self, query_id: u64) {
        self.dynamic_query = query_id;
        self.dynamic.clear();
        self.reindex_dynamic();
    }

    /// Dynamic results arrived. Ignored unless they answer the current
    /// query; returns whether they did. Items breaking the protocol's
    /// limits are dropped.
    pub fn add_results(&mut self, provider: &str, query_id: u64, items: Vec<Item>) -> bool {
        if query_id != self.dynamic_query || !self.is_external(provider) {
            return false;
        }
        for item in items.into_iter().filter(|i| i.check().is_ok()) {
            self.dynamic.retain(|(_, existing)| existing.id != item.id);
            self.dynamic.push((provider.to_owned(), item));
        }
        self.reindex_dynamic();
        true
    }

    fn reindex_dynamic(&mut self) {
        let mut by_provider: HashMap<&str, Vec<Candidate>> = HashMap::new();
        for (provider, item) in &self.dynamic {
            by_provider
                .entry(provider)
                .or_default()
                .push(candidate(item, provider, 0.0));
        }
        self.dynamic_index = Index::new();
        for (provider, candidates) in by_provider {
            self.dynamic_index.set_provider(provider, candidates);
        }
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
                provider: quicklinks::ID.to_owned(),
                highlights: vec![],
                score: 0.0,
            }],
            Some(Scope::Provider(id)) => self.hits(text, Some(&id), now),
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
        self.shown.iter().map(|s| self.row(s)).collect()
    }

    fn row(&self, shown: &Shown) -> Row {
        let external = self.is_external(&shown.provider);
        Row {
            title: shown.item.title.clone(),
            subtitle: shown.item.subtitle.clone().unwrap_or_default(),
            badge: self.title_of(&shown.provider),
            highlights: shown.highlights.clone(),
            actions: shown
                .item
                .actions
                .iter()
                .map(|a| MenuAction {
                    title: a.title.clone(),
                    key: a.key.as_deref().and_then(|k| k.chars().next()),
                    disabled: unavailable(&shown.provider, external, a),
                })
                .collect(),
            dimmed: false,
        }
    }

    /// The item behind row `row`.
    pub fn shown(&self, row: usize) -> Option<&Shown> {
        self.shown.get(row)
    }

    /// The text the shown rows were found for.
    pub fn shown_for(&self) -> &str {
        &self.shown_for
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
            .filter(|(_, p)| p == provider_id)
            .count()
    }

    /// Static hits and dynamic results, by score; dynamic results that
    /// don't match the text (a provider's own search found them) after
    /// the rest, in the order they came.
    fn hits(&mut self, text: &str, scope: Option<&str>, now: u64) -> Vec<Shown> {
        let search = Search {
            text,
            scope,
            limit: LIMIT,
            now,
        };
        let mut shown: Vec<Shown> = Vec::new();
        for hit in self.index.search(search, &self.frecency) {
            if let Some((item, provider)) = self.items.get(&hit.id) {
                shown.push(Shown {
                    item: item.clone(),
                    provider: provider.clone(),
                    highlights: hit.highlights,
                    score: hit.score,
                });
            }
        }
        if !self.dynamic.is_empty() {
            let matched = self.dynamic_index.search(search, &self.frecency);
            for hit in &matched {
                if let Some((provider, item)) = self.dynamic.iter().find(|(_, i)| i.id == hit.id) {
                    shown.retain(|s| s.item.id != item.id);
                    shown.push(Shown {
                        item: item.clone(),
                        provider: provider.clone(),
                        highlights: hit.highlights.clone(),
                        score: hit.score,
                    });
                }
            }
            shown.sort_by(|a, b| b.score.total_cmp(&a.score));
            for (provider, item) in &self.dynamic {
                let in_scope = scope.is_none_or(|s| s == provider);
                if in_scope && !shown.iter().any(|s| s.item.id == item.id) {
                    shown.push(Shown {
                        item: item.clone(),
                        provider: provider.clone(),
                        highlights: vec![],
                        score: f64::NEG_INFINITY,
                    });
                }
            }
            shown.truncate(LIMIT);
        }
        shown
    }
}

fn candidate(item: &Item, provider: &str, boost: f64) -> Candidate {
    let mut secondary = item.keywords.join(" ");
    if let Some(subtitle) = &item.subtitle {
        secondary.push(' ');
        secondary.push_str(subtitle);
    }
    Candidate {
        id: item.id.clone(),
        provider: provider.to_owned(),
        title: item.title.clone(),
        secondary,
        boost,
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
    fn fenix_projects_and_recent_files_have_scopes_and_keep_their_order() {
        let mut c = catalog();
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../flint-fenix/tests/fixtures/fenix-home");
        let (state, _) = flint_fenix::state::read(&flint_fenix::Roots::portable(&home));
        c.set_ranked(fenix::PROJECTS, fenix::projects(&state));
        c.set_ranked(fenix::RECENT, fenix::recent(&state));
        assert_eq!(c.split_scope("p x").unwrap().0.label, "Projects");
        assert_eq!(c.split_scope("f x").unwrap().0.label, "Recent files");
        // The pinned project first, then Fenix's own order.
        assert_eq!(
            titles(&c.search(Some("p"), "", 0)),
            ["fenix", "flint", "ops", "scratch"]
        );
        // The most recent file first among equal matches.
        assert_eq!(
            titles(&c.search(Some("f"), "", 0)),
            ["main.rs", "Cargo.toml", "DESIGN.md"]
        );
        assert_eq!(c.search(None, "design", 0)[0].badge, "Recent files");
    }

    #[test]
    fn rows_carry_their_menu_with_reasons_for_what_cant_run() {
        let mut c = catalog();
        let home = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../flint-fenix/tests/fixtures/fenix-home");
        let (state, _) = flint_fenix::state::read(&flint_fenix::Roots::portable(&home));
        c.set_ranked(fenix::PROJECTS, fenix::projects(&state));
        let rows = c.search(Some("p"), "fenix", 0);
        let menu: Vec<(&str, Option<char>, bool)> = rows[0]
            .actions
            .iter()
            .map(|a| (a.title.as_str(), a.key, a.disabled.is_some()))
            .collect();
        assert_eq!(
            menu,
            [
                ("Open folder", Some('o'), false),
                ("Open in Fenix", Some('e'), true),
                ("Terminal here", Some('t'), false),
                ("Copy path", Some('c'), false),
            ]
        );
    }

    #[test]
    fn leader_scopes_name_providers_and_quicklinks() {
        let c = catalog();
        assert_eq!(c.chip_for("apps").unwrap().keyword, "a");
        assert_eq!(c.chip_for("fenix.recent").unwrap().label, "Recent files");
        assert_eq!(c.chip_for("quicklink.j").unwrap().label, "Jira issue");
        assert!(c.chip_for("quicklink.nope").is_none());
        assert!(c.chip_for("nope").is_none());
        assert!(
            c.chip_for("quicklink").is_none(),
            "quicklinks as a whole have no keyword"
        );
    }

    fn external_item(id: &str, title: &str) -> Item {
        Item {
            id: id.into(),
            title: title.into(),
            subtitle: None,
            icon: None,
            keywords: vec![],
            actions: vec![Action {
                id: "start".into(),
                title: "Start timer".into(),
                key: None,
                kind: ActionKind::Provider,
            }],
        }
    }

    #[test]
    fn an_external_provider_registers_a_prefix_and_its_actions_are_live() {
        let mut c = catalog();
        assert!(c
            .register_external("jira", "Jira", Some("ji".into()))
            .is_empty());
        c.set_items(
            "jira",
            vec![external_item("jira:ABC-1", "ABC-1 Fix export")],
        );
        assert_eq!(c.split_scope("ji x").unwrap().0.label, "Jira");
        let rows = c.search(Some("ji"), "", 0);
        assert_eq!(rows[0].badge, "Jira");
        assert_eq!(
            rows[0].actions[0].disabled, None,
            "a provider action of a provider process runs"
        );
        // Taken prefixes are refused with the reason, and the link loses.
        let problems = c.register_external("other", "Other", Some("a".into()));
        assert!(problems[0].contains("which Apps has"), "{problems:?}");
        c.unregister_external("jira");
        assert!(c.split_scope("ji x").is_none());
        assert_eq!(c.count("jira"), 0);
        assert!(c.register_external("apps", "Fake", None)[0].contains("built-in"));
    }

    #[test]
    fn dynamic_results_join_the_ranking_for_their_query_only() {
        let mut c = catalog();
        c.register_external("jira", "Jira", Some("ji".into()));
        c.begin_query(7);
        assert!(
            !c.add_results("jira", 6, vec![external_item("jira:old", "Old")]),
            "stale"
        );
        assert!(c.add_results(
            "jira",
            7,
            vec![
                external_item("jira:ABC-1", "Firewall bug"),
                external_item("jira:ABC-2", "Server-side match"),
            ]
        ));
        let rows = c.search(None, "fire", 0);
        let found = titles(&rows);
        assert!(found.contains(&"Firewall bug"), "{found:?}");
        assert_eq!(
            *found.last().unwrap(),
            "Server-side match",
            "unmatched results come last"
        );
        // Scoped to another provider, Jira's results stay out.
        assert!(!titles(&c.search(Some("a"), "fire", 0)).contains(&"Firewall bug"));
        c.begin_query(8);
        assert!(
            !titles(&c.search(None, "fire", 0)).contains(&"Firewall bug"),
            "a new query clears them"
        );
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
