//! taskboard — a deliberately feature-dense TUI, built as a subject for
//! [`termlens`] integration tests: tabs, a filtered list, a kanban board, a
//! detail pane, a modal dialog, a progress run, a text input with a real
//! cursor, a help overlay, styled cells, wide (CJK/emoji) glyphs,
//! responsive layout, and mouse support.
//!
//! It also emits, on purpose, the things a screen-grid harness struggles
//! with: `OSC 8` hyperlinks, `OSC 52` clipboard writes, `OSC 4` palette
//! overrides, `DECSCUSR` cursor shapes, `BEL`, focus events, a burst of
//! complete frames, and — behind `--probe-sync` — a `DECRQM` capability
//! probe that decides whether synchronized updates are used at all.
//!
//! `tests/tui.rs` drives it through a real PTY, `tests/hard.rs` drives the
//! hard cases, and `docs/TERMLENS-COVERAGE.md` records what that reaches.

mod app;
mod ui;

use std::io::{self, Read, Stdout, Write};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::cursor::{RestorePosition, SavePosition};
use crossterm::event::{
    self, DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, BeginSynchronizedUpdate, EndSynchronizedUpdate,
    EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

use crate::app::{App, Effect, Mode, Quit};

/// Exit codes follow the shell convention of 128 + signal number. `q`
/// exits 0.
const EXIT_INTERRUPTED: u8 = 130; // 128 + SIGINT
const EXIT_TERMINATED: u8 = 143; // 128 + SIGTERM

/// How long the event loop blocks before re-checking the SIGTERM flag.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// How long a capability probe waits for its answer before concluding the
/// terminal has none. Real terminals answer in microseconds.
const PROBE_TIMEOUT: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, Default)]
struct Args {
    /// Ask the terminal (via `DECRQM`) whether it supports synchronized
    /// output, and bracket repaints only if it says yes. This is what a
    /// careful application does, and it is the difference between
    /// `wait_frame` working and never working.
    probe_sync: bool,
    /// Emit a batch of capability probes at startup and report how many
    /// were answered.
    probe_caps: bool,
}

fn parse_args() -> Args {
    let mut args = Args::default();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--probe-sync" => args.probe_sync = true,
            "--probe-caps" => args.probe_caps = true,
            _ => {}
        }
    }
    args
}

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
    let args = parse_args();
    let mut terminal = setup()?;
    let result = event_loop(&mut terminal, args);
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
        EnableFocusChange,
    )?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn restore(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    // Undo the palette override too: leaving a caller's terminal with
    // redefined colors is the same class of rudeness as leaving raw mode on.
    let out = terminal.backend_mut();
    write!(out, "{}", palette_sequences(false))?;
    write!(out, "\x1b[0 q")?; // DECSCUSR: back to the terminal default
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture,
        DisableBracketedPaste,
        DisableFocusChange,
    )?;
    terminal.show_cursor()
}

// ------------------------------------------------------- raw sequences

/// Minimal base64 for `OSC 52`, which takes its payload encoded.
fn base64(input: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(input.len().div_ceil(3) * 4);
    for chunk in input.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// `OSC 4` palette redefinition, or the reset form.
fn palette_sequences(high_contrast: bool) -> String {
    // Slots 1-6 are the ones the UI actually paints with.
    const HIGH: [(u8, &str); 6] = [
        (1, "ff/00/40"),
        (2, "00/ff/80"),
        (3, "ff/ff/00"),
        (4, "00/c0/ff"),
        (5, "ff/00/ff"),
        (6, "00/ff/ff"),
    ];
    let mut out = String::new();
    if high_contrast {
        for (slot, rgb) in HIGH {
            out.push_str(&format!("\x1b]4;{slot};rgb:{rgb}\x1b\\"));
        }
    } else {
        // OSC 104 resets palette entries to their defaults.
        out.push_str("\x1b]104\x1b\\");
    }
    out
}

/// Everything the cell buffer cannot carry, emitted straight after the
/// frame is drawn and before the synchronized update ends — so it is part
/// of the same atomic repaint.
fn emit_effects(app: &mut App) -> io::Result<()> {
    let mut out = io::stdout();

    // DECSCUSR: a bar while typing, a block otherwise. Real terminals
    // change shape; a grid model has no field for it.
    let shape = if matches!(app.mode, Mode::Filter { .. }) {
        "\x1b[5 q"
    } else {
        "\x1b[2 q"
    };
    write!(out, "{shape}")?;

    // The hyperlink: park the cursor on the label the renderer drew, repaint
    // it wrapped in OSC 8, then put the cursor back where ratatui left it.
    if let Some((x, y, label, url)) = app.link_hint.clone() {
        execute!(out, SavePosition)?;
        write!(out, "\x1b[{};{}H", y + 1, x + 1)?;
        write!(out, "\x1b]8;;{url}\x1b\\{label}\x1b]8;;\x1b\\")?;
        execute!(out, RestorePosition)?;
    }

    for effect in std::mem::take(&mut app.effects) {
        match effect {
            Effect::Bell => write!(out, "\x07")?,
            Effect::Copy(text) => {
                write!(out, "\x1b]52;c;{}\x1b\\", base64(text.as_bytes()))?;
            }
            Effect::Title(title) => write!(out, "\x1b]2;{title}\x07")?,
            Effect::Palette(on) => write!(out, "{}", palette_sequences(on))?,
        }
    }
    out.flush()
}

// ------------------------------------------------------- capability probes

/// Read whatever the terminal has sent us, up to `timeout`. Unix-only:
/// `poll(2)` is the only way to *not* consume the user's first keystroke
/// when the answer never comes.
fn read_reply(timeout: Duration) -> Vec<u8> {
    let mut collected = Vec::new();
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let mut fds = libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `fds` is a valid, initialized pollfd for stdin; poll(2)
        // writes only into `revents`.
        let ready = unsafe { libc::poll(&mut fds, 1, remaining.as_millis() as libc::c_int) };
        if ready <= 0 {
            break;
        }
        let mut buf = [0u8; 256];
        match io::stdin().read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => collected.extend_from_slice(&buf[..n]),
        }
        // A terminal answers in one write; a second poll round only serves
        // to pick up a straggler, so stop as soon as we have something.
        if !collected.is_empty() {
            break;
        }
    }
    collected
}

/// `DECRQM` for DEC private mode 2026: "do you support synchronized
/// output?" The answer is `CSI ? 2026 ; <state> $ y`, where state 1, 2 or 3
/// means the mode exists.
fn probe_synchronized_output() -> bool {
    let mut out = io::stdout();
    if write!(out, "\x1b[?2026$p").and_then(|()| out.flush()).is_err() {
        return false;
    }
    let reply = read_reply(PROBE_TIMEOUT);
    let text = String::from_utf8_lossy(&reply);
    match text.find("$y") {
        // `CSI ? 2026 ; 0 $ y` means "not recognized".
        Some(end) => !text[..end].ends_with(";0"),
        None => false,
    }
}

/// Ask everything at once, the way a capability-hungry application does at
/// startup, and report how many answers came back.
fn probe_capabilities() -> (usize, usize) {
    const PROBES: [&str; 6] = [
        "\x1b[c",          // DA1
        "\x1b[>c",         // DA2
        "\x1b[6n",         // DSR cursor position
        "\x1b]10;?\x1b\\", // foreground color
        "\x1b]11;?\x1b\\", // background color
        "\x1bP+q544e\x1b\\", // XTGETTCAP for "TN"
    ];
    let mut out = io::stdout();
    let mut answered = 0;
    for probe in PROBES {
        if write!(out, "{probe}").and_then(|()| out.flush()).is_err() {
            break;
        }
        if !read_reply(PROBE_TIMEOUT).is_empty() {
            answered += 1;
        }
    }
    (answered, PROBES.len())
}

// ----------------------------------------------------------- event loop

/// Draw one frame. When synchronized updates are in use it is bracketed in
/// DEC 2026, so the terminal (and any harness watching) is shown the frame
/// only once it is complete, never half-painted.
fn draw_frame(terminal: &mut Tui, app: &mut App, synchronized: bool) -> io::Result<()> {
    if synchronized {
        execute!(io::stdout(), BeginSynchronizedUpdate)?;
    }
    let result = terminal.draw(|frame| ui::render(frame, app));
    emit_effects(app)?;
    if synchronized {
        execute!(io::stdout(), EndSynchronizedUpdate)?;
    }
    result.map(|_| ())
}

fn event_loop(terminal: &mut Tui, args: Args) -> io::Result<Quit> {
    let mut app = App::new();

    // Default: always bracket repaints, which is what makes the app
    // frame-testable. With --probe-sync, do what a careful application
    // does instead — ask first, and go without if there is no answer.
    let synchronized = if args.probe_sync {
        probe_synchronized_output()
    } else {
        true
    };
    if args.probe_sync {
        let verdict = if synchronized { "yes" } else { "no" };
        app.logs
            .insert(0, format!("[000] DECRQM ?2026 supported: {verdict}"));
    }
    if args.probe_caps {
        let (answered, total) = probe_capabilities();
        app.logs
            .insert(0, format!("[000] capability probes: {answered}/{total}"));
    }

    // SIGTERM sets a flag rather than killing us outright, so the app can
    // finish its frame and shut down in an orderly way.
    let terminate = Arc::new(AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&terminate))?;

    app.effects.push(Effect::Title(app.window_title()));
    draw_frame(terminal, &mut app, synchronized)?;
    loop {
        // A run is not interactive: paint every step of it back to back, so
        // a whole sequence of complete frames lands inside one burst of
        // output with nothing to pace it.
        if matches!(app.mode, Mode::Running { .. }) {
            loop {
                draw_frame(terminal, &mut app, synchronized)?;
                if !app.step_run() {
                    break;
                }
            }
            draw_frame(terminal, &mut app, synchronized)?;
            continue;
        }

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
                Event::FocusGained => app.on_focus(true),
                Event::FocusLost => app.on_focus(false),
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
            draw_frame(terminal, &mut app, synchronized)?;
            return Ok(Quit::Terminated);
        }

        draw_frame(terminal, &mut app, synchronized)?;
        if let Some(quit) = app.quit {
            return Ok(quit);
        }
    }
}
