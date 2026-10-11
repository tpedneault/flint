//! What the window shows, as plain data, and how each key changes it:
//! the query and its scope chip, the mode, the result rows, the selection
//! and scroll position, and the footer line.
//!
//! The grammar is Fenix's (DESIGN.md, "Modes"). In Insert mode typing
//! edits the query, `Ctrl-n`/`Ctrl-p` or the arrows move, `Ctrl-w`
//! deletes a word, `Ctrl-u` clears, `Enter` runs the selected row's first
//! action and `Esc` goes to Normal mode. In Normal mode `j`/`k` move,
//! `gg`/`G` jump, `i`/`a`/`/` go back to Insert, `y` copies, and `Esc` or
//! `q` hide. `behaviour.escape_hides` makes one `Esc` hide from either
//! mode. The action menu (`Tab`, `.`) and the leader (`SPC`) arrive in M4;
//! until then those keys say so in the footer.
//!
//! This module decides nothing about results: it reports
//! [`Outcome::QueryChanged`], [`Outcome::Activate`] and [`Outcome::Copy`]
//! and the daemon searches and acts.

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Row {
    /// Shown in the body colour.
    pub title: String,
    /// Shown dimmed after the title.
    pub subtitle: String,
    /// The provider, shown at the right.
    pub badge: String,
    /// Character positions in `title` that matched the query.
    pub highlights: Vec<u32>,
}

/// The footer's one line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FooterLine {
    /// What it says.
    pub text: String,
    /// Whether it reports a failure (drawn in the error colour).
    pub error: bool,
}

impl FooterLine {
    /// A plain message.
    pub fn info(text: impl Into<String>) -> Self {
        FooterLine {
            text: text.into(),
            error: false,
        }
    }

    /// A failure.
    pub fn error(text: impl Into<String>) -> Self {
        FooterLine {
            text: text.into(),
            error: true,
        }
    }
}

/// The scope chip: the query is limited to one provider or quicklink.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// The keyword that set it (`a`, `j`).
    pub keyword: String,
    /// What the chip says (`Apps`, `Jira issue`).
    pub label: String,
}

/// Which keys do what.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Typing edits the query.
    #[default]
    Insert,
    /// Letters are commands.
    Normal,
}

/// A key press, already separated from the platform's key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// A character, as typed (shift applied, AltGr resolved).
    Char(char),
    /// `Ctrl` and a letter, lowercased.
    Ctrl(char),
    /// `Esc`.
    Escape,
    /// `Enter`.
    Enter,
    /// `Backspace`.
    Backspace,
    /// `Ctrl-Backspace`.
    CtrlBackspace,
    /// The up arrow.
    Up,
    /// The down arrow.
    Down,
    /// `Tab`.
    Tab,
}

/// What a key press means for the window and the daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Nothing changed.
    Nothing,
    /// Something visible changed; draw again.
    Redraw,
    /// The query or its scope changed: search again.
    QueryChanged,
    /// Run row `n`'s first action.
    Activate(usize),
    /// Copy row `n`'s primary text.
    Copy(usize),
    /// Hide the window.
    Hide,
}

/// The window's state.
#[derive(Debug, Clone, Default)]
pub struct Launcher {
    query: String,
    scope: Option<Chip>,
    mode: Mode,
    rows: Vec<Row>,
    selected: usize,
    /// The first visible row.
    top: usize,
    /// How many rows fit.
    visible: usize,
    /// `g` was pressed in Normal mode; a second `g` jumps to the top.
    pending_g: bool,
    escape_hides: bool,
    /// Set by the daemon and kept until it changes it.
    notice: Option<FooterLine>,
    /// Set by an action or key and cleared by the next edit or show.
    message: Option<FooterLine>,
}

impl Launcher {
    /// The query line's text, without the scope.
    pub fn query(&self) -> &str {
        &self.query
    }

    /// The scope chip, if any.
    pub fn scope(&self) -> Option<&Chip> {
        self.scope.as_ref()
    }

    /// The current mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The rows.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// The selected row's index; meaningless when there are no rows.
    pub fn selected(&self) -> usize {
        self.selected
    }

    /// The first visible row's index.
    pub fn top(&self) -> usize {
        self.top
    }

    /// What the footer says now.
    pub fn footer(&self) -> FooterLine {
        self.message
            .clone()
            .or_else(|| self.notice.clone())
            .unwrap_or_else(|| FooterLine::info(self.hint()))
    }

    fn hint(&self) -> &'static str {
        match self.mode {
            Mode::Insert if self.rows.is_empty() => "type to search · Esc for Normal mode",
            Mode::Insert => "Enter runs · Ctrl-n/Ctrl-p move · Esc for Normal mode",
            Mode::Normal => "j/k move · Enter runs · y copies · i types · q hides",
        }
    }

    /// Sets how many rows fit, keeping the selection visible.
    pub fn set_visible_rows(&mut self, visible: usize) {
        self.visible = visible.max(1);
        self.scroll_to_selection();
    }

    /// Whether `Esc` hides from either mode.
    pub fn set_escape_hides(&mut self, escape_hides: bool) {
        self.escape_hides = escape_hides;
    }

    /// New results after a new query: the first row selected.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.rows = rows;
        self.selected = 0;
        self.top = 0;
    }

    /// Results changed without a new query: the selection stays where it
    /// is (the daemon has kept the rows above it in place).
    pub fn merge_rows(&mut self, rows: Vec<Row>) {
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.scroll_to_selection();
    }

    /// Sets or clears the scope chip.
    pub fn set_scope(&mut self, scope: Option<Chip>) {
        self.scope = scope;
    }

    /// Replaces the query text.
    pub fn set_query(&mut self, query: String) {
        self.query = query;
    }

    /// Sets or clears the daemon's standing notice.
    pub fn set_notice(&mut self, notice: Option<FooterLine>) {
        self.notice = notice;
    }

    /// Shows a message until the next edit or show.
    pub fn set_message(&mut self, message: FooterLine) {
        self.message = Some(message);
    }

    /// Starts afresh for a show: Insert mode, the given query, no scope, no
    /// message. Rows stay until the daemon's new results replace them.
    pub fn reset(&mut self, query: Option<String>) {
        self.query = query.unwrap_or_default();
        self.scope = None;
        self.mode = Mode::Insert;
        self.selected = 0;
        self.top = 0;
        self.pending_g = false;
        self.message = None;
    }

    /// Applies one key press.
    pub fn press(&mut self, press: Press) -> Outcome {
        let pending_g = std::mem::take(&mut self.pending_g);
        match self.mode {
            Mode::Insert => self.insert(press),
            Mode::Normal => self.normal(press, pending_g),
        }
    }

    fn insert(&mut self, press: Press) -> Outcome {
        match press {
            Press::Char(c) if !c.is_control() => {
                self.query.push(c);
                self.edited()
            }
            Press::Char(_) => Outcome::Nothing,
            Press::Backspace => {
                if self.query.pop().is_some() {
                    self.edited()
                } else if self.scope.take().is_some() {
                    // Backspace on an empty query removes the scope.
                    self.edited()
                } else {
                    Outcome::Nothing
                }
            }
            Press::CtrlBackspace | Press::Ctrl('w') => {
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
            Press::Ctrl('u') => {
                if self.query.is_empty() {
                    return Outcome::Nothing;
                }
                self.query.clear();
                self.edited()
            }
            Press::Down | Press::Ctrl('n') => self.select(self.selected + 1),
            Press::Up | Press::Ctrl('p') => self.select(self.selected.saturating_sub(1)),
            Press::Enter => self.activate(),
            Press::Tab => self.later("the action menu arrives in M4"),
            Press::Escape if self.escape_hides => Outcome::Hide,
            Press::Escape => {
                self.mode = Mode::Normal;
                Outcome::Redraw
            }
            Press::Ctrl(_) => Outcome::Nothing,
        }
    }

    fn normal(&mut self, press: Press, pending_g: bool) -> Outcome {
        match press {
            Press::Char('j') | Press::Down | Press::Ctrl('n') => self.select(self.selected + 1),
            Press::Char('k') | Press::Up | Press::Ctrl('p') => {
                self.select(self.selected.saturating_sub(1))
            }
            Press::Char('g') if pending_g => self.select(0),
            Press::Char('g') => {
                self.pending_g = true;
                Outcome::Nothing
            }
            Press::Char('G') => self.select(self.rows.len().saturating_sub(1)),
            Press::Char('i' | 'a' | '/') => {
                self.mode = Mode::Insert;
                Outcome::Redraw
            }
            Press::Enter => self.activate(),
            Press::Char('y') if !self.rows.is_empty() => Outcome::Copy(self.selected),
            Press::Char('y') => Outcome::Nothing,
            Press::Escape | Press::Char('q') => Outcome::Hide,
            Press::Tab | Press::Char('.') => self.later("the action menu arrives in M4"),
            Press::Char(' ') => self.later("the leader arrives in M4"),
            _ => Outcome::Nothing,
        }
    }

    fn later(&mut self, what: &str) -> Outcome {
        self.message = Some(FooterLine::info(what));
        Outcome::Redraw
    }

    fn activate(&self) -> Outcome {
        if self.rows.is_empty() {
            Outcome::Nothing
        } else {
            Outcome::Activate(self.selected)
        }
    }

    fn edited(&mut self) -> Outcome {
        self.message = None;
        Outcome::QueryChanged
    }

    fn select(&mut self, index: usize) -> Outcome {
        let index = index.min(self.rows.len().saturating_sub(1));
        if index == self.selected {
            return Outcome::Nothing;
        }
        self.selected = index;
        self.scroll_to_selection();
        Outcome::Redraw
    }

    fn scroll_to_selection(&mut self) {
        let visible = self.visible.max(1);
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected >= self.top + visible {
            self.top = self.selected + 1 - visible;
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
                ..Row::default()
            })
            .collect()
    }

    fn launcher(n: usize) -> Launcher {
        let mut l = Launcher::default();
        l.set_visible_rows(3);
        l.set_rows(rows(n));
        l
    }

    fn typed(l: &mut Launcher, text: &str) {
        for c in text.chars() {
            l.press(Press::Char(c));
        }
    }

    #[test]
    fn typing_edits_and_reports_the_change() {
        let mut l = launcher(0);
        assert_eq!(l.press(Press::Char('h')), Outcome::QueryChanged);
        typed(&mut l, "éllo");
        assert_eq!(l.query(), "héllo");
        assert_eq!(l.press(Press::Backspace), Outcome::QueryChanged);
        assert_eq!(l.query(), "héll");
        assert_eq!(l.press(Press::Ctrl('u')), Outcome::QueryChanged);
        assert_eq!(l.press(Press::Ctrl('u')), Outcome::Nothing);
        assert_eq!(l.press(Press::Char('\u{8}')), Outcome::Nothing);
    }

    #[test]
    fn ctrl_w_deletes_a_word() {
        let mut l = launcher(0);
        typed(&mut l, "j ABC-123 ");
        l.press(Press::Ctrl('w'));
        assert_eq!(l.query(), "j ");
        l.press(Press::CtrlBackspace);
        assert_eq!(l.query(), "");
        assert_eq!(l.press(Press::Ctrl('w')), Outcome::Nothing);
    }

    #[test]
    fn backspace_on_an_empty_query_removes_the_scope() {
        let mut l = launcher(0);
        l.set_scope(Some(Chip {
            keyword: "a".into(),
            label: "Apps".into(),
        }));
        typed(&mut l, "x");
        l.press(Press::Backspace);
        assert!(
            l.scope().is_some(),
            "the first backspace deletes the character"
        );
        assert_eq!(l.press(Press::Backspace), Outcome::QueryChanged);
        assert!(l.scope().is_none());
        assert_eq!(l.press(Press::Backspace), Outcome::Nothing);
    }

    #[test]
    fn esc_goes_to_normal_then_hides_unless_escape_hides() {
        let mut l = launcher(2);
        assert_eq!(l.press(Press::Escape), Outcome::Redraw);
        assert_eq!(l.mode(), Mode::Normal);
        assert_eq!(l.press(Press::Escape), Outcome::Hide);
        let mut quick = launcher(2);
        quick.set_escape_hides(true);
        assert_eq!(quick.press(Press::Escape), Outcome::Hide);
    }

    #[test]
    fn normal_mode_moves_jumps_copies_and_returns_to_insert() {
        let mut l = launcher(10);
        l.press(Press::Escape);
        l.press(Press::Char('j'));
        l.press(Press::Char('j'));
        assert_eq!(l.selected(), 2);
        assert_eq!(l.press(Press::Char('G')), Outcome::Redraw);
        assert_eq!(l.selected(), 9);
        assert_eq!(l.top(), 7, "the selection stays visible");
        assert_eq!(l.press(Press::Char('g')), Outcome::Nothing);
        assert_eq!(l.press(Press::Char('g')), Outcome::Redraw);
        assert_eq!((l.selected(), l.top()), (0, 0));
        // `g` then something else doesn't jump.
        l.press(Press::Char('j'));
        l.press(Press::Char('g'));
        l.press(Press::Char('k'));
        l.press(Press::Char('g'));
        assert_eq!(l.selected(), 0);
        assert_eq!(l.press(Press::Char('y')), Outcome::Copy(0));
        assert_eq!(
            l.press(Press::Char('x')),
            Outcome::Nothing,
            "letters don't type in Normal mode"
        );
        assert_eq!(l.query(), "");
        assert_eq!(l.press(Press::Char('a')), Outcome::Redraw);
        assert_eq!(l.mode(), Mode::Insert);
        assert_eq!(
            l.press(Press::Char('j')),
            Outcome::QueryChanged,
            "and now they do"
        );
    }

    #[test]
    fn enter_activates_the_selected_row_in_either_mode() {
        let mut l = launcher(3);
        l.press(Press::Ctrl('n'));
        assert_eq!(l.press(Press::Enter), Outcome::Activate(1));
        l.press(Press::Escape);
        assert_eq!(l.press(Press::Enter), Outcome::Activate(1));
        assert_eq!(launcher(0).press(Press::Enter), Outcome::Nothing);
    }

    #[test]
    fn keys_for_later_milestones_say_so() {
        let mut l = launcher(1);
        l.press(Press::Tab);
        assert!(l.footer().text.contains("M4"));
        l.press(Press::Escape);
        l.press(Press::Char(' '));
        assert!(l.footer().text.contains("leader"));
    }

    #[test]
    fn merged_rows_keep_the_selection_new_rows_reset_it() {
        let mut l = launcher(5);
        l.press(Press::Down);
        l.press(Press::Down);
        l.merge_rows(rows(8));
        assert_eq!(l.selected(), 2);
        l.merge_rows(rows(1));
        assert_eq!(l.selected(), 0);
        l.press(Press::Down);
        l.set_rows(rows(4));
        assert_eq!(l.selected(), 0);
    }

    #[test]
    fn the_footer_prefers_a_message_then_the_notice_then_the_hint() {
        let mut l = launcher(1);
        assert!(l.footer().text.starts_with("Enter runs"));
        l.set_notice(Some(FooterLine::error("hotkey failed")));
        assert!(l.footer().error);
        l.set_message(FooterLine::info("copied"));
        assert_eq!(l.footer().text, "copied");
        l.press(Press::Char('x'));
        assert_eq!(l.footer().text, "hotkey failed");
        l.press(Press::Escape);
        l.set_notice(None);
        assert!(l.footer().text.starts_with("j/k move"));
    }

    #[test]
    fn reset_starts_afresh_in_insert_mode() {
        let mut l = launcher(4);
        l.set_scope(Some(Chip {
            keyword: "a".into(),
            label: "Apps".into(),
        }));
        l.press(Press::Escape);
        l.press(Press::Char('j'));
        l.reset(Some("j ".into()));
        assert_eq!(l.query(), "j ");
        assert_eq!(l.mode(), Mode::Insert);
        assert!(l.scope().is_none());
        assert_eq!(l.selected(), 0);
    }
}
