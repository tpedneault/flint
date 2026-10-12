//! The Quicklinks provider: one item per `[[quicklinks]]` entry, and the
//! filled-in link when the query is scoped to one.
//!
//! Unscoped, a link is an item like any other, found by its name or
//! keyword. Choosing one whose URL takes a query scopes the query line to
//! it (a `query` action, `j ` for keyword `j`), so the next thing typed
//! fills it. Scoped, the provider offers exactly one item: the link with
//! `{query}` replaced by the text, percent-encoded, so `j ABC-123` and
//! `Enter` opens the issue. The scoped and unscoped items share an id, so
//! frecency counts them together.

use flint_config::Quicklink;
use flint_protocol::item::{Action, ActionKind};
use flint_protocol::Item;

/// The provider's id.
pub const ID: &str = "quicklink";
/// Its badge.
pub const TITLE: &str = "Link";
/// What the URL's query placeholder looks like.
pub const PLACEHOLDER: &str = "{query}";

/// The item id of `link`.
pub fn id(link: &Quicklink) -> String {
    format!("{ID}:{}", link.keyword)
}

/// The unscoped item for `link`.
pub fn item(link: &Quicklink) -> Item {
    let takes_query = link.url.contains(PLACEHOLDER);
    let primary = if takes_query {
        action(
            "scope",
            "Type the query",
            ActionKind::Query {
                text: format!("{} ", link.keyword),
            },
        )
    } else {
        action(
            "open",
            "Open",
            ActionKind::Open {
                target: link.url.clone(),
            },
        )
    };
    Item {
        id: id(link),
        title: link.name.clone(),
        subtitle: Some(format!("{} · {}", link.keyword, link.url)),
        icon: Some("link".into()),
        keywords: vec![link.keyword.clone()],
        actions: vec![
            primary,
            action(
                "copy",
                "Copy URL",
                ActionKind::Copy {
                    text: link.url.clone(),
                },
            ),
        ],
    }
}

/// The item for `link` scoped with `text` typed after its keyword.
pub fn filled(link: &Quicklink, text: &str) -> Item {
    let url = fill(&link.url, text);
    let title = if text.trim().is_empty() || !link.url.contains(PLACEHOLDER) {
        link.name.clone()
    } else {
        format!("{}: {}", link.name, text.trim())
    };
    Item {
        id: id(link),
        title,
        subtitle: Some(url.clone()),
        icon: Some("link".into()),
        keywords: vec![],
        actions: vec![
            action(
                "open",
                "Open",
                ActionKind::Open {
                    target: url.clone(),
                },
            ),
            action("copy", "Copy URL", ActionKind::Copy { text: url }),
        ],
    }
}

/// `url` with every `{query}` replaced by `text`, trimmed and
/// percent-encoded.
pub fn fill(url: &str, text: &str) -> String {
    url.replace(PLACEHOLDER, &percent_encode(text.trim()))
}

/// RFC 3986 percent-encoding: unreserved characters stay, every other
/// byte of the UTF-8 becomes `%XX`. A space is `%20`, never `+`, which
/// only means a space in form bodies.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn action(id: &str, title: &str, kind: ActionKind) -> Action {
    Action {
        id: id.into(),
        title: title.into(),
        key: flint_protocol::item::conventional_key(id),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jira() -> Quicklink {
        Quicklink {
            keyword: "j".into(),
            name: "Jira issue".into(),
            url: "https://jira.example.com/browse/{query}".into(),
        }
    }

    #[test]
    fn encoding_keeps_unreserved_and_escapes_the_rest() {
        assert_eq!(percent_encode("ABC-123"), "ABC-123");
        assert_eq!(percent_encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(percent_encode("~_."), "~_.");
    }

    #[test]
    fn a_link_taking_a_query_scopes_to_itself_when_chosen() {
        let item = item(&jira());
        assert_eq!(item.id, "quicklink:j");
        assert_eq!(
            item.actions[0].kind,
            ActionKind::Query { text: "j ".into() }
        );
        assert!(item.check().is_ok());
    }

    #[test]
    fn scoped_the_link_is_filled() {
        let item = filled(&jira(), " ABC 123 ");
        assert_eq!(
            item.id, "quicklink:j",
            "shares frecency with the unscoped item"
        );
        assert_eq!(item.title, "Jira issue: ABC 123");
        assert_eq!(
            item.actions[0].kind,
            ActionKind::Open {
                target: "https://jira.example.com/browse/ABC%20123".into()
            }
        );
    }

    #[test]
    fn a_link_without_a_placeholder_just_opens() {
        let outlook = Quicklink {
            keyword: "w".into(),
            name: "Outlook".into(),
            url: "https://outlook.office.com".into(),
        };
        assert!(matches!(
            item(&outlook).actions[0].kind,
            ActionKind::Open { .. }
        ));
        assert_eq!(filled(&outlook, "ignored").title, "Outlook");
    }
}
