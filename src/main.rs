//! taskboard — a deliberately feature-dense TUI, built as a subject for
//! [`termlens`] integration tests: tabs, a filtered list, a detail pane, a
//! modal dialog, a text input with a real cursor, a help overlay, styled
//! cells, wide (CJK/emoji) glyphs, responsive layout, and mouse support.
//!
//! `tests/tui.rs` drives it through a real PTY;
//! `docs/TERMLENS-COVERAGE.md` records what that can and cannot reach.

mod app;
mod ui;

use std::io::{self, Stdout};
use std::process::ExitCode;

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind, MouseButton, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{App, Quit};

/// Exit code for a Ctrl-C interrupt, following the shell convention
/// (128 + SIGINT). `q` exits 0.
const EXIT_INTERRUPTED: u8 = 130;

fn main() -> ExitCode {
    match run() {
        Ok(Quit::Normal) => ExitCode::SUCCESS,
        Ok(Quit::Interrupted) => ExitCode::from(EXIT_INTERRUPTED),
        Err(e) => {
            eprintln!("taskboard: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> io::Result<Quit> {
    let mut terminal = setup()?;
    let result = event_loop(&mut terminal);
    // Restore the terminal even if the loop failed — a panicking or erroring
    // TUI that leaves raw mode on wrecks the user's shell.
    restore(&mut terminal)?;
    result
}

type Tui = Terminal<CrosstermBackend<Stdout>>;

fn setup() -> io::Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste,
        SetTitle("taskboard")
    )?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste
    )?;
    terminal.show_cursor()
}

fn event_loop(terminal: &mut Tui) -> io::Result<Quit> {
    let mut app = App::new();
    loop {
        terminal.draw(|frame| ui::render(frame, &mut app))?;

        match event::read()? {
            // Windows reports both press and release; only act on press so
            // one keystroke isn't handled twice.
            Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
            Event::Mouse(mouse) => {
                if let MouseEventKind::Down(MouseButton::Left) = mouse.kind {
                    // The list starts below the 3-row tab bar and its own
                    // top border.
                    if let Some(row) = mouse.row.checked_sub(4) {
                        app.select_row(usize::from(row));
                    }
                }
            }
            Event::Paste(text) => app.on_paste(&text),
            // Redraw happens at the top of the loop; nothing else to do.
            Event::Resize(_, _) => {}
            _ => {}
        }

        if let Some(quit) = app.quit {
            return Ok(quit);
        }
    }
}
