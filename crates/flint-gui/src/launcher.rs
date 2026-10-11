//! What the window shows, as plain data: the query, the rows, which row
//! is selected, and the footer line; and how each input changes it.
//!
//! M1's list is static: the daemon supplies the rows (facts about itself)
//! and typing edits the query without filtering them. Matching and ranking
//! arrive with `flint-index` in M2, along with Insert/Normal modes; until
//! then there is one mode, close to Insert's keys, and `Esc` hides.

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Shown in the body colour.
    pub title: String,
    /// Shown dimmed after the title.
    pub subtitle: String,
}

/// The footer's one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterLine {
    /// What it says.
    pub text: String,
    /// Whether it reports a failure (drawn in the error colour).
    pub error: bool,
}

/// What the user did, independent of the key that did it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// Typed a character.
    Char(char),
    /// Deleted the character before the caret.
    Backspace,
    /// Deleted the word before the caret (`Ctrl-w`).
    DeleteWord,
    /// Cleared the query (`Ctrl-u`).
    Clear,
    /// Moved the selection up.
    Up,
    /// Moved the selection down.
    Down,
    /// Pressed `Enter`.
    Submit,
    /// Pressed `Esc`.
    Escape,
}

/// What an input means for the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Something visible changed; draw again.
    Redraw,
    /// Nothing changed.
    Nothing,
    /// Hide the window.
    Hide,
}

/// The footer when there is nothing else to say.
const HINT: &str = "Esc hides · results and actions arrive in M2";

/// The window's state.
#[derive(Debug, Clone, Default)]
pub struct Launcher {
    query: String,
    rows: Vec<Row>,
    selected: usize,
    /// Set by the daemon and kept until it changes it: the hotkey failed,
    /// settings have errors.
    notice: Option<FooterLine>,
    /// Set by an input and cleared by the next edit or show.
    message: Option<FooterLine>,
}

impl Launcher {
    /// The query line's text.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The rows.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The selected row's index; meaningless when there are no rows.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// What the footer says now.
    pub fn footer(&self) -> FooterLine {
        self.message
            .clone()
            .or_else(|| self.notice.clone())
            .unwrap_or(FooterLine {
                text: HINT.to_owned(),
                error: false,
            })
    }

    /// Replaces the rows, keeping the selection in range.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    /// Sets or clears the daemon's notice.
    pub fn set_notice(&mut self, notice: Option<FooterLine>) {
        self.notice = notice;
    }

    /// Starts afresh for a show: the given query (or none), the first row
    /// selected, no message.
    pub fn reset(&mut self, query: Option<String>) {
        self.query = query.unwrap_or_default();
        self.selected = 0;
        self.message = None;
    }

    /// Applies one input.
    pub fn apply(&mut self, input: Input) -> Outcome {
        match input {
            Input::Char(c) if !c.is_control() => {
                self.query.push(c);
                self.edited()
            }
            Input::Char(_) => Outcome::Nothing,
            Input::Backspace => {
                if self.query.pop().is_some() {
                    self.edited()
                } else {
                    Outcome::Nothing
                }
            }
            Input::DeleteWord => {
                let trimmed = self.query.trim_end().len();
                let start = self.query[..trimmed]
                    .rfind(char::is_whitespace)
                    .map_or(0, |i| i + 1);
                if start == self.query.len() {
                    return Outcome::Nothing;
                }
                self.query.truncate(start);
                self.edited()
            }
            Input::Clear => {
                if self.query.is_empty() {
                    return Outcome::Nothing;
                }
                self.query.clear();
                self.edited()
            }
            Input::Up => self.select(self.selected.saturating_sub(1)),
            Input::Down => self.select((self.selected + 1).min(self.rows.len().saturating_sub(1))),
            Input::Submit => {
                self.message = Some(FooterLine {
                    text: "Nothing runs yet: actions arrive in M2".to_owned(),
                    error: false,
                });
                Outcome::Redraw
            }
            Input::Escape => Outcome::Hide,
        }
    }

    fn edited(&mut self) -> Outcome {
        self.message = None;
        Outcome::Redraw
    }

    fn select(&mut self, index: usize) -> Outcome {
        if index == self.selected {
            Outcome::Nothing
        } else {
            self.selected = index;
            Outcome::Redraw
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(n: usize) -> Vec<Row> {
        (0..n)
            .map(|i| Row {
                title: format!("row {i}"),
                subtitle: String::new(),
            })
            .collect()
    }

    fn typed(text: &str) -> Launcher {
        let mut l = Launcher::default();
        for c in text.chars() {
            l.apply(Input::Char(c));
        }
        l
    }

    #[test]
    fn typing_and_deleting() {
        let mut l = typed("héllo");
        assert_eq!(l.query(), "héllo");
        assert_eq!(l.apply(Input::Backspace), Outcome::Redraw);
        assert_eq!(l.query(), "héll");
        assert_eq!(l.apply(Input::Clear), Outcome::Redraw);
        assert_eq!(l.apply(Input::Clear), Outcome::Nothing);
        assert_eq!(l.apply(Input::Backspace), Outcome::Nothing);
        assert_eq!(l.apply(Input::Char('\u{8}')), Outcome::Nothing);
    }

    #[test]
    fn ctrl_w_deletes_the_last_word_and_its_trailing_space() {
        let mut l = typed("j ABC-123 ");
        l.apply(Input::DeleteWord);
        assert_eq!(l.query(), "j ");
        l.apply(Input::DeleteWord);
        assert_eq!(l.query(), "");
        assert_eq!(l.apply(Input::DeleteWord), Outcome::Nothing);
    }

    #[test]
    fn the_selection_stays_in_range() {
        let mut l = Launcher::default();
        assert_eq!(l.apply(Input::Down), Outcome::Nothing, "no rows");
        l.set_rows(rows(3));
        assert_eq!(l.apply(Input::Up), Outcome::Nothing);
        l.apply(Input::Down);
        l.apply(Input::Down);
        assert_eq!(l.apply(Input::Down), Outcome::Nothing);
        assert_eq!(l.selected(), 2);
        l.set_rows(rows(1));
        assert_eq!(l.selected(), 0);
    }

    #[test]
    fn the_footer_prefers_a_message_then_the_notice_then_the_hint() {
        let mut l = Launcher::default();
        assert!(l.footer().text.starts_with("Esc hides"));
        l.set_notice(Some(FooterLine {
            text: "hotkey failed".into(),
            error: true,
        }));
        assert!(l.footer().error);
        l.apply(Input::Submit);
        assert!(l.footer().text.contains("M2"));
        l.apply(Input::Char('x'));
        assert_eq!(l.footer().text, "hotkey failed");
        l.set_notice(None);
        assert!(!l.footer().error);
    }

    #[test]
    fn reset_starts_afresh_with_a_query() {
        let mut l = typed("old");
        l.set_rows(rows(4));
        l.apply(Input::Down);
        l.reset(Some("j ".into()));
        assert_eq!(l.query(), "j ");
        assert_eq!(l.selected(), 0);
        assert_eq!(l.apply(Input::Escape), Outcome::Hide);
    }
}
