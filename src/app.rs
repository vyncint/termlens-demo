//! Application state and the key-handling logic that drives it.
//!
//! Kept free of rendering so the state machine can be reasoned about (and
//! unit-tested) on its own; `ui` turns a `&App` into frames.
//!
//! Several features here exist because they are *hard to observe from a
//! test harness*, not because a task manager needs them: a clipboard yank
//! (OSC 52), a hyperlinked reference (OSC 8), a bell on rejected input, a
//! palette override (OSC 4), and a progress run that completes many frames
//! inside one write burst. `tests/hard.rs` records which of them termlens
//! can actually reach.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Which top-level tab is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    Board,
    Stats,
    Logs,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Tasks, Tab::Board, Tab::Stats, Tab::Logs];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Tasks => "Tasks",
            Tab::Board => "Board",
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
    /// A task is "running": the event loop paints one complete frame per
    /// percentage step, all inside a single burst of output.
    Running { pct: u8 },
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

/// Which board column a task sits in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Todo,
    Doing,
    Done,
}

impl Lane {
    pub const ALL: [Lane; 3] = [Lane::Todo, Lane::Doing, Lane::Done];

    pub fn title(self) -> &'static str {
        match self {
            Lane::Todo => "todo",
            Lane::Doing => "doing",
            Lane::Done => "done",
        }
    }
}

/// A side effect the renderer cannot express, emitted as a raw escape
/// sequence by `main` after the frame is drawn but before it is published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// `BEL` — rejected input.
    Bell,
    /// `OSC 52` — write the string to the system clipboard.
    Copy(String),
    /// `OSC 0/2` — retitle the window.
    Title(String),
    /// `OSC 4` — redefine palette slots (true = high contrast, false = reset).
    Palette(bool),
}

#[derive(Debug, Clone)]
pub struct Task {
    pub title: String,
    pub done: bool,
    pub priority: Priority,
    pub tags: Vec<String>,
    pub notes: String,
    /// Rendered as an `OSC 8` hyperlink: the label is on the grid, the
    /// target is not.
    pub url: Option<String>,
    /// Rendered with `SGR 8` (conceal). A real terminal hides it; the
    /// characters are still in the grid.
    pub secret: Option<String>,
    /// Draws a badge with `SGR 5` (blink).
    pub overdue: bool,
    /// In the `Doing` lane on the board.
    pub started: bool,
}

impl Task {
    fn new(title: &str, done: bool, priority: Priority, tags: &[&str], notes: &str) -> Self {
        Self {
            title: title.to_string(),
            done,
            priority,
            tags: tags.iter().map(|t| t.to_string()).collect(),
            notes: notes.to_string(),
            url: None,
            secret: None,
            overdue: false,
            started: false,
        }
    }

    fn url(mut self, url: &str) -> Self {
        self.url = Some(url.to_string());
        self
    }

    fn secret(mut self, secret: &str) -> Self {
        self.secret = Some(secret.to_string());
        self
    }

    fn overdue(mut self) -> Self {
        self.overdue = true;
        self
    }

    fn started(mut self) -> Self {
        self.started = true;
        self
    }

    pub fn lane(&self) -> Lane {
        if self.done {
            Lane::Done
        } else if self.started {
            Lane::Doing
        } else {
            Lane::Todo
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
    /// SIGTERM: shut down gracefully, then exit 143.
    Terminated,
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
    /// Which board column has the cursor.
    pub lane: usize,
    /// How many rows the list pane last had — set by the renderer so paging
    /// keys can move by a real page.
    pub page_size: usize,
    pub quit: Option<Quit>,
    /// Set while the app is winding down after SIGTERM, so the last frame
    /// can say so before the process leaves.
    pub shutting_down: bool,
    /// A transient notice, shown on exactly one frame and then cleared —
    /// the state a harness can only catch if it retains completed frames.
    pub toast: Option<String>,
    /// False while the terminal reports the window is unfocused (mode 1004).
    pub focused: bool,
    /// High-contrast palette applied via `OSC 4`.
    pub high_contrast: bool,
    /// Whatever was last yanked, so the UI can say so.
    pub clipboard: Option<String>,
    /// Escape sequences for `main` to emit with the next frame.
    pub effects: Vec<Effect>,
    /// Set by the renderer: where the hyperlink label landed, and its
    /// target, so `main` can re-emit it wrapped in `OSC 8`.
    pub link_hint: Option<(u16, u16, String, String)>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        // Deliberately includes CJK, a ZWJ emoji sequence, a regional-
        // indicator flag, a VS16 emoji, decomposed (NFD) text and a title
        // with trailing whitespace: every one of them is a place where a
        // harness's column arithmetic or its text comparison can disagree
        // with what a real terminal shows.
        let tasks = vec![
            Task::new(
                "Wire up the PTY reader",
                true,
                Priority::High,
                &["core", "io"],
                "Drain continuously so the kernel buffer never stalls the child.",
            )
            .url("https://example.invalid/rfc/pty-reader"),
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
            )
            .started(),
            Task::new(
                "Handle SIGWINCH",
                false,
                Priority::High,
                &["core", "layout"],
                "Resize the PTY and the emulated grid together.",
            )
            .overdue(),
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
            )
            .started(),
            Task::new(
                "Windows ConPTY support",
                false,
                Priority::Low,
                &["portability"],
                "portable-pty already speaks ConPTY; the harness does not.",
            ),
            // NFD: 'e' + U+0301, not the precomposed U+00E9. A test that
            // greps for "café" as typed in its own source will miss this.
            Task::new(
                "Rotate the cafe\u{301} credentials",
                false,
                Priority::High,
                &["ops", "secret"],
                "Decomposed text renders identically and compares differently.",
            )
            .secret("hunter2-rotate-me")
            .overdue(),
            // A ZWJ sequence, a regional-indicator flag, and a VS16 emoji:
            // three different ways a terminal's idea of "one glyph, N
            // columns" can diverge from an emulator's.
            Task::new(
                "Audit 👨‍👩‍👧 🇻🇳 ❤️ glyph widths",
                false,
                Priority::Medium,
                &["i18n"],
                "ZWJ sequences, flags and variation selectors each count differently.",
            ),
            // Trailing whitespace survives in the grid but not in a
            // trailing-whitespace-stripped rendering of it.
            Task::new(
                "Trim trailing space   ",
                false,
                Priority::Low,
                &["ui"],
                "The three spaces after the title are real cells.",
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
            lane: 0,
            page_size: 10,
            quit: None,
            shutting_down: false,
            toast: None,
            focused: true,
            high_contrast: false,
            clipboard: None,
            effects: Vec::new(),
            link_hint: None,
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
                    || t.tags
                        .iter()
                        .any(|tag| tag.to_lowercase().contains(&needle))
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

    pub fn open_count(&self) -> usize {
        self.tasks.len() - self.done_count()
    }

    pub fn count_by(&self, priority: Priority) -> usize {
        self.tasks.iter().filter(|t| t.priority == priority).count()
    }

    /// Indices of the tasks in one board lane.
    pub fn lane_tasks(&self, lane: Lane) -> Vec<usize> {
        self.visible()
            .into_iter()
            .filter(|&i| self.tasks[i].lane() == lane)
            .collect()
    }

    /// The window title, which tracks state rather than being set once.
    pub fn window_title(&self) -> String {
        format!("taskboard — {} open", self.open_count())
    }

    fn notify(&mut self, message: impl Into<String>) {
        self.toast = Some(message.into());
    }

    fn reject(&mut self) {
        self.effects.push(Effect::Bell);
    }

    /// Feed one key press through the state machine.
    pub fn on_key(&mut self, key: KeyEvent) {
        // A new key means the previous frame's transient notice is spent.
        self.toast = None;

        // Ctrl-C always wins, in every mode.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.quit = Some(Quit::Interrupted);
            return;
        }

        match &self.mode {
            Mode::Filter { .. } => self.on_key_filter(key),
            Mode::ConfirmDelete => self.on_key_confirm(key),
            Mode::Help => self.on_key_help(key),
            // A run is not interactive; any key is ignored until it ends.
            Mode::Running { .. } => {}
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
                if draft.pop().is_none() {
                    // Nothing left to erase: say so audibly.
                    self.reject();
                }
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
                    let title = self.tasks[idx].title.clone();
                    self.tasks.remove(idx);
                    self.notify(format!("deleted {title}"));
                    self.retitle();
                }
                self.mode = Mode::Normal;
                self.clamp_selection();
            }
            KeyCode::Char('n') | KeyCode::Esc => self.mode = Mode::Normal,
            // A modal that ignores a key silently is indistinguishable from
            // one that hung — ring instead.
            _ => self.reject(),
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
    /// Control characters are stripped: a paste is a value, not a submit.
    pub fn on_paste(&mut self, text: &str) {
        if let Mode::Filter { draft } = &mut self.mode {
            draft.extend(text.chars().filter(|c| !c.is_control()));
        }
    }

    /// The terminal reported a focus change (mode 1004).
    pub fn on_focus(&mut self, focused: bool) {
        self.focused = focused;
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

            // Board lane movement.
            KeyCode::Char('h') | KeyCode::Left if self.tab == Tab::Board => {
                self.lane = self.lane.saturating_sub(1);
            }
            KeyCode::Char('l') | KeyCode::Right if self.tab == Tab::Board => {
                self.lane = (self.lane + 1).min(Lane::ALL.len() - 1);
            }

            KeyCode::Char(' ') if self.tab == Tab::Tasks => self.toggle_done(),
            KeyCode::Char('d') if self.tab == Tab::Tasks => {
                if self.selected_task().is_some() {
                    self.mode = Mode::ConfirmDelete;
                } else {
                    self.reject();
                }
            }
            KeyCode::Char('/') if self.tab == Tab::Tasks => {
                self.mode = Mode::Filter {
                    draft: self.filter.clone(),
                };
            }
            // Yank the selected title to the system clipboard (OSC 52).
            KeyCode::Char('y') => match self.selected_task() {
                Some(task) => {
                    let title = task.title.clone();
                    self.clipboard = Some(title.clone());
                    self.effects.push(Effect::Copy(title.clone()));
                    self.notify(format!("copied {title}"));
                }
                None => self.reject(),
            },
            // Run the selected task: many complete frames, one burst.
            KeyCode::Char('r') if self.tab == Tab::Tasks => {
                if self.selected_task().is_some() {
                    self.mode = Mode::Running { pct: 0 };
                } else {
                    self.reject();
                }
            }
            // Redefine palette slots 1-6 (OSC 4).
            KeyCode::Char('T') => {
                self.high_contrast = !self.high_contrast;
                self.effects.push(Effect::Palette(self.high_contrast));
                let state = if self.high_contrast { "on" } else { "off" };
                self.notify(format!("high contrast {state}"));
            }
            KeyCode::Esc if self.tab == Tab::Tasks && !self.filter.is_empty() => {
                self.filter.clear();
                self.clamp_selection();
            }
            _ => {}
        }
    }

    /// Advance a run by one step. Returns false once the run is over.
    pub fn step_run(&mut self) -> bool {
        let Mode::Running { pct } = self.mode else {
            return false;
        };
        if pct >= 100 {
            let visible = self.visible();
            if let Some(&idx) = visible.get(self.selected) {
                self.tasks[idx].done = true;
                self.tasks[idx].started = false;
                let title = self.tasks[idx].title.clone();
                self.notify(format!("finished {title}"));
            }
            self.mode = Mode::Normal;
            self.retitle();
            return false;
        }
        self.mode = Mode::Running { pct: pct + 10 };
        true
    }

    fn retitle(&mut self) {
        let title = self.window_title();
        self.effects.push(Effect::Title(title));
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
            Tab::Board => self.lane_tasks(Lane::ALL[self.lane]).len(),
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
            if self.tasks[idx].done {
                self.tasks[idx].started = false;
            }
            self.retitle();
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.visible().len();
        self.selected = self.selected.min(len.saturating_sub(1));
    }

    /// Select the task on a given row of the list pane — the click target.
    pub fn select_row(&mut self, row: usize) {
        if self.tab == Tab::Tasks && row < self.visible().len() {
            self.selected = row;
        }
    }

    /// Move the cursor by a wheel notch.
    pub fn scroll_by(&mut self, delta: isize) {
        self.move_by(delta);
    }

    /// Drop any applied filter — the right-click action.
    pub fn clear_filter(&mut self) {
        self.filter.clear();
        self.clamp_selection();
    }
}
