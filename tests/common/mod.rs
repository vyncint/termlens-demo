//! Shared spawn helpers for the termlens suites.
//!
//! This module is compiled into every integration-test binary, so helpers
//! only one suite uses would otherwise warn.
#![allow(dead_code)]

use std::time::Duration;

use termlens::{Screen, Terminal};

pub const COLS: u16 = 90;
pub const ROWS: u16 = 26;

/// Spawn taskboard at a given size and wait for its first complete frame.
pub fn spawn_sized(cols: u16, rows: u16) -> Terminal {
    let mut t = Terminal::builder()
        .size(cols, rows)
        // Hermetic: a developer's LS_COLORS or COLORTERM must not be able to
        // change a snapshot. TERM=xterm-256color is supplied by termlens.
        .env_clear()
        .timeout(Duration::from_secs(10))
        .spawn(env!("CARGO_BIN_EXE_taskboard"))
        .expect("spawn taskboard");
    // taskboard brackets every repaint in a DEC 2026 synchronized update, so
    // `wait_frame` only ever sees complete frames. Under 0.1 this predicate
    // was a race — "NORMAL" could land while the rest of its row was still
    // in flight — and the helper had to wait on the last text of the last
    // row instead. That discipline is no longer needed.
    t.wait_frame(|s| s.contains("NORMAL")).expect("first frame");
    t
}

/// Spawn at the standard test size.
pub fn spawn() -> Terminal {
    spawn_sized(COLS, ROWS)
}

/// Spawn a plain shell script in a PTY — for probing terminal behaviour that
/// has nothing to do with the TUI.
pub fn spawn_sh(script: &str, timeout: Duration) -> Terminal {
    Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(timeout)
        .args(["-c", script])
        .spawn("/bin/sh")
        .expect("spawn /bin/sh")
}

/// The style of the first cell of `needle`. Panics if the text isn't on
/// screen — a missing needle is a test bug worth failing loudly on.
pub fn style_at(screen: &Screen, needle: &str) -> termlens::Style {
    let (row, col) = screen
        .find(needle)
        .unwrap_or_else(|| panic!("{needle:?} not on screen:\n{screen}"));
    *screen
        .cell(row, col)
        .expect("find returned an in-bounds cell")
        .style()
}
