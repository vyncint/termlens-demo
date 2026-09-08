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

/// Spawn taskboard with command-line arguments and an explicit timeout.
///
/// The timeout is a parameter because `wait_frame` and `wait_idle` have no
/// per-call override (see `docs/TERMLENS-COVERAGE.md` §2.8): a test that
/// expects one of them to *fail* has to lower the builder value or pay the
/// full deadline.
pub fn spawn_args(args: &[&str], timeout: Duration) -> Terminal {
    let mut t = Terminal::builder()
        .size(COLS, ROWS)
        .env_clear()
        .timeout(timeout)
        .args(args)
        .spawn(env!("CARGO_BIN_EXE_taskboard"))
        .expect("spawn taskboard");
    t.wait_until(|s| s.contains("NORMAL")).expect("first paint");
    t
}

/// Spawn a plain shell script in a PTY — for probing terminal behaviour that
/// has nothing to do with the TUI.
pub fn spawn_sh(script: &str, timeout: Duration) -> Terminal {
    spawn_sh_sized(script, timeout, 80, 24)
}

/// [`spawn_sh`] at an explicit size.
///
/// Needed since termlens 0.8, which refuses a mouse coordinate outside the
/// grid *before* consulting the encoding: probing what an encoding can carry
/// now requires a terminal wide enough to hold the column being probed.
pub fn spawn_sh_sized(script: &str, timeout: Duration, cols: u16, rows: u16) -> Terminal {
    Terminal::builder()
        .size(cols, rows)
        .env_clear()
        .timeout(timeout)
        .args(["-c", script])
        .spawn("/bin/sh")
        .expect("spawn /bin/sh")
}

/// A `Style` built by applying `f` to `base`.
///
/// `Style` became `#[non_exhaustive]` in termlens 0.10, so a consumer can no
/// longer write `Style { dim: true, ..base }` — the crate reserves the right
/// to add attributes without a major bump, which is the whole point of the
/// marker. Assertions that mean "this style and nothing else" still want a
/// whole value to compare against, so they build one here. A closure rather
/// than `let mut` + field assignment because `clippy::field_reassign_with_default`
/// is denied in this repository's CI.
pub fn style_with(base: termlens::Style, f: impl FnOnce(&mut termlens::Style)) -> termlens::Style {
    let mut style = base;
    f(&mut style);
    style
}

/// [`style_with`] from the default style.
pub fn style(f: impl FnOnce(&mut termlens::Style)) -> termlens::Style {
    style_with(termlens::Style::default(), f)
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
