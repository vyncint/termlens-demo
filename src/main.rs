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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event, KeyEventKind, MouseButton, MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
    EnterAlternateScreen, LeaveAlternateScreen, SetTitle,
};
use crossterm::execute;
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{App, Quit};

/// Exit codes follow the shell convention of 128 + signal number. `q`
/// exits 0.
const EXIT_INTERRUPTED: u8 = 130; // 128 + SIGINT
const EXIT_TERMINATED: u8 = 143; // 128 + SIGTERM

/// How long the event loop blocks before re-checking the SIGTERM flag.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

fn main() -> ExitCode {
    match run() {
        Ok(Quit::Normal) => ExitCode::SUCCESS,
        Ok(Quit::Interrupted) => ExitCode::from(EXIT_INTERRUPTED),
        Ok(Quit::Terminated) => ExitCode::from(EXIT_TERMINATED),
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

/// Draw one frame bracketed in a DEC 2026 synchronized update, so the
/// terminal (and any test harness watching it) is shown the frame only once
/// it is complete, never half-painted.
fn draw_frame(terminal: &mut Tui, app: &mut App) -> io::Result<()> {
    execute!(io::stdout(), BeginSynchronizedUpdate)?;
    let result = terminal.draw(|frame| ui::render(frame, app));
    execute!(io::stdout(), EndSynchronizedUpdate)?;
    result.map(|_| ())
}

fn event_loop(terminal: &mut Tui) -> io::Result<Quit> {
    let mut app = App::new();

    // SIGTERM sets a flag rather than killing us outright, so the app can
    // finish its frame and shut down in an orderly way.
    let terminate = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&terminate))?;

    draw_frame(terminal, &mut app)?;
    loop {
        // Poll rather than block, so the SIGTERM flag is noticed even when
        // no input is arriving.
        if event::poll(POLL_INTERVAL)? {
            match event::read()? {
                // Windows reports both press and release; only act on press
                // so one keystroke isn't handled twice.
                Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        // The list starts below the 3-row tab bar and its
                        // own top border.
                        if let Some(row) = mouse.row.checked_sub(4) {
                            app.select_row(usize::from(row));
                        }
                    }
                    // Right-click clears an applied filter.
                    MouseEventKind::Down(MouseButton::Right) => app.clear_filter(),
                    MouseEventKind::ScrollDown => app.scroll_by(1),
                    MouseEventKind::ScrollUp => app.scroll_by(-1),
                    _ => {}
                },
                Event::Paste(text) => app.on_paste(&text),
                // The redraw below is all a resize needs.
                Event::Resize(_, _) => {}
                _ => {}
            }
        } else if !terminate.load(Ordering::Relaxed) {
            // Nothing happened and no signal pending: don't repaint, or the
            // app would never be idle.
            continue;
        }

        if terminate.load(Ordering::Relaxed) && app.quit.is_none() {
            // Show the shutdown state for one frame, then leave.
            app.shutting_down = true;
            draw_frame(terminal, &mut app)?;
            return Ok(Quit::Terminated);
        }

        draw_frame(terminal, &mut app)?;
        if let Some(quit) = app.quit {
            return Ok(quit);
        }
    }
}
