//! Scope prefixes: `j ABC-123` scopes the query to the provider (or
//! quicklink) whose keyword is `j`.
//!
//! A keyword counts only once it is followed by a space, so typing `a`
//! still searches everything for "a", and `a ` turns into the Apps chip.
//! Keywords are matched exactly and case-sensitively: they are short and
//! chosen by the user, and `J` meaning something different from `j` is
//! theirs to decide.

/// If `text` starts with one of `keywords` and a space, the keyword and
/// the rest of the text (one separating space removed).
pub fn split_scope<'a, 'k>(
    text: &'a str,
    keywords: impl IntoIterator<Item = &'k str>,
) -> Option<(&'k str, &'a str)> {
    let (first, rest) = text.split_once(' ')?;
    keywords
        .into_iter()
        .find(|k| !k.is_empty() && *k == first)
        .map(|k| (k, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYWORDS: [&str; 3] = ["a", "j", "flint"];

    #[test]
    fn a_keyword_needs_its_space() {
        assert_eq!(split_scope("a", KEYWORDS), None);
        assert_eq!(split_scope("a ", KEYWORDS), Some(("a", "")));
        assert_eq!(split_scope("j ABC-123", KEYWORDS), Some(("j", "ABC-123")));
        assert_eq!(
            split_scope("flint  quit", KEYWORDS),
            Some(("flint", " quit"))
        );
    }

    #[test]
    fn only_whole_known_keywords_scope() {
        assert_eq!(split_scope("ab c", KEYWORDS), None);
        assert_eq!(split_scope("A x", KEYWORDS), None);
        assert_eq!(split_scope(" a x", KEYWORDS), None);
        assert_eq!(split_scope("x y", KEYWORDS), None);
    }
}
