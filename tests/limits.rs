//! What termlens **0.2** still cannot do, demonstrated against the real
//! binary rather than asserted from reading the source.
//!
//! Every test here passes and pins *current* behaviour, so closing one of
//! these gaps will fail the test that encodes its workaround.
//!
//! For what 0.2 fixed relative to 0.1, see `docs/TERMLENS-COVERAGE.md`.

mod common;

use std::time::Duration;

use common::{spawn, spawn_sh};
use termlens::{Error, Key, Scroll, Terminal};

// ------------------------------------------------ 1. one frame, not a history

/// `wait_frame` guarantees frame-*consistent* screens, not observation of
/// every frame. Only the most recently completed frame is retained, so when
/// several complete inside one read burst the earlier ones are gone — and
/// no API can reach back for them.
///
/// This matters for asserting on rapid intermediate states: a progress
/// counter ticking 1→2→3 in one burst is only ever observable at 3.
#[test]
fn only_the_newest_frame_of_a_burst_survives() -> termlens::Result<()> {
    let mut t = spawn_sh(
        r"printf '\033[?2026h\033[HFRAME-A\033[?2026l\033[?2026h\033[HFRAME-B\033[?2026l\033[?2026h\033[HFRAME-C\033[?2026l'; read x",
        Duration::from_secs(2),
    );

    // All three frames have completed by the time this returns.
    t.wait_frame(|s| s.contains("FRAME-C"))?;

    // A and B were complete frames too, but they were never retained.
    let missed = t.wait_frame(|s| s.contains("FRAME-A"));
    assert!(
        matches!(missed, Err(Error::Timeout { .. })),
        "expected the earlier frame to be unreachable, got {missed:?}"
    );
    Ok(())
}

/// `wait_frame` only works for applications that bracket their repaints in
/// DEC 2026. For anything else — most CLIs, and any TUI that doesn't opt in
/// — it can never succeed, and you're back to `wait_until` and its
/// torn-frame discipline. The error at least says so plainly.
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

// -------------------------------------------- 2. Esc is still ambiguous on the wire

/// Unchanged from 0.1, and now documented in `Key::Esc`: a bare `0x1B`
/// followed immediately by another key is byte-identical to an Alt chord.
/// There is still no `send_after(delay)`, so the only fix is to make the
/// Esc's effect observable and wait for it — which requires it to *have*
/// an observable effect.
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
    merged.wait_frame(|s| s.contains("HELP"))?;
    assert!(
        merged.screen().contains("filter:core"),
        "expected the Esc to be swallowed into Alt('?')\n{}",
        merged.screen()
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

// ------------------------------------------------- 3. the mouse API is one button

/// `click` sends button 0 and `Scroll` has only `Up`/`Down`. Right-click,
/// middle-click, drag, and modifier+click (Ctrl-click to multi-select — a
/// common TUI idiom) have no API, so they still need hand-encoded SGR
/// bytes, exactly as *all* mouse input did in 0.1.
#[test]
fn right_click_and_drag_still_need_hand_rolled_bytes() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.paste("core");
    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("filter:core"))?;

    // There is no `t.click_with(Button::Right, …)`. SGR button 2 = right.
    t.send_str("\x1b[<2;11;7M");
    t.send_str("\x1b[<2;11;7m");
    t.wait_frame(|s| s.contains("tasks (13)") && !s.contains("filter:"))?;

    // And the wheel cannot go sideways: `Scroll` is a two-variant enum, so
    // horizontal scroll (SGR buttons 66/67) is unreachable too.
    t.scroll(10, 6, Scroll::Down)?;
    t.wait_frame(|s| s.contains("Tasks 2/13"))?;
    Ok(())
}

// ------------------------------- 4. queries it recognises but cannot answer

/// 0.2 answers DSR, DA1/DA2, text-area size and OSC 10/11 — enough that
/// ordinary capability probes now succeed. A few are still recognised but
/// unanswerable: the kitty keyboard protocol probe (`CSI ? u`), DA3, OSC 12
/// (cursor colour), `DECRQM`, `XTGETTCAP`. An app that *blocks* on one of
/// those still hangs.
///
/// What did change is the diagnosis: the timeout now names the query, so
/// the cause is in the failure message instead of requiring a strace.
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

/// The same diagnosis appears when query answering is switched off — which
/// is also the way to reproduce 0.1's behaviour deliberately.
#[test]
fn answering_can_be_disabled_and_the_note_still_fires() {
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(2))
        .answer_queries(false)
        .args(["-c", r#"printf '\033[6n'; read -r r; printf 'got\n'; read x"#])
        .spawn("/bin/sh")
        .expect("spawn");

    let err = t.wait_until(|s| s.contains("got")).expect_err("no answer");
    assert!(err.to_string().contains("^[[6n"), "{err}");
}

// ------------------------------------------------------------ 5. no scrollback

/// Unchanged, and still listed in the crate's own known limitations: the
/// emulator is built with **zero** scrollback rows, and resizing does not
/// reflow. Only the visible grid is testable.
#[test]
fn output_scrolled_off_the_top_is_unrecoverable() -> termlens::Result<()> {
    let mut t = spawn_sh(
        r#"i=1; while [ $i -le 100 ]; do echo "line $i"; i=$((i+1)); done; read x"#,
        Duration::from_secs(5),
    );

    t.wait_until(|s| s.contains("line 100"))?;

    let screen = t.screen();
    assert!(!screen.contains("line 1\n"), "line 1 is gone\n{screen}");
    assert!(!screen.contains("line 50"), "line 50 is gone\n{screen}");
    assert_eq!(screen.rows(), 24);
    Ok(())
}

// ------------------------------------------- 6. stdout and stderr are one stream

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

// ------------------------------------------- 7. per-call timeouts are wait_until only

/// 0.2 added [`Terminal::wait_until_for`], but there is no `wait_frame_for`
/// or `wait_idle_for`. A frame-driven suite that needs one long wait must
/// raise the builder timeout for every wait it makes — which is what makes
/// a genuinely stuck app take the long timeout on its first failure.
#[test]
fn only_wait_until_takes_a_per_call_timeout() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_millis(300))
        .args(["-c", "sleep 0.8; printf 'late\\n'; read x"])
        .spawn("/bin/sh")?;

    // wait_until has the escape hatch…
    t.wait_until_for(|s| s.contains("late"), Duration::from_secs(5))?;

    // …while wait_frame and wait_idle are stuck with the builder value.
    // (Shown on wait_idle, which needs only the timeout to be too short.)
    let err = t.wait_idle(Duration::from_secs(1)).expect_err("300ms deadline");
    assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    Ok(())
}
