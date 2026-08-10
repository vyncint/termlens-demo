//! Shared spawn helpers for the termlens suites.
//!
//! This module is compiled into every integration-test binary, so helpers
//! only one suite uses would otherwise warn.
#![allow(dead_code)]

use std::time::Duration;

use termlens::{Screen, Terminal};

pub const COLS: u16 = 90;
pub const ROWS: u16 = 26;

/// Spawn taskboard at a given size and wait for its first painted frame.
pub fn spawn_sized(cols: u16, rows: u16) -> Terminal {
    let mut t = Terminal::builder()
        .size(cols, rows)
        // Hermetic: a developer's LS_COLORS or COLORTERM must not be able to
        // change a snapshot. TERM=xterm-256color is supplied by termlens.
        .env_clear()
        .timeout(Duration::from_secs(10))
        .spawn(env!("CARGO_BIN_EXE_taskboard"))
        .expect("spawn taskboard");
    // Wait on painted content, never on a bare delay. Note this waits for
    // "q quit" — the *rightmost text of the bottom row*, i.e. the last thing
    // the first frame paints. Waiting on "NORMAL" instead is a real race:
    // frames are not atomic and even a single row arrives in pieces, so the
    // mode indicator lands while the rest of the status line is still in
    // flight. See docs/TERMLENS-COVERAGE.md §2.
    t.wait_until(|s| s.contains("q quit")).expect("first frame");
    t
}

/// Let output stop before snapshotting a whole screen.
///
/// A heuristic, and the honest one: there is no frame-boundary signal, so
/// "nothing has arrived for a moment" is the best available evidence that a
/// repaint finished. Only needed when asserting on a *whole* screen; a
/// targeted `wait_until` predicate is exact and always preferred.
pub fn settle(t: &mut Terminal) {
    t.wait_idle(Duration::from_millis(100)).expect("settle");
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

/// Text of one row restricted to a column range — lets a test look at a
/// single pane instead of the whole row.
pub fn pane_text(screen: &Screen, row: u16, cols: std::ops::Range<u16>) -> String {
    cols.filter_map(|c| screen.cell(row, c))
        .filter(|cell| !cell.is_wide_continuation())
        .map(|cell| {
            if cell.contents().is_empty() {
                " ".to_string()
            } else {
                cell.contents().to_string()
            }
        })
        .collect()
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
