//! `flint-gitlab`: your GitLab merge requests in Flint, as a provider
//! process.
//!
//! ```toml
//! [[providers]]
//! name = "gitlab"
//! command = ["flint-gitlab", "--url", "https://gitlab.example.com",
//!            "--project", "ops/telemetry", "--project", "ops/ground"]
//! credential = "gitlab:https://gitlab.example.com"
//! ```
//!
//! For each `--project`, the open merge requests you opened and those
//! assigned to you, refreshed every three minutes. They are static items:
//! Flint matches them itself (title, number, branch, author, project) as
//! you type, with frecency, and no keystroke ever reaches GitLab. `mr `
//! scopes to them. `Enter` opens one in the browser; the action menu
//! copies its link or its branch.
//!
//! The API calls (`PRIVATE-TOKEN`, `/api/v4`, the OS certificate store for
//! TLS) and the merge request model are Fenix's `fenix-gitlab` and
//! `fenix-forge`, one project per client as Fenix uses them. The token
//! comes from Flint (`credential`), as for `flint-jira`.
//!
//! Listing across every project at once (`/merge_requests` without a
//! project) would need no `--project`, but `fenix-gitlab` is per project
//! by design, and naming the projects you care about keeps the list
//! yours rather than the instance's.

use std::time::Duration;

use fenix_forge::{Forge, MergeRequest, MrFilter, PipelineStatus};
use fenix_gitlab::GitLab as Client;
use flint_protocol::item::{Action, ActionKind};
use flint_protocol::provider::Register;
use flint_protocol::Item;
use flint_provider_kit::Provider;

/// The provider, configured from its command line.
pub struct GitLab {
    /// Its id (`--name`, default `gitlab`).
    name: String,
    /// One client per `--project`, with the project's path.
    projects: Vec<(String, Client)>,
}

impl GitLab {
    /// From the arguments after the program name and the token.
    pub fn from_args(args: &[String], token: String) -> Result<GitLab, String> {
        let base = flint_provider_kit::flag_values(args, "--url")
            .pop()
            .map(|u| u.trim_end_matches('/').to_owned())
            .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
            .ok_or("needs `--url https://gitlab.example.com`")?;
        let projects: Vec<String> = flint_provider_kit::flag_values(args, "--project")
            .into_iter()
            .map(|p| p.trim().trim_matches('/').to_owned())
            .filter(|p| !p.is_empty())
            .collect();
        if projects.is_empty() {
            return Err("needs at least one `--project group/name`".into());
        }
        let name = flint_provider_kit::flag_values(args, "--name")
            .pop()
            .unwrap_or_else(|| "gitlab".into());
        Ok(GitLab {
            name,
            projects: projects
                .into_iter()
                .map(|p| (p.clone(), Client::new(base.clone(), token.clone(), p)))
                .collect(),
        })
    }

    fn item(&self, project: &str, mr: &MergeRequest) -> Item {
        let draft = if mr.draft { "Draft: " } else { "" };
        let mut details = vec![project.to_owned(), mr.author.clone()];
        details.push(format!("{} → {}", mr.source_branch, mr.target_branch));
        match &mr.pipeline {
            Some(p) if p.is_bad() => details.push(format!("pipeline {}", p.label())),
            Some(PipelineStatus::Running) => details.push("pipeline running".into()),
            _ => {}
        }
        if mr.has_conflicts {
            details.push("conflicts".into());
        }
        let action = |id: &str, title: &str, key: &str, kind: ActionKind| Action {
            id: id.into(),
            title: title.into(),
            key: Some(key.into()),
            kind,
        };
        Item {
            id: format!("{}:{project}!{}", self.name, mr.number),
            title: format!("!{} {draft}{}", mr.number, mr.title)
                .chars()
                .take(300)
                .collect(),
            subtitle: Some(details.join(" · ")),
            icon: Some("merge-request".into()),
            keywords: vec![
                project.to_owned(),
                mr.source_branch.clone(),
                mr.author.clone(),
            ],
            actions: vec![
                action(
                    "open",
                    "Open in browser",
                    "o",
                    ActionKind::Open {
                        target: mr.web_url.clone(),
                    },
                ),
                action(
                    "copy-link",
                    "Copy link",
                    "l",
                    ActionKind::Copy {
                        text: mr.web_url.clone(),
                    },
                ),
                action(
                    "copy-branch",
                    "Copy branch",
                    "b",
                    ActionKind::Copy {
                        text: mr.source_branch.clone(),
                    },
                ),
            ],
        }
    }
}

impl Provider for GitLab {
    fn register(&self) -> Register {
        Register {
            id: self.name.clone(),
            title: "GitLab".into(),
            prefix: Some("mr".into()),
            dynamic: false,
            debounce_ms: 0,
            min_query_length: 0,
            items: Vec::new(),
        }
    }

    /// Yours and assigned to you, per project, without repeats. A project
    /// that fails doesn't hide the others; only when all fail is it an
    /// error, the first one's.
    fn refresh(&mut self) -> Option<Result<Vec<Item>, String>> {
        let mut items: Vec<Item> = Vec::new();
        let mut first_error = None;
        let mut any_ok = false;
        for (project, client) in &self.projects {
            for filter in [MrFilter::ForMe, MrFilter::Mine] {
                match client.list_merge_requests(filter) {
                    Ok(found) => {
                        any_ok = true;
                        for mr in &found {
                            let item = self.item(project, mr);
                            if !items.iter().any(|i| i.id == item.id) {
                                items.push(item);
                            }
                        }
                    }
                    Err(why) => {
                        first_error.get_or_insert(format!("{project}: {why}"));
                    }
                }
            }
        }
        Some(match (any_ok, first_error) {
            (false, Some(why)) => Err(why),
            _ => Ok(items),
        })
    }

    fn refresh_every(&self) -> Duration {
        Duration::from_secs(3 * 60)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flint_provider_kit::testing::FakeHttp;

    fn mrs() -> String {
        r#"[
            {"iid": 42, "title": "Export the last frame", "state": "opened", "draft": false,
             "source_branch": "fix/last-frame", "target_branch": "main",
             "author": {"name": "Thomas", "username": "thomas"},
             "web_url": "https://gitlab.example.com/ops/telemetry/-/merge_requests/42",
             "has_conflicts": true, "head_pipeline": {"status": "failed"}},
            {"iid": 7, "title": "Rotate keys", "state": "opened", "draft": true,
             "source_branch": "keys", "target_branch": "main",
             "author": {"name": "", "username": "sam"},
             "web_url": "https://gitlab.example.com/ops/telemetry/-/merge_requests/7"}
        ]"#
        .into()
    }

    fn args(server: &FakeHttp, projects: &[&str]) -> Vec<String> {
        let mut args = vec!["--url".to_string(), server.url.clone()];
        for p in projects {
            args.extend(["--project".to_string(), p.to_string()]);
        }
        args
    }

    #[test]
    fn your_merge_requests_become_items_once_each() {
        let server = FakeHttp::start(vec![("/api/v4/projects/ops%2Ftelemetry/", 200, mrs())]);
        let mut gitlab =
            GitLab::from_args(&args(&server, &["ops/telemetry"]), "t0ken".into()).unwrap();
        let items = gitlab.refresh().unwrap().unwrap();
        assert_eq!(items.len(), 2, "mine and for-me overlap; each once");
        let first = &items[0];
        assert!(first.check().is_ok());
        assert_eq!(first.id, "gitlab:ops/telemetry!42");
        assert_eq!(first.title, "!42 Export the last frame");
        assert_eq!(
            first.subtitle.as_deref(),
            Some("ops/telemetry · Thomas · fix/last-frame → main · pipeline failed · conflicts")
        );
        assert_eq!(items[1].title, "!7 Draft: Rotate keys");
        assert_eq!(items[1].keywords, ["ops/telemetry", "keys", "sam"]);
        let asked = server.requests();
        assert!(
            asked.iter().any(|r| r.contains("scope=created_by_me")),
            "{asked:?}"
        );
        assert!(
            asked.iter().any(|r| r.contains("scope=assigned_to_me")),
            "{asked:?}"
        );
        let (_, headers) = &server.seen.lock().unwrap()[0];
        assert!(
            headers.iter().any(|h| h == "PRIVATE-TOKEN: t0ken"),
            "{headers:?}"
        );
    }

    #[test]
    fn one_failing_project_doesnt_hide_the_others() {
        let server = FakeHttp::start(vec![("/api/v4/projects/ops%2Ftelemetry/", 200, mrs())]);
        let mut gitlab =
            GitLab::from_args(&args(&server, &["ops/telemetry", "ops/gone"]), "t".into()).unwrap();
        assert_eq!(gitlab.refresh().unwrap().unwrap().len(), 2);
        let mut gone = GitLab::from_args(&args(&server, &["ops/gone"]), "t".into()).unwrap();
        let err = gone.refresh().unwrap().unwrap_err();
        assert!(err.starts_with("ops/gone: HTTP 404"), "{err}");
    }

    #[test]
    fn it_needs_a_url_and_a_project() {
        let none: Vec<String> = Vec::new();
        assert!(matches!(GitLab::from_args(&none, "t".into()), Err(e) if e.contains("--url")));
        let no_project = ["--url".to_string(), "https://g".to_string()];
        assert!(
            matches!(GitLab::from_args(&no_project, "t".into()), Err(e) if e.contains("--project"))
        );
    }
}
