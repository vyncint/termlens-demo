//! What termlens **0.4** still cannot do, demonstrated against the real
//! binary rather than asserted from reading the source.
//!
//! Every test here passes and pins *current* behaviour, so closing one of
//! these gaps will fail the test that encodes its workaround.
//!
//! **That contract was broken once and it is worth saying how.** Four of the
//! nine pins in the 0.2 version of this file stayed green against 0.4 while
//! their claims went false — because each asserted a *symptom* the closed
//! gap still shares with its replacement. "No frame history" was pinned by a
//! frame being unreachable, which is also true once the frame has merely been
//! consumed; "no scrollback" was pinned on the visible grid, which stays
//! empty of scrolled-off text either way. A pin has to assert the mechanism:
//! count the frames, count the history rows, name the API.
//!
//! Four of the items below are therefore **bounds** rather than absences,
//! which is what the replacements look like. For what 0.3 and 0.4 fixed, see
//! `docs/TERMLENS-COVERAGE.md`.

mod common;

use std::time::Duration;

use common::{spawn, spawn_sh};
use termlens::{Error, Key, Terminal};

// ------------------------------------------------ 1. one frame, not a history

/// The retained history is **8 frames**. A longer burst drops its oldest,
/// and nothing can reach back for them.
///
/// The 0.2 pin here (`only_the_newest_frame_of_a_burst_survives`) waited for
/// the last frame and then showed the first was unreachable — which is also
/// what happens once the first has simply been *consumed*, so it kept
/// passing against a version that retains eight. This one counts.
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
    // ring, so it consumes nothing.
    t.wait_until(|s| s.contains("FRAME-12"))?;

    // 05 is the oldest of the newest eight, so it is still observable…
    t.wait_frame(|s| s.contains("FRAME-05"))?;
    // …and 01 is four frames past the bound.
    let dropped = t.wait_frame(|s| s.contains("FRAME-01"));
    assert!(
        matches!(dropped, Err(Error::Timeout { .. })),
        "expected the oldest frames to be past the bound, got {dropped:?}"
    );
    Ok(())
}

/// The discipline that came with the history: each call scans retained
/// frames **oldest first**, so a predicate true of more than one of them
/// resolves on the *older* — which may not be the state you meant. The
/// remedy is the one `wait_until` already demands: name something only the
/// frame you want can show.
#[test]
fn a_predicate_true_of_two_retained_frames_matches_the_older() -> termlens::Result<()> {
    let mut t = spawn_sh(
        concat!(
            r"printf '\033[?2026h\033[2J\033[HCOMMON first\033[?2026l",
            r"\033[?2026h\033[2J\033[HCOMMON SECOND\033[?2026l'; read x"
        ),
        Duration::from_secs(2),
    );
    t.wait_until(|s| s.contains("SECOND"))?;

    let frame = t.wait_frame(|s| s.contains("COMMON"))?;
    assert!(
        frame.contains("first") && !frame.contains("SECOND"),
        "the older matching frame is the one returned:\n{frame}"
    );

    let frame = t.wait_frame(|s| s.contains("SECOND"))?;
    assert!(frame.contains("SECOND"));
    Ok(())
}

/// Documented in 0.4 rather than changed, and deliberately: `screen()`
/// returns the live grid, which can be half-painted even for an application
/// that brackets every repaint correctly. `wait_frame` is the only
/// frame-gated observation.
///
/// Substituting the newest complete frame would be worse — a `wait_until`
/// predicate could then match content the following `screen()` does not
/// show — and a torn read is what diagnoses an application hung mid-repaint.
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

    // `wait_idle` refuses to call this idle, which is what makes the
    // documented "settle before a whole-screen snapshot" recipe work.
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

// ------------------------------------------------- 3. the mouse API (resolved)
//
// 0.2's "the mouse API is one button" is gone: 0.3 added `click_with`,
// `drag`, modifier chords and the horizontal wheel. The pin that lived here
// sent `\x1b[<2;11;7M` by hand and asserted the app responded — which stayed
// true, because nothing removed the ability to send raw bytes, so it could
// never have failed when the gap closed.
//
// Coverage moved to `tui::right_click_clears_the_filter` (the real API against
// the real binding) and `tui::drag_modifiers_and_the_horizontal_wheel_are_encoded`
// (the exact bytes on the wire, for the gestures taskboard has no binding for).

// ------------------------------- 4. queries it recognises but cannot answer

/// 0.3/0.4 answer DSR, DA1/DA2, text-area size, OSC 10/11 and **`DECRQM`** —
/// the last of which is why a probe-then-enable application now works
/// unmodified (`hard::a_probing_application_is_told_that_synchronized_output_works`).
/// A few are still recognised but unanswerable: the kitty keyboard protocol
/// probe (`CSI ? u`), DA3, OSC 12 (cursor colour), `XTGETTCAP`, and `OSC 52`
/// clipboard *reads*. An app that *blocks* on one of those still hangs.
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
        .args([
            "-c",
            r#"printf '\033[6n'; read -r r; printf 'got\n'; read x"#,
        ])
        .spawn("/bin/sh")
        .expect("spawn");

    let err = t.wait_until(|s| s.contains("got")).expect_err("no answer");
    assert!(err.to_string().contains("^[[6n"), "{err}");
}

// -------------------------------- 5. scrollback: bounded, unreflowed, text only

/// Scrollback exists as of 0.4, so 0.2's
/// `output_scrolled_off_the_top_is_unrecoverable` is gone — and it is worth
/// noting *why* it kept passing against a version that retains history: it
/// only asserted on the visible grid, which holds no scrolled-off text either
/// way. Three bounds replace it, each counted rather than inferred.
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
    assert!(screen.full_text().contains("line 60"));
    Ok(())
}

/// History is **text only**: a scrolled-off row has no `Style` and no cell
/// addressing, so a style regression above the fold is not assertable.
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

    assert!(
        screen.full_text().contains("STYLED-AWAY"),
        "the text survives"
    );
    assert!(!screen.contains("STYLED-AWAY"), "it is off the grid");
    assert!(
        screen.find("STYLED-AWAY").is_none(),
        "so it has no coordinates"
    );
    assert!(
        !screen.with_styles().to_string().contains("fg=1"),
        "and no span describes it:\n{}",
        screen.with_styles()
    );
    Ok(())
}

/// A resize does not reflow history: rows keep the width they were captured
/// at, so narrowing does not retroactively rewrap them.
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

/// Not a defect, and the reason none of the rest of this suite changed when
/// retention was switched on by default: an application on the **alternate
/// screen** accumulates no history, because that is what a real terminal
/// does. taskboard repaints continuously and scrolls nothing into history.
#[test]
fn an_alternate_screen_app_has_no_scrollback() -> termlens::Result<()> {
    let mut t = spawn();
    for _ in 0..14 {
        t.send(Key::Char('j'));
    }
    t.wait_until(|s| s.contains("Tasks 13/13"))?;

    let screen = t.screen();
    assert_eq!(screen.scrollback_rows(), 0);
    assert_eq!(screen.full_text(), screen.text());
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

// ---------------------------------------- 7. every wait takes its own deadline

/// 0.3 gave `wait_frame`, `wait_idle` and `wait_exit` the per-call deadline
/// that only `wait_until` had. The 0.2 pin demonstrated the gap through
/// `wait_idle`, which honours the *builder* timeout either way — so it kept
/// passing once the overrides existed. This asserts the overrides.
#[test]
fn every_wait_takes_a_per_call_deadline() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        // Deliberately far too short for any of the waits below.
        .timeout(Duration::from_millis(150))
        .args([
            "-c",
            r"sleep 0.5; printf '\033[?2026hlate frame\033[?2026l'; sleep 0.3; exit 7",
        ])
        .spawn("/bin/sh")?;

    let frame = t.wait_frame_for(|s| s.contains("late frame"), Duration::from_secs(5))?;
    assert!(frame.contains("late frame"));
    t.wait_idle_for(Duration::from_millis(50), Duration::from_secs(5))?;
    assert_eq!(t.wait_exit_for(Duration::from_secs(5))?.code(), 7);

    // The builder value still applies when no override is given.
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_millis(150))
        .args(["-c", "sleep 0.6; printf 'late\\n'; read x"])
        .spawn("/bin/sh")?;
    let err = t
        .wait_until(|s| s.contains("late"))
        .expect_err("150ms deadline");
    assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    Ok(())
}
