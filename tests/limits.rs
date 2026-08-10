//! Where termlens v0.1 stops — each limitation demonstrated against the
//! real binary rather than asserted from reading the source.
//!
//! Every test here passes. They pin *current* behaviour, so if a later
//! version closes one of these gaps, the test that encodes the workaround
//! fails and points at the docs that need updating.
//!
//! Narrative version: `docs/TERMLENS-COVERAGE.md`.

mod common;

use std::time::Duration;

use common::{pane_text, spawn, spawn_sh, style_at};
use termlens::{Error, Key};

// ------------------------------------------------------- 1. no mouse in `Key`

/// `Key` has no mouse variants, so a mouse-driven code path can only be
/// reached by hand-encoding an SGR mouse report and pushing it through
/// `send_str`. It works — but the test now owns a protocol detail that the
/// typed API exists to hide, and nothing checks it against the mode the app
/// actually enabled (1000 vs 1002 vs 1006).
#[test]
fn mouse_clicks_require_hand_rolled_escape_bytes() -> termlens::Result<()> {
    let mut t = spawn();

    // SGR (1006) press then release of button 0. Coordinates are 1-based:
    // screen row 7 is the third row of the list.
    t.send_str("\x1b[<0;10;7M");
    t.send_str("\x1b[<0;10;7m");

    t.wait_until(|s| s.contains("Tasks 3/10"))?;
    assert!(t.screen().contains("priority med"), "{}", t.screen());
    Ok(())
}

// -------------------------------------------- 2. no modifier+special-key keys

/// `Key::Ctrl` takes a *char* and encodes a C0 control byte, so it cannot
/// express Ctrl with a special key: Ctrl-Right, Shift-Up, Alt-PageDown and
/// friends have no representation. The CSI-modifier form has to be typed out
/// by hand.
#[test]
fn ctrl_arrow_chords_are_not_expressible_as_a_key() -> termlens::Result<()> {
    let mut t = spawn();

    // Ctrl-Right == CSI 1 ; 5 C. There is no `Key` that produces this.
    t.send_str("\x1b[1;5C");
    t.wait_until(|s| s.contains("stats") && s.contains("total    10"))?;

    // Ctrl-Left == CSI 1 ; 5 D.
    t.send_str("\x1b[1;5D");
    t.wait_until(|s| s.contains("tasks (10)"))?;
    Ok(())
}

// ------------------------------------------------------ 3. no bracketed paste

/// A paste is not a burst of key presses — an app that enables bracketed
/// paste sees one `Paste` event, and `Key` cannot produce one. The wrapper
/// has to be written literally.
#[test]
fn bracketed_paste_has_no_typed_api() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_until(|s| s.contains("FILTER"))?;

    // Typing the same characters would produce four key presses; this is
    // one paste event.
    t.send_str("\x1b[200~core\x1b[201~");
    t.wait_until(|s| s.contains("/core"))?;

    t.send(Key::Enter);
    t.wait_until(|s| s.contains("tasks (4) filtered") && s.contains("filter:core"))?;
    Ok(())
}

// ------------------------------------------------- 4. Esc is ambiguous on the wire

/// `Key::Esc` sends a bare `0x1B`. Immediately followed by another key, the
/// bytes are *identical* to an Alt chord, and every input parser resolves the
/// ambiguity the same way: it reads one Alt chord. Real keyboards are saved
/// by the human delay between presses; `send` writes back-to-back with none.
///
/// There is no `send_after(delay)` in v0.1, so the fix is to make the first
/// key's effect observable and wait for it before sending the second.
#[test]
fn esc_immediately_followed_by_a_key_is_read_as_alt() -> termlens::Result<()> {
    // Back-to-back: the Esc is swallowed into Alt('?'), so the filter that
    // Esc was supposed to clear survives.
    let mut merged = spawn();
    merged.send(Key::Char('/'));
    merged.wait_until(|s| s.contains("FILTER"))?;
    merged.send_str("core");
    merged.send(Key::Enter);
    merged.wait_until(|s| s.contains("filter:core"))?;

    merged.send(Key::Esc);
    merged.send(Key::Char('?'));
    merged.wait_until(|s| s.contains("HELP"))?;
    assert!(
        merged.screen().contains("filter:core"),
        "expected the Esc to be absorbed into an Alt chord\n{}",
        merged.screen()
    );

    // Separated by an observation, both keys land.
    let mut separated = spawn();
    separated.send(Key::Char('/'));
    separated.wait_until(|s| s.contains("FILTER"))?;
    separated.send_str("core");
    separated.send(Key::Enter);
    separated.wait_until(|s| s.contains("filter:core"))?;

    separated.send(Key::Esc);
    separated.wait_until(|s| !s.contains("filter:core"))?; // <- the fix
    separated.send(Key::Char('?'));
    separated.wait_until(|s| s.contains("HELP"))?;
    Ok(())
}

// -------------------------------------------- 5. the terminal never answers back

/// termlens renders what the app writes; it never writes back. An app that
/// *asks* the terminal a question — DSR cursor position, DA device
/// attributes, OSC 11 background colour, kitty-keyboard support — waits
/// forever for a reply that no one will send.
///
/// Here the shell asks for the cursor position and blocks on `read`. The
/// wait can only end in a timeout.
#[test]
fn terminal_queries_are_never_answered() {
    let mut t = spawn_sh(
        r#"printf '\033[6n'; read -r reply; printf 'got:[%s]\n' "$reply"; read x"#,
        Duration::from_secs(2),
    );

    let outcome = t.wait_until(|s| s.contains("got:"));
    match outcome {
        Err(Error::Timeout { .. }) => {} // the documented behaviour
        other => panic!("expected a timeout waiting for a DSR reply, got {other:?}"),
    }
}

// ------------------------------------------------------------ 6. no scrollback

/// The emulator is constructed with **zero** scrollback rows, so anything
/// that scrolls off the top is gone. Only the visible grid is testable —
/// which is fine for a full-screen TUI, and a hard wall for a log-spewing
/// CLI where the interesting line is 200 rows back.
#[test]
fn output_scrolled_off_the_top_is_unrecoverable() -> termlens::Result<()> {
    let mut t = spawn_sh(
        r#"i=1; while [ $i -le 100 ]; do echo "line $i"; i=$((i+1)); done; read x"#,
        Duration::from_secs(5),
    );

    t.wait_until(|s| s.contains("line 100"))?;

    let screen = t.screen();
    assert!(!screen.contains("line 1\n"), "line 1 should be gone\n{screen}");
    assert!(!screen.contains("line 50"), "line 50 should be gone\n{screen}");
    // A 24-row screen keeps only the last 24 lines; there is no API that
    // reaches past them.
    assert_eq!(screen.rows(), 24);
    Ok(())
}

// ------------------------------------------------ 7. styles are not snapshotted

/// Per-cell styles are captured and queryable via `Cell::style`, but the
/// `Display`/snapshot format is text only. So a regression that changes
/// *only* styling — the selection highlight landing on the wrong row, a
/// priority losing its colour — is invisible to `assert_screen_snapshot!`.
#[test]
fn moving_the_highlight_does_not_change_the_snapshot_text() -> termlens::Result<()> {
    let mut t = spawn();

    // Rows 4 and 5 are the first two list entries; columns 0..40 are the
    // list pane, excluding the detail pane which legitimately changes.
    let before = t.screen();
    let before_rows = [
        pane_text(&before, 4, 0..40),
        pane_text(&before, 5, 0..40),
    ];
    assert!(style_at(&before, "[x] HIGH Wire up").reverse);

    t.send(Key::Down);
    t.wait_until(|s| s.contains("Tasks 2/10"))?;

    let after = t.screen();
    let after_rows = [pane_text(&after, 4, 0..40), pane_text(&after, 5, 0..40)];

    // The highlight moved…
    assert!(!style_at(&after, "[x] HIGH Wire up").reverse);
    assert!(style_at(&after, "[x] HIGH Snapshot").reverse);
    // …and the text a snapshot would compare is byte-identical.
    assert_eq!(
        before_rows, after_rows,
        "text-only snapshots cannot see a highlight move"
    );
    Ok(())
}

// ------------------------------------------- 8. stdout and stderr are one stream

/// A PTY has a single output stream. Whatever the app writes to stderr is
/// interleaved into the same grid, so "assert this went to stderr" is not a
/// question termlens can answer.
#[test]
fn stdout_and_stderr_are_indistinguishable() -> termlens::Result<()> {
    let mut t = spawn_sh(
        r#"echo to-stdout; echo to-stderr >&2; echo done; read x"#,
        Duration::from_secs(5),
    );

    t.wait_until(|s| s.contains("done"))?;
    let screen = t.screen();
    assert!(screen.contains("to-stdout"), "{screen}");
    assert!(screen.contains("to-stderr"), "{screen}");
    // Both are just rows; nothing marks which stream produced them.
    Ok(())
}

// ------------------------------------------------- 9. `find` is single-row only

/// `contains` joins rows with `\n` and so matches across them; `find` scans
/// row by row and cannot. There is no "locate this multi-row block" API, so
/// anchoring on a box-drawn widget means finding one row and doing the
/// arithmetic yourself.
#[test]
fn find_cannot_locate_text_that_spans_rows() {
    let t = spawn();
    let screen = t.screen();

    let single_row = "[x] HIGH Wire up the PTY reader";
    let (row, col) = screen.find(single_row).expect("single-row needle");
    assert_eq!(col, 1, "found at the real column, inside the border");

    // Two whole rows joined exactly the way `text()` joins them — trailing
    // whitespace stripped per line.
    let across = format!(
        "{}\n{}",
        screen.row_text(row).trim_end(),
        screen.row_text(row + 1).trim_end()
    );

    assert!(screen.contains(&across), "contains matches across rows");
    assert_eq!(screen.find(&across), None, "find does not");
}

// --------------------------------------- 10. no frame boundary to synchronise on

/// The reader thread feeds every chunk of PTY output into the emulator as it
/// arrives, and `wait_until` re-evaluates on each one. Nothing marks where
/// one repaint ends and the next begins, so a predicate can fire on a
/// half-painted frame — including *half a row*.
///
/// This test pins the mitigation rather than the failure: match the last
/// thing the app paints, and settle before whole-screen snapshots. The
/// failure itself is timing-dependent (it reproduced roughly twice in
/// fifteen loaded runs while this suite was being written) and would make a
/// flaky test, which is exactly the thing being warned about.
#[test]
fn a_frame_is_only_complete_once_its_last_cell_arrives() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('d'));
    // "CONFIRM" is at the *start* of the status row. Waiting only for it can
    // return with the rest of that row still in flight.
    t.wait_until(|s| s.contains("CONFIRM"))?;
    // "q quit" is the row's tail — the last text the frame paints.
    t.wait_until(|s| s.row_text(s.rows() - 1).contains("q quit"))?;

    let screen = t.screen();
    assert!(screen.contains("delete this task?"), "{screen}");
    assert!(
        screen.row_text(screen.rows() - 1).contains("CONFIRM"),
        "{screen}"
    );
    Ok(())
}

// ------------------------------------------ 11. terminal state that isn't a cell

/// The `Screen` model is a grid plus a cursor. Terminal state that lives
/// *outside* the grid has no accessor: the window title the app set (OSC 0),
/// whether the alternate screen is active, the cursor's shape or blink, OSC 8
/// hyperlink targets, the clipboard set via OSC 52.
///
/// The app sets its title to "taskboard" on startup; the only reason that
/// string is assertable at all is that it independently appears as a border
/// label. Alt-screen state can likewise only be inferred — here, from the
/// frame vanishing on exit.
#[test]
fn out_of_band_terminal_state_is_only_observable_by_inference() -> termlens::Result<()> {
    let mut t = spawn();

    // This matches the *border label*, not the window title — there is no
    // `screen.title()` to check the OSC 0 the app sent.
    assert!(t.screen().contains("taskboard"), "{}", t.screen());

    t.send(Key::Char('q'));
    t.wait_exit()?;

    // Inference: the alternate screen was left, because its contents are
    // gone. No `screen.alternate_screen()` exists to ask directly.
    t.wait_until(|s| !s.contains("taskboard"))?;
    Ok(())
}
