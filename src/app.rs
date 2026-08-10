//! Application state and the key-handling logic that drives it.
//!
//! Kept free of rendering so the state machine can be reasoned about (and
//! unit-tested) on its own; `ui` turns a `&App` into frames.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Which top-level tab is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    Stats,
    Logs,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Tasks, Tab::Stats, Tab::Logs];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Tasks => "Tasks",
            Tab::Stats => "Stats",
            Tab::Logs => "Logs",
        }
    }

    fn index(self) -> usize {
        Tab::ALL.iter().position(|t| *t == self).unwrap()
    }

    fn next(self) -> Tab {
        Tab::ALL[(self.index() + 1) % Tab::ALL.len()]
    }

    fn prev(self) -> Tab {
        Tab::ALL[(self.index() + Tab::ALL.len() - 1) % Tab::ALL.len()]
    }
}

/// What the app is doing right now — decides which keys mean what, and
/// whether an overlay is drawn on top of the main content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// Normal navigation.
    Normal,
    /// Typing into the filter box; carries the in-progress query.
    Filter { draft: String },
    /// Modal confirmation before deleting the selected task.
    ConfirmDelete,
    /// Help overlay.
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    Low,
    Medium,
    High,
}

impl Priority {
    pub fn label(self) -> &'static str {
        match self {
            Priority::Low => "low",
            Priority::Medium => "med",
            Priority::High => "HIGH",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Task {
    pub title: String,
    pub done: bool,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub notes: String,
}

impl Task {
    fn new(title: &str, done: bool, priority: Priority, tags: &[&str], notes: &str) -> Self {
        Self {
            title: title.to_string(),
            done,
            priority,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            notes: notes.to_string(),
        }
    }
}

/// Why the app stopped — mapped to a process exit code by `main`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quit {
    /// Clean quit via `q`.
    Normal,
    /// Interrupted via Ctrl-C.
    Interrupted,
}

pub struct App {
    pub tab: Tab,
    pub mode: Mode,
    pub tasks: Vec<Task>,
    /// Index into the *filtered* view, not into `tasks`.
    pub selected: usize,
    /// Applied filter (the committed one, not the in-progress draft).
    pub filter: String,
    pub logs: Vec<String>,
    pub log_offset: usize,
    /// How many rows the list pane last had — set by the renderer so paging
    /// keys can move by a real page.
    pub page_size: usize,
    pub quit: Option<Quit>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        // Deliberately includes CJK and emoji: double-width cells are where
        // column arithmetic goes wrong, so the fixture data exercises them.
        let tasks = vec![
            Task::new(
                "Wire up the PTY reader",
                true,
                Priority::High,
                &["core", "io"],
                "Drain continuously so the kernel buffer never stalls the child.",
            ),
            Task::new(
                "Snapshot the screen grid",
                true,
                Priority::High,
                &["core"],
                "Immutable snapshots taken under the reader lock.",
            ),
            Task::new(
                "帳票をレンダリングする",
                false,
                Priority::Medium,
                &["i18n"],
                "Wide glyphs must occupy two columns and one continuation cell.",
            ),
            Task::new(
                "Handle SIGWINCH",
                false,
                Priority::High,
                &["core", "layout"],
                "Resize the PTY and the emulated grid together.",
            ),
            Task::new(
                "Add bracketed paste",
                false,
                Priority::Low,
                &["input"],
                "Wrap pasted text in ESC[200~ / ESC[201~.",
            ),
            Task::new(
                "Support 🚀 emoji width",
                false,
                Priority::Low,
                &["i18n"],
                "Most emoji are double-width; a few are not.",
            ),
            Task::new(
                "Document the wait heuristics",
                false,
                Priority::Medium,
                &["docs"],
                "Silence is evidence a render finished, not proof.",
            ),
            Task::new(
                "Benchmark the parser",
                true,
                Priority::Low,
                &["perf"],
                "Snapshot caching keeps an idle terminal at one Arc clone.",
            ),
            Task::new(
                "Ship v0.2 styles block",
                false,
                Priority::Medium,
                &["docs", "core"],
                "Render per-cell attributes into the snapshot text.",
            ),
            Task::new(
                "Windows ConPTY support",
                false,
                Priority::Low,
                &["portability"],
                "portable-pty already speaks ConPTY; the harness does not.",
            ),
        ];

        let logs = (1..=40)
            .map(|i| format!("[{i:03}] event {i} — reader drained {} bytes", i * 17))
            .collect();

        Self {
            tab: Tab::Tasks,
            mode: Mode::Normal,
            tasks,
            selected: 0,
            filter: String::new(),
            logs,
            log_offset: 0,
            page_size: 10,
            quit: None,
        }
    }

    /// Indices into `tasks` that survive the current filter, in order.
    pub fn visible(&self) -> Vec<usize> {
        if self.filter.is_empty() {
            return (0..self.tasks.len()).collect();
        }
        let needle = self.filter.to_lowercase();
        self.tasks
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                t.title.to_lowercase().contains(&needle)
                    || t.tags.iter().any(|tag| tag.to_lowercase().contains(&needle))
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// The task under the cursor, if the filtered view is non-empty.
    pub fn selected_task(&self) -> Option<&Task> {
        self.visible().get(self.selected).map(|&i| &self.tasks[i])
    }

    pub fn done_count(&self) -> usize {
        self.tasks.iter().filter(|t| t.done).count()
    }

    pub fn count_by(&self, priority: Priority) -> usize {
        self.tasks.iter().filter(|t| t.priority == priority).count()
    }

    /// Feed one key press through the state machine.
    pub fn on_key(&mut self, key: KeyEvent) {
        // Ctrl-C always wins, in every mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = Some(Quit::Interrupted);
            return;
        }

        match &self.mode {
            Mode::Filter { .. } => self.on_key_filter(key),
            Mode::ConfirmDelete => self.on_key_confirm(key),
            Mode::Help => self.on_key_help(key),
            Mode::Normal => self.on_key_normal(key),
        }
    }

    fn on_key_filter(&mut self, key: KeyEvent) {
        let Mode::Filter { draft } = &mut self.mode else {
            return;
        };
        match key.code {
            KeyCode::Char(c) => draft.push(c),
            KeyCode::Backspace => {
                draft.pop();
            }
            KeyCode::Enter => {
                self.filter = draft.clone();
                self.mode = Mode::Normal;
                self.clamp_selection();
            }
            KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
    }

    fn on_key_confirm(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Enter => {
                let visible = self.visible();
                if let Some(&idx) = visible.get(self.selected) {
                    self.tasks.remove(idx);
                }
                self.mode = Mode::Normal;
                self.clamp_selection();
            }
            KeyCode::Char('n') | KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
    }

    fn on_key_help(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') | KeyCode::F(1) => {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    /// Text arriving as one bracketed-paste burst rather than as key presses.
    /// Newlines are stripped: a paste is a value, not a submit.
    pub fn on_paste(&mut self, text: &str) {
        if let Mode::Filter { draft } = &mut self.mode {
            draft.extend(text.chars().filter(|c| !c.is_control()));
        }
    }

    fn on_key_normal(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('q') => self.quit = Some(Quit::Normal),
            // Ctrl-arrows are an alternative to Tab/Shift-Tab.
            KeyCode::Right if ctrl => self.tab = self.tab.next(),
            KeyCode::Left if ctrl => self.tab = self.tab.prev(),
            KeyCode::Tab => self.tab = self.tab.next(),
            KeyCode::BackTab => self.tab = self.tab.prev(),
            KeyCode::Char('?') | KeyCode::F(1) => self.mode = Mode::Help,

            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            KeyCode::PageDown => self.move_by(self.page_size as isize),
            KeyCode::PageUp => self.move_by(-(self.page_size as isize)),
            KeyCode::Home => self.move_to_start(),
            KeyCode::End => self.move_to_end(),

            KeyCode::Char(' ') if self.tab == Tab::Tasks => self.toggle_done(),
            KeyCode::Char('d') if self.tab == Tab::Tasks => {
                if self.selected_task().is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
            }
            KeyCode::Char('/') if self.tab == Tab::Tasks => {
                self.mode = Mode::Filter {
                    draft: self.filter.clone(),
                };
            }
            KeyCode::Esc if self.tab == Tab::Tasks && !self.filter.is_empty() => {
                self.filter.clear();
                self.clamp_selection();
            }
            _ => {}
        }
    }

    /// Move the cursor within whichever list the current tab shows.
    fn move_by(&mut self, delta: isize) {
        let len = self.list_len();
        if len == 0 {
            return;
        }
        let cursor = self.cursor_mut_value();
        let next = (cursor as isize + delta).clamp(0, len as isize - 1) as usize;
        self.set_cursor(next);
    }

    fn move_to_start(&mut self) {
        self.set_cursor(0);
    }

    fn move_to_end(&mut self) {
        let len = self.list_len();
        self.set_cursor(len.saturating_sub(1));
    }

    fn list_len(&self) -> usize {
        match self.tab {
            Tab::Tasks => self.visible().len(),
            Tab::Logs => self.logs.len(),
            Tab::Stats => 0,
        }
    }

    fn cursor_mut_value(&self) -> usize {
        match self.tab {
            Tab::Logs => self.log_offset,
            _ => self.selected,
        }
    }

    fn set_cursor(&mut self, value: usize) {
        match self.tab {
            Tab::Logs => self.log_offset = value,
            _ => self.selected = value,
        }
    }

    fn toggle_done(&mut self) {
        let visible = self.visible();
        if let Some(&idx) = visible.get(self.selected) {
            self.tasks[idx].done = !self.tasks[idx].done;
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.visible().len();
        self.selected = self.selected.min(len.saturating_sub(1));
    }

    /// Select the task on a given row of the list pane. Only reachable by
    /// clicking — see `docs/TERMLENS-COVERAGE.md` on why that makes this
    /// path awkward to test through the typed `Key` API.
    pub fn select_row(&mut self, row: usize) {
        if self.tab == Tab::Tasks && row < self.visible().len() {
            self.selected = row;
        }
    }
}
