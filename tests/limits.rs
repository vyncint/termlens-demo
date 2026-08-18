//! What termlens **0.4** still cannot do, demonstrated against the real
//! binary rather than asserted from reading the source.
//!
//! Every test here passes and pins *current* behaviour, so closing one of
//! these gaps will fail the test that encodes its workaround.
//!
//! Four of the nine limitations pinned by the 0.2 version of this file are
//! gone. Their replacements are not "no such feature" but **bounds and
//! disciplines**: retention has a limit, scrollback has a limit, a frame
//! predicate that is true of two retained frames resolves on the older one.
//! Those are the things a test author now has to know, and they are the
//! things this file is for. What the features *do* is covered in
//! `tests/tui.rs`; see `docs/TERMLENS-COVERAGE.md` for the comparison.

mod common;

use std::time::Duration;

use common::{spawn, spawn_sh};
use termlens::{Error, Key, Terminal};

// ------------------------------------------------ 1. frames: bounds, not absence

/// The retained history is **8 frames**. A longer burst drops its oldest,
/// and nothing can reach back for them.
///
/// This replaces the 0.2 pin (`only_the_newest_frame_of_a_burst_survives`),
/// which asserted that *one* frame was kept. That test still passed against
/// 0.4 — for an entirely different reason, because the frame it looked for
/// had been consumed rather than never retained. A pinning test that keeps
/// passing after the gap closes is worse than no pin at all, which is why
/// this one counts frames instead.
#[test]
fn a_burst_longer_than_eight_frames_drops_its_oldest() -> termlens::Result<()> {
    let mut script = String::new();
    for n in 1..=12 {
        script.push_str(&format!(r"\033[?2026h\033[HFRAME-{n:02}\033[?2026l"));
    }
    let mut t = spawn_sh(
        &format!("printf '{script}'; read x"),
        Duration::from_secs(2),
    );

    // Settle on the live screen: `wait_until` reads the grid, not the frame
    // ring, so it does not consume anything.
    t.wait_until(|s| s.contains("FRAME-12"))?;

    // The newest 8 are 05..=12, so 05 is the oldest still observable.
    t.wait_frame(|s| s.contains("FRAME-05"))?;

    let dropped = t.wait_frame(|s| s.contains("FRAME-01"));
    assert!(
        matches!(dropped, Err(Error::Timeout { .. })),
        "expected the 12th-from-last frame to be past the bound, got {dropped:?}"
    );
    Ok(())
}

/// New in 0.4, and the discipline that replaces "only one frame is kept":
/// each call scans the retained frames **oldest first**, so a predicate that
/// is true of more than one of them resolves on the *older* — which may not
/// be the state you meant.
///
/// The remedy is the one this suite already follows for `wait_until`: name
/// something only the frame you want can show.
#[test]
fn a_predicate_true_of_two_retained_frames_matches_the_older() -> termlens::Result<()> {
    // Two complete frames in one write. Both contain "COMMON"; only the
    // second contains "SECOND".
    let mut t = spawn_sh(
        concat!(
            r"printf '\033[?2026h\033[2J\033[HCOMMON first\033[?2026l",
            r"\033[?2026h\033[2J\033[HCOMMON SECOND\033[?2026l'; read x"
        ),
        Duration::from_secs(2),
    );
    t.wait_until(|s| s.contains("SECOND"))?;

    // The ambiguous predicate lands on the first frame, not the newest.
    let frame = t.wait_frame(|s| s.contains("COMMON"))?;
    assert!(
        frame.contains("first") && !frame.contains("SECOND"),
        "the older matching frame is the one returned:\n{frame}"
    );

    // Naming something unique to the newer frame reaches it.
    let frame = t.wait_frame(|s| s.contains("SECOND"))?;
    assert!(frame.contains("SECOND"));
    Ok(())
}

/// `wait_frame` still only works for applications that bracket their
/// repaints in DEC 2026. For anything else — most CLIs, and any TUI that
/// hasn't opted in — it can never succeed, and you are back to `wait_until`
/// and its torn-frame discipline. The error says so plainly.
#[test]
fn wait_frame_is_useless_without_synchronized_updates() {
    let mut t = spawn_sh("printf 'plain output\\n'; read x", Duration::from_secs(2));

    let err = t
        .wait_frame(|s| s.contains("plain output"))
        .expect_err("no synchronized updates were emitted");
    let message = err.to_string();
    assert!(
        message.contains("never emitted a DEC 2026 synchronized update"),
        "unexpected error: {message}"
    );
    // The text is on screen — `wait_until` would have matched immediately.
    assert!(err.screen().is_some_and(|s| s.contains("plain output")));
}

/// Documented in 0.4 rather than fixed, and deliberately so: `screen()`
/// returns the live grid, which can be **half-painted** even for an
/// application that brackets every repaint correctly. `wait_frame` is the
/// only frame-gated observation.
///
/// The alternative would be worse: serving the newest *complete* frame while
/// an update is open would let a `wait_until` predicate match content that
/// the following `screen()` does not show.
#[test]
fn a_snapshot_can_be_torn_even_for_a_synchronized_app() -> termlens::Result<()> {
    let mut t = spawn_sh(
        concat!(
            r"printf '\033[?2026h\033[2J\033[HROW-ONE'; read a; ",
            r"printf '\033[2;1HROW-TWO\033[?2026l'; read b"
        ),
        Duration::from_secs(5),
    );

    t.wait_until(|s| s.contains("ROW-ONE"))?;
    let torn = t.screen();
    assert!(torn.contains("ROW-ONE"));
    assert!(
        torn.row_text(1).trim_end().is_empty(),
        "the frame is still open, and screen() shows that:\n{torn}"
    );

    // `wait_idle` will not call this idle, which is what makes the documented
    // "settle before a whole-screen snapshot" recipe work.
    let idle = t.wait_idle_for(Duration::from_millis(50), Duration::from_millis(400));
    let message = idle.expect_err("an open frame is not idle").to_string();
    assert!(
        message.contains("unfinished DEC 2026 synchronized update"),
        "the timeout should name the real state: {message}"
    );

    t.send(Key::Enter);
    let whole = t.wait_frame(|s| s.contains("ROW-TWO"))?;
    assert!(whole.contains("ROW-ONE") && whole.contains("ROW-TWO"));
    Ok(())
}

// -------------------------------------------- 2. Esc is still ambiguous on the wire

/// Unchanged, and documented on `Key::Esc`: a bare `0x1B` followed
/// immediately by another key is byte-identical to an Alt chord. There is
/// still no `send_after(delay)`, so the only fix is to make the Esc's effect
/// observable and wait for it — which requires it to *have* an observable
/// effect.
///
/// The first half sends the two keys as one write, because that is what
/// makes the hazard deterministic: `send(Esc)` then `send(Char('?'))`
/// produces exactly these bytes, and whether the application sees one Alt
/// chord or two key presses then depends on whether its input loop happens
/// to read them together. A test that relied on that coin flip was itself
/// flaky at roughly 1 run in 5.
#[test]
fn esc_immediately_followed_by_a_key_is_still_read_as_alt() -> termlens::Result<()> {
    let mut merged = spawn();
    merged.send(Key::Char('/'));
    merged.wait_frame(|s| s.contains("FILTER"))?;
    merged.paste("core");
    merged.send(Key::Enter);
    merged.wait_frame(|s| s.contains("filter:core"))?;

    // `\x1b?` — byte-for-byte what Esc-then-'?' puts on the wire.
    merged.send_str("\x1b?");
    let frame = merged.wait_frame(|s| s.contains("HELP"))?;
    assert!(
        frame.contains("filter:core"),
        "expected the Esc to be swallowed into Alt('?')\n{frame}"
    );

    // Separated by an observation, both land.
    let mut separated = spawn();
    separated.send(Key::Char('/'));
    separated.wait_frame(|s| s.contains("FILTER"))?;
    separated.paste("core");
    separated.send(Key::Enter);
    separated.wait_frame(|s| s.contains("filter:core"))?;

    separated.send(Key::Esc);
    separated.wait_frame(|s| !s.contains("filter:core"))?; // <- the fix
    separated.send(Key::Char('?'));
    separated.wait_frame(|s| s.contains("HELP"))?;
    Ok(())
}

// ------------------------------- 3. queries it recognises but cannot answer

/// 0.4 answers DSR, DA1/DA2, text-area size, OSC 10/11 and **`DECRQM`** —
/// the last of which is why a probe-then-enable application now turns its
/// mouse on against termlens unmodified (`tests/tui.rs`). A few are still
/// recognised but unanswerable: the kitty keyboard protocol probe
/// (`CSI ? u`), DA3, OSC 12 (cursor colour), `XTGETTCAP`, and `OSC 52`
/// clipboard *reads*. An app that blocks on one of those still hangs.
///
/// What did change in 0.2, and still holds, is the diagnosis: the timeout
/// names the query, so the cause is in the failure message instead of
/// requiring a strace.
#[test]
fn unanswerable_queries_still_hang_but_say_why() {
    let mut t = spawn_sh(
        r#"printf '\033[?u'; read -r reply; printf 'got\n'; read x"#,
        Duration::from_secs(2),
    );

    let err = t
        .wait_until(|s| s.contains("got"))
        .expect_err("nothing answers the kitty probe");
    let message = err.to_string();
    assert!(message.contains("queried the terminal"), "{message}");
    assert!(message.contains("^[[?u"), "the query is named: {message}");
}

/// `OSC 52` writes are captured as of 0.4 (`Screen::clipboard`), but a
/// clipboard *read* is a different sequence and is not answered: an
/// application that copies and then reads back to verify still hangs.
#[test]
fn a_clipboard_read_is_recognised_but_not_answered() {
    let mut t = spawn_sh(
        r#"printf '\033]52;c;?\007'; read -r reply; printf 'got\n'; read x"#,
        Duration::from_secs(2),
    );

    let err = t
        .wait_until(|s| s.contains("got"))
        .expect_err("clipboard reads are not answered");
    assert!(
        err.to_string().contains("]52;c;?"),
        "the read should still be named: {err}"
    );
}

/// The same diagnosis appears when query answering is switched off — which
/// is also the way to reproduce 0.1's behaviour deliberately.
#[test]
fn answering_can_be_disabled_and_the_note_still_fires() {
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(2))
        .answer_queries(false)
        .args([
            "-c",
            r#"printf '\033[6n'; read -r r; printf 'got\n'; read x"#,
        ])
        .spawn("/bin/sh")
        .expect("spawn");

    let err = t.wait_until(|s| s.contains("got")).expect_err("no answer");
    assert!(err.to_string().contains("^[[6n"), "{err}");
}

// ------------------------------------------ 4. scrollback: bounded, and text only

/// Scrollback exists as of 0.4, so the 0.2 pin
/// (`output_scrolled_off_the_top_is_unrecoverable`) is gone — but the
/// history is **bounded**, and past the bound the oldest rows are dropped
/// exactly as before.
#[test]
fn scrollback_is_bounded_and_drops_its_oldest_rows() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(40, 4)
        .env_clear()
        .scrollback(10)
        .timeout(Duration::from_secs(5))
        .args([
            "-c",
            r#"i=1; while [ $i -le 60 ]; do echo "line $i"; i=$((i+1)); done; read x"#,
        ])
        .spawn("/bin/sh")?;

    t.wait_until(|s| s.contains("line 60"))?;
    let screen = t.screen();

    assert_eq!(
        screen.scrollback_rows(),
        10,
        "bounded at the configured length"
    );
    assert!(
        !screen.full_text().contains("line 1\n"),
        "line 1 is far past the bound:\n{}",
        screen.full_text()
    );
    // What survives is the newest window plus the visible grid.
    assert!(screen.full_text().contains("line 60"));
    Ok(())
}

/// History is **text only**. A scrolled-off row has no `Style` and no cell
/// addressing, so a style regression above the fold is not assertable — the
/// styled rendering covers the visible grid alone.
#[test]
fn scrolled_off_rows_keep_their_text_but_lose_their_styles() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(40, 3)
        .env_clear()
        .scrollback(50)
        .timeout(Duration::from_secs(5))
        .args([
            "-c",
            r"printf '\033[1;31mSTYLED-AWAY\033[0m\n'; printf 'a\nb\nc\nd\n'; read x",
        ])
        .spawn("/bin/sh")?;

    t.wait_until(|s| s.scrollback_text().contains("STYLED-AWAY"))?;
    let screen = t.screen();

    // The text is recoverable…
    assert!(screen.full_text().contains("STYLED-AWAY"));
    // …but it is off the grid, so no cell and no span describe it.
    assert!(!screen.contains("STYLED-AWAY"));
    assert!(screen.find("STYLED-AWAY").is_none());
    assert!(
        !screen.with_styles().to_string().contains("fg=1"),
        "the styled rendering covers the visible grid only:\n{}",
        screen.with_styles()
    );
    Ok(())
}

/// A resize does not reflow history: rows keep the width they were captured
/// at, so narrowing the terminal does not retroactively rewrap them.
#[test]
fn a_resize_does_not_reflow_scrollback() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(40, 3)
        .env_clear()
        .scrollback(50)
        .timeout(Duration::from_secs(5))
        .args([
            "-c",
            r"printf 'a-long-row-captured-at-forty-columns-x\n'; printf '1\n2\n3\n'; read x",
        ])
        .spawn("/bin/sh")?;

    t.wait_until(|s| s.scrollback_text().contains("a-long-row"))?;
    let before = t.screen().scrollback_text();

    t.resize(12, 3)?;
    t.wait_idle(Duration::from_millis(80))?;

    assert_eq!(
        t.screen().scrollback_text(),
        before,
        "history is not rewrapped by a resize"
    );
    Ok(())
}

/// Not a defect, and worth knowing: an application on the **alternate
/// screen** accumulates no history at all, because that is what a real
/// terminal does. taskboard repaints continuously and never scrolls
/// anything into scrollback.
#[test]
fn an_alternate_screen_app_has_no_scrollback() -> termlens::Result<()> {
    let mut t = spawn();
    for _ in 0..12 {
        t.send(Key::Down);
    }
    t.wait_until(|s| s.contains("Tasks 10/10"))?;

    assert_eq!(t.screen().scrollback_rows(), 0);
    assert_eq!(t.screen().full_text(), t.screen().text());
    Ok(())
}

// ------------------------------------------- 5. stdout and stderr are one stream

/// Inherent to PTY testing rather than a termlens defect, but it still
/// means "assert this went to stderr" is unanswerable.
#[test]
fn stdout_and_stderr_are_indistinguishable() -> termlens::Result<()> {
    let mut t = spawn_sh(
        r"echo to-stdout; echo to-stderr >&2; echo done; read x",
        Duration::from_secs(5),
    );

    t.wait_until(|s| s.contains("done"))?;
    let screen = t.screen();
    assert!(screen.contains("to-stdout"), "{screen}");
    assert!(screen.contains("to-stderr"), "{screen}");
    Ok(())
}
