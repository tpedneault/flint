//! `flint-jira`: Jira issues in Flint, as a provider process.
//!
//! ```toml
//! [[providers]]
//! name = "jira"
//! command = ["flint-jira", "--url", "https://jira.example.com", "--project", "OPS"]
//! credential = "jira:https://jira.example.com"
//! ```
//!
//! - **Static items:** your open issues (assigned to you, not done, in the
//!   `--project`s if any), fetched at start and every five minutes, and
//!   matched by Flint itself as you type, with frecency: no request per
//!   keystroke for the issues you work on.
//! - **Scoped (`j `):** a Jira text search for what you type, newest
//!   first, and an issue key (`ops-123`) looked up directly.
//! - **Unscoped:** only an issue key is looked up, so typing anything
//!   else in Flint never reaches Jira.
//! - Each issue opens in the browser (`Enter`), or copies its key or link.
//!
//! The REST calls, the bearer-token scheme for Jira Server/Data Center and
//! the OS certificate store for TLS (a corporate CA works) are Fenix's
//! `fenix-jira`. The token comes from Flint (`credential`), which keeps it
//! in the OS credential store under the name FENIX-INTEGRATION.md's F12
//! agrees for the suite; this program never stores or prints it.
//! Jira Cloud's email-and-token scheme is out of scope, as it is for
//! `fenix-jira`.

use std::time::Duration;

use fenix_jira::{build_keys_jql, build_my_open_issues_jql, IssueSummary, JiraClient};
use flint_protocol::item::{Action, ActionKind};
use flint_protocol::provider::{Query, Register};
use flint_protocol::Item;
use flint_provider_kit::Provider;

/// The most issues a search or the open-issues list asks for.
const MAX_RESULTS: u32 = 50;

/// The provider, configured from its command line.
pub struct Jira {
    /// Its id (`--name`, default `jira`), as `[[providers]] name` says.
    name: String,
    /// The instance's root, without a trailing slash.
    base: String,
    /// `--project` keys limiting the open issues and searches.
    projects: Vec<String>,
    client: JiraClient,
}

impl Jira {
    /// From the arguments after the program name and the token.
    pub fn from_args(args: &[String], token: String) -> Result<Jira, String> {
        let base = flint_provider_kit::flag_values(args, "--url")
            .pop()
            .map(|u| u.trim_end_matches('/').to_owned())
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .ok_or("needs `--url https://jira.example.com`")?;
        let name = flint_provider_kit::flag_values(args, "--name")
            .pop()
            .unwrap_or_else(|| "jira".into());
        let projects = flint_provider_kit::flag_values(args, "--project")
            .into_iter()
            .map(|p| p.trim().to_ascii_uppercase())
            .filter(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
            .collect();
        Ok(Jira {
            client: JiraClient::new(base.clone(), token),
            name,
            base,
            projects,
        })
    }

    fn item(&self, issue: &IssueSummary) -> Item {
        let link = format!("{}/browse/{}", self.base, issue.key);
        let mut details = vec![issue.status.clone()];
        details.extend(issue.assignee.clone());
        details.extend(issue.issue_type.clone());
        let action = |id: &str, title: &str, key: &str, kind: ActionKind| Action {
            id: id.into(),
            title: title.into(),
            key: Some(key.into()),
            kind,
        };
        Item {
            id: format!("{}:{}", self.name, issue.key),
            title: format!("{} {}", issue.key, issue.summary)
                .chars()
                .take(300)
                .collect(),
            subtitle: Some(details.join(" · ")),
            icon: Some("ticket".into()),
            keywords: vec![issue.key.clone()],
            actions: vec![
                action(
                    "open",
                    "Open in browser",
                    "o",
                    ActionKind::Open {
                        target: link.clone(),
                    },
                ),
                action(
                    "copy-key",
                    "Copy key",
                    "c",
                    ActionKind::Copy {
                        text: issue.key.clone(),
                    },
                ),
                action(
                    "copy-link",
                    "Copy link",
                    "l",
                    ActionKind::Copy { text: link },
                ),
            ],
        }
    }

    /// `project IN (...)` for searches, when limited to projects.
    fn in_projects(&self) -> String {
        if self.projects.is_empty() {
            String::new()
        } else {
            format!(" AND project IN ({})", self.projects.join(","))
        }
    }
}

/// `ABC-123` (any case) as Jira writes it, if `text` is an issue key.
pub fn issue_key(text: &str) -> Option<String> {
    let text = text.trim();
    let (project, number) = text.split_once('-')?;
    let project_ok = project.len() >= 2
        && project.as_bytes()[0].is_ascii_alphabetic()
        && project
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_');
    let number_ok = !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit());
    (project_ok && number_ok).then(|| format!("{}-{number}", project.to_ascii_uppercase()))
}

/// `text` as a JQL string literal.
pub fn jql_string(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Provider for Jira {
    fn register(&self) -> Register {
        Register {
            id: self.name.clone(),
            title: "Jira".into(),
            prefix: Some("j".into()),
            dynamic: true,
            // A request per pause in typing, not per key.
            debounce_ms: 250,
            min_query_length: 2,
            items: Vec::new(),
        }
    }

    fn refresh(&mut self) -> Option<Result<Vec<Item>, String>> {
        let jql = build_my_open_issues_jql(&self.projects);
        Some(
            self.client
                .search_issues(&jql, MAX_RESULTS)
                .map(|issues| issues.iter().map(|i| self.item(i)).collect()),
        )
    }

    fn refresh_every(&self) -> Duration {
        Duration::from_secs(5 * 60)
    }

    fn query(&mut self, query: &Query) -> Result<Vec<Item>, String> {
        let text = query.text.trim();
        let key = issue_key(text);
        let jql = match (&key, query.scoped) {
            (Some(key), _) => build_keys_jql(std::slice::from_ref(key)),
            (None, true) if !text.is_empty() => format!(
                "text ~ {}{} ORDER BY updated DESC",
                jql_string(text),
                self.in_projects()
            ),
            // Unscoped and not a key: Jira isn't asked.
            _ => return Ok(Vec::new()),
        };
        let issues = self.client.search_issues(&jql, MAX_RESULTS)?;
        Ok(issues.iter().map(|i| self.item(i)).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flint_protocol::provider::Context;
    use flint_provider_kit::testing::FakeHttp;

    fn search_reply() -> String {
        r#"{"issues": [
            {"key": "OPS-12", "fields": {"summary": "Telemetry export drops the last frame",
             "status": {"name": "In Progress"}, "assignee": {"displayName": "Thomas"},
             "issuetype": {"name": "Bug"}, "updated": "2026-10-01T10:00:00.000+0000"}},
            {"key": "OPS-7", "fields": {"summary": "Rotate the keys", "status": {"name": "Open"},
             "assignee": null, "updated": "2026-09-01T10:00:00.000+0000"}}
        ]}"#
        .into()
    }

    fn jira(server: &FakeHttp, extra: &[&str]) -> Jira {
        let mut args = vec!["--url".to_string(), format!("{}/", server.url)];
        args.extend(extra.iter().map(|s| s.to_string()));
        Jira::from_args(&args, "t0ken".into()).unwrap()
    }

    fn query(text: &str, scoped: bool) -> Query {
        Query {
            query_id: 1,
            text: text.into(),
            scoped,
            context: Context::default(),
        }
    }

    #[test]
    fn open_issues_become_items_that_open_in_the_browser() {
        let server = FakeHttp::start(vec![("/rest/api/2/search", 200, search_reply())]);
        let mut jira = jira(&server, &["--project", "ops"]);
        let items = jira.refresh().unwrap().unwrap();
        assert_eq!(items.len(), 2);
        let first = &items[0];
        assert!(first.check().is_ok());
        assert_eq!(first.id, "jira:OPS-12");
        assert_eq!(first.title, "OPS-12 Telemetry export drops the last frame");
        assert_eq!(
            first.subtitle.as_deref(),
            Some("In Progress · Thomas · Bug")
        );
        assert_eq!(
            first.actions[0].kind,
            ActionKind::Open {
                target: format!("{}/browse/OPS-12", server.url)
            }
        );
        assert_eq!(items[1].subtitle.as_deref(), Some("Open"));
        let asked = server.requests();
        assert!(
            asked[0].contains(
                "assignee = currentUser() AND statusCategory != Done AND project IN (OPS)"
            ),
            "{asked:?}"
        );
        let (_, headers) = &server.seen.lock().unwrap()[0];
        assert!(
            headers.iter().any(|h| h == "Authorization: Bearer t0ken"),
            "{headers:?}"
        );
    }

    #[test]
    fn only_a_key_reaches_jira_unscoped_and_a_scoped_query_searches() {
        let server = FakeHttp::start(vec![("/rest/api/2/search", 200, search_reply())]);
        let mut jira = jira(&server, &[]);
        assert!(jira.query(&query("firefox", false)).unwrap().is_empty());
        assert!(server.requests().is_empty(), "nothing was asked");

        jira.query(&query("ops-12", false)).unwrap();
        jira.query(&query(r#"say "hi""#, true)).unwrap();
        let asked = server.requests();
        assert!(asked[0].contains(r#"key IN ("OPS-12")"#), "{asked:?}");
        assert!(
            asked[1].contains(r#"text ~ "say \"hi\"" ORDER BY updated DESC"#),
            "{asked:?}"
        );
    }

    #[test]
    fn a_refusal_is_one_line() {
        let server = FakeHttp::start(vec![("/rest/api/2/search", 401, "{}".into())]);
        let mut jira = jira(&server, &[]);
        let err = jira.refresh().unwrap().unwrap_err();
        assert!(err.starts_with("HTTP 401"), "{err}");
        assert!(!err.contains('\n'));
    }

    #[test]
    fn keys_urls_and_strings() {
        assert_eq!(issue_key(" abc-123 ").as_deref(), Some("ABC-123"));
        assert_eq!(issue_key("A2_B-9").as_deref(), Some("A2_B-9"));
        for not_a_key in ["a-1", "ABC-", "ABC-12x", "123-4", "fire fox", "ABC"] {
            assert_eq!(issue_key(not_a_key), None, "{not_a_key}");
        }
        assert_eq!(jql_string(r#"a "b" \c"#), r#""a \"b\" \\c""#);
        assert!(matches!(Jira::from_args(&[], "t".into()), Err(e) if e.contains("--url")));
        let ftp = ["--url".to_string(), "ftp://x".to_string()];
        assert!(Jira::from_args(&ftp, "t".into()).is_err());
    }
}
