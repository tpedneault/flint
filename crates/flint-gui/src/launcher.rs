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
//! mode.
//!
//! Two overlays take the list's place for a moment. The **action menu**
//! (`Tab` in either mode, `.` in Normal mode) lists the selected row's
//! actions: `j`/`k` or the arrows move, `Enter` or an action's own key
//! runs it, `Esc` or `Tab` closes it; a disabled action says why instead
//! of running. The **leader** (`SPC` in Normal mode) walks the user's
//! `[leader]` map with a which-key list of what each next key does;
//! `Backspace` goes back a key and `Esc` cancels.
//!
//! This module decides nothing about results: it reports
//! [`Outcome::QueryChanged`], [`Outcome::Activate`], [`Outcome::Run`],
//! [`Outcome::Copy`] and [`Outcome::Leader`], and the daemon searches and
//! acts.

use std::borrow::Cow;
use std::sync::Arc;

use crate::leader::{Leader, LeaderStep};

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
    /// What the action menu offers; the first is `Enter`'s.
    pub actions: Vec<MenuAction>,
    /// Drawn dimmed: a disabled action in the menu.
    pub dimmed: bool,
}

/// One entry of the action menu.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuAction {
    /// What it does.
    pub title: String,
    /// Its accelerator in the menu.
    pub key: Option<char>,
    /// Why it can't run, when it can't.
    pub disabled: Option<String>,
}

/// What the list area shows: results, or an overlay in their place.
#[derive(Debug, Clone)]
pub struct View<'a> {
    /// The rows to draw.
    pub rows: Cow<'a, [Row]>,
    /// The highlighted row, if any.
    pub selected: Option<usize>,
    /// The first row drawn.
    pub top: usize,
    /// Whether this is an overlay (menu, leader) rather than results.
    pub overlay: bool,
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
    /// Run action `action` of row `row`, chosen in the action menu.
    Run {
        /// The row.
        row: usize,
        /// The action's index in the row's actions.
        action: usize,
    },
    /// The leader sequence named the daemon's binding `n`.
    Leader(usize),
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
    /// The action menu is open, with this action selected.
    menu: Option<usize>,
    /// The leader map, and the keys pressed after `SPC` while it's open.
    leader: Arc<Leader>,
    leader_path: Option<Vec<char>>,
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

    fn hint(&self) -> String {
        if self.menu.is_some() {
            let title = self
                .rows
                .get(self.selected)
                .map_or("", |r| r.title.as_str());
            return format!("Actions for {title} · Enter or a key runs · Esc closes");
        }
        if let Some(path) = &self.leader_path {
            let keys: String = path.iter().map(|k| format!(" {k}")).collect();
            let group = self
                .leader
                .group(path)
                .map(|g| format!(" ({g})"))
                .unwrap_or_default();
            return format!("SPC{keys}{group} · Backspace goes back · Esc cancels");
        }
        match self.mode {
            Mode::Insert if self.rows.is_empty() => "type to search · Esc for Normal mode",
            Mode::Insert => "Enter runs · Ctrl-n/Ctrl-p move · Esc for Normal mode",
            Mode::Normal => "j/k move · Enter runs · Tab actions · SPC leader · i types · q hides",
        }
        .to_owned()
    }

    /// What the list area shows now.
    pub fn view(&self) -> View<'_> {
        if let Some(chosen) = self.menu {
            let actions = self.rows.get(self.selected).map_or(&[][..], |r| &r.actions);
            let rows = actions
                .iter()
                .map(|a| Row {
                    title: a.title.clone(),
                    subtitle: a.disabled.clone().unwrap_or_default(),
                    badge: a.key.map(String::from).unwrap_or_default(),
                    highlights: Vec::new(),
                    actions: Vec::new(),
                    dimmed: a.disabled.is_some(),
                })
                .collect();
            let top = chosen.saturating_sub(self.visible.max(1) - 1);
            return View {
                rows: Cow::Owned(rows),
                selected: Some(chosen),
                top,
                overlay: true,
            };
        }
        if let Some(path) = &self.leader_path {
            let rows = hint_rows(&self.leader.hints(path), self.visible.max(1));
            return View {
                rows: Cow::Owned(rows),
                selected: None,
                top: 0,
                overlay: true,
            };
        }
        View {
            rows: Cow::Borrowed(&self.rows),
            selected: (!self.rows.is_empty()).then_some(self.selected),
            top: self.top,
            overlay: false,
        }
    }

    /// Goes to Insert mode.
    pub fn insert_mode(&mut self) {
        self.mode = Mode::Insert;
    }

    /// Uses `leader` for `SPC`.
    pub fn set_leader(&mut self, leader: Arc<Leader>) {
        self.leader = leader;
        self.leader_path = None;
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
        self.menu = None;
    }

    /// Results changed without a new query: the selection stays where it
    /// is (the daemon has kept the rows above it in place).
    pub fn merge_rows(&mut self, rows: Vec<Row>) {
        self.rows = rows;
        self.menu = None;
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

    /// Takes the message down: what it said stopped being true.
    pub fn clear_message(&mut self) {
        self.message = None;
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
        self.menu = None;
        self.leader_path = None;
    }

    /// Applies one key press.
    pub fn press(&mut self, press: Press) -> Outcome {
        let pending_g = std::mem::take(&mut self.pending_g);
        if self.menu.is_some() {
            return self.menu_key(press);
        }
        if self.leader_path.is_some() {
            return self.leader_key(press);
        }
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
            Press::Tab => self.open_menu(),
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
            Press::Tab | Press::Char('.') => self.open_menu(),
            Press::Char(' ') => self.open_leader(),
            _ => Outcome::Nothing,
        }
    }

    fn open_menu(&mut self) -> Outcome {
        match self.rows.get(self.selected) {
            Some(row) if !row.actions.is_empty() => {
                self.menu = Some(0);
                self.message = None;
                Outcome::Redraw
            }
            Some(_) => self.say("nothing to do with this one"),
            None => Outcome::Nothing,
        }
    }

    fn open_leader(&mut self) -> Outcome {
        if self.leader.is_empty() {
            return self.say("no leader bindings: add a [leader] table to settings.toml");
        }
        self.leader_path = Some(Vec::new());
        self.message = None;
        Outcome::Redraw
    }

    fn say(&mut self, what: &str) -> Outcome {
        self.message = Some(FooterLine::info(what));
        Outcome::Redraw
    }

    fn menu_key(&mut self, press: Press) -> Outcome {
        let row = self.selected;
        let count = self.rows.get(row).map_or(0, |r| r.actions.len());
        let chosen = self.menu.unwrap_or(0);
        let run = |this: &mut Self, action: usize| -> Outcome {
            let disabled = this.rows[row].actions[action].disabled.clone();
            match disabled {
                Some(why) => {
                    this.message = Some(FooterLine::info(why));
                    Outcome::Redraw
                }
                None => {
                    this.menu = None;
                    Outcome::Run { row, action }
                }
            }
        };
        match press {
            Press::Escape | Press::Tab => {
                self.menu = None;
                Outcome::Redraw
            }
            Press::Down | Press::Ctrl('n') | Press::Char('j') if chosen + 1 < count => {
                self.menu = Some(chosen + 1);
                self.message = None;
                Outcome::Redraw
            }
            Press::Up | Press::Ctrl('p') | Press::Char('k') if chosen > 0 => {
                self.menu = Some(chosen - 1);
                self.message = None;
                Outcome::Redraw
            }
            Press::Enter if chosen < count => run(self, chosen),
            Press::Char(c) => {
                let found = self.rows[row].actions.iter().position(|a| a.key == Some(c));
                match found {
                    Some(action) => run(self, action),
                    None => Outcome::Nothing,
                }
            }
            _ => Outcome::Nothing,
        }
    }

    fn leader_key(&mut self, press: Press) -> Outcome {
        let mut path = self.leader_path.take().unwrap_or_default();
        match press {
            Press::Escape => Outcome::Redraw,
            Press::Backspace => {
                if path.pop().is_some() {
                    self.leader_path = Some(path);
                }
                Outcome::Redraw
            }
            Press::Char(c) => match self.leader.step(&path, c) {
                LeaderStep::Matched(binding) => Outcome::Leader(binding),
                LeaderStep::Pending => {
                    path.push(c);
                    self.leader_path = Some(path);
                    Outcome::Redraw
                }
                LeaderStep::NoMatch => {
                    let keys: String = path.iter().chain([&c]).map(|k| format!(" {k}")).collect();
                    self.say(&format!("SPC{keys} isn't bound"))
                }
            },
            _ => {
                self.leader_path = Some(path);
                Outcome::Nothing
            }
        }
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

/// The which-key list as rows: `p  Projects`, groups as `f  +Fenix (3)`,
/// each key highlighted. When there are more hints than rows, they are
/// packed into columns, padded with spaces: Flint's font is monospace,
/// as Fenix's is, so the columns line up.
fn hint_rows(hints: &[crate::leader::Hint], visible: usize) -> Vec<Row> {
    let entries: Vec<(char, String)> = hints
        .iter()
        .map(|h| {
            let label = if h.group {
                format!("+{} ({})", h.label, h.count)
            } else {
                h.label.clone()
            };
            (h.key, label)
        })
        .collect();
    let columns = entries.len().div_ceil(visible.max(1)).max(1);
    let per_column = entries.len().div_ceil(columns).max(1);
    let width = entries
        .iter()
        .map(|(_, l)| l.chars().count())
        .max()
        .unwrap_or(0)
        + 3
        + 3;
    (0..per_column)
        .map(|line| {
            let mut title = String::new();
            let mut highlights = Vec::new();
            for column in 0..columns {
                let Some((key, label)) = entries.get(column * per_column + line) else {
                    break;
                };
                let start = title.chars().count();
                if column > 0 {
                    let pad = (column * width).saturating_sub(start);
                    title.extend(std::iter::repeat_n(' ', pad));
                }
                highlights.push(title.chars().count() as u32);
                title.push(*key);
                title.push_str("  ");
                title.push_str(label);
            }
            Row {
                title,
                highlights,
                ..Row::default()
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::leader::LeaderEntry;

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

    fn with_actions(n: usize) -> Launcher {
        let mut l = Launcher::default();
        l.set_visible_rows(3);
        let action = |title: &str, key: char, disabled: Option<&str>| MenuAction {
            title: title.into(),
            key: Some(key),
            disabled: disabled.map(str::to_owned),
        };
        l.set_rows(
            (0..n)
                .map(|i| Row {
                    title: format!("row {i}"),
                    actions: vec![
                        action("Open", 'o', None),
                        action("Open in Fenix", 'e', Some("needs a newer Fenix")),
                        action("Copy path", 'c', None),
                    ],
                    ..Row::default()
                })
                .collect(),
        );
        l
    }

    #[test]
    fn tab_opens_the_menu_and_its_keys_run_actions() {
        let mut l = with_actions(2);
        l.press(Press::Down);
        assert_eq!(l.press(Press::Tab), Outcome::Redraw);
        let view = l.view();
        assert!(view.overlay);
        assert_eq!(view.rows.len(), 3);
        assert_eq!(view.rows[1].subtitle, "needs a newer Fenix");
        assert!(view.rows[1].dimmed && !view.rows[0].dimmed);
        assert_eq!(view.rows[0].badge, "o");
        assert!(l.footer().text.starts_with("Actions for row 1"));
        assert_eq!(
            l.press(Press::Char('c')),
            Outcome::Run { row: 1, action: 2 }
        );
        assert!(!l.view().overlay, "running closes it");
    }

    #[test]
    fn a_disabled_action_says_why_and_stays_open() {
        let mut l = with_actions(1);
        l.press(Press::Tab);
        l.press(Press::Char('j'));
        assert_eq!(l.press(Press::Enter), Outcome::Redraw);
        assert_eq!(l.footer().text, "needs a newer Fenix");
        assert!(l.view().overlay);
        assert_eq!(l.press(Press::Char('k')), Outcome::Redraw);
        assert_eq!(l.press(Press::Enter), Outcome::Run { row: 0, action: 0 });
    }

    #[test]
    fn the_menu_closes_with_esc_or_tab_and_letters_dont_type() {
        let mut l = with_actions(1);
        l.press(Press::Escape);
        l.press(Press::Char('.'));
        assert!(l.view().overlay, "`.` opens it in Normal mode");
        assert_eq!(l.press(Press::Char('x')), Outcome::Nothing);
        l.press(Press::Tab);
        assert!(!l.view().overlay);
        l.press(Press::Char('i'));
        l.press(Press::Tab);
        assert_eq!(l.press(Press::Escape), Outcome::Redraw);
        assert_eq!(
            l.mode(),
            Mode::Insert,
            "Esc closed the menu, not Insert mode"
        );
    }

    fn with_leader() -> Launcher {
        let mut l = launcher(1);
        let entry = |keys: &str, label: &str, target: Option<usize>| LeaderEntry {
            keys: keys.chars().filter(|c| *c != ' ').collect(),
            label: label.into(),
            target,
        };
        l.set_leader(Arc::new(Leader::new(vec![
            entry("p", "Projects", Some(0)),
            entry("f r", "Recent files", Some(1)),
            entry("f", "Fenix", None),
        ])));
        l.press(Press::Escape);
        l
    }

    #[test]
    fn the_leader_walks_the_map_with_which_key() {
        let mut l = with_leader();
        assert_eq!(l.press(Press::Char(' ')), Outcome::Redraw);
        let titles: Vec<String> = l.view().rows.iter().map(|r| r.title.clone()).collect();
        assert_eq!(titles, ["f  +Fenix (1)", "p  Projects"]);
        assert_eq!(l.press(Press::Char('f')), Outcome::Redraw);
        assert!(l.footer().text.starts_with("SPC f (Fenix)"));
        assert_eq!(l.press(Press::Backspace), Outcome::Redraw);
        assert_eq!(l.press(Press::Char('f')), Outcome::Redraw);
        assert_eq!(l.press(Press::Char('r')), Outcome::Leader(1));
        assert!(!l.view().overlay);
    }

    #[test]
    fn an_unbound_key_or_esc_ends_the_leader() {
        let mut l = with_leader();
        l.press(Press::Char(' '));
        assert_eq!(l.press(Press::Char('z')), Outcome::Redraw);
        assert_eq!(l.footer().text, "SPC z isn't bound");
        assert!(!l.view().overlay);
        l.press(Press::Char(' '));
        l.press(Press::Escape);
        assert!(!l.view().overlay);
        assert_eq!(l.mode(), Mode::Normal);
        let mut empty = launcher(1);
        empty.press(Press::Escape);
        empty.press(Press::Char(' '));
        assert!(empty.footer().text.contains("[leader]"));
    }

    #[test]
    fn many_hints_are_packed_into_aligned_columns() {
        let hints: Vec<crate::leader::Hint> = "abcde"
            .chars()
            .map(|key| crate::leader::Hint {
                key,
                label: format!("item {key}"),
                group: false,
                count: 0,
            })
            .collect();
        let rows = hint_rows(&hints, 2);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "a  item a   c  item c   e  item e");
        assert_eq!(rows[0].highlights, [0, 12, 24]);
        assert_eq!(rows[1].title, "b  item b   d  item d");
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
