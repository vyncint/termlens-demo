//! The hard cases: taskboard features chosen because they are difficult
//! or impossible to observe through a screen-grid harness.
//!
//! Every test passes. The ones still under "unreachable" pass by *pinning
//! the gap* — they assert that the thing the application demonstrably did is
//! not visible from the outside, so closing the gap will fail the test that
//! encodes it. That is the same contract as `tests/limits.rs`.
//!
//! Six of the pins here are gone as of 0.4, moved into "covered": the three
//! style attributes, the clipboard payload, the progress burst, and the
//! `DECRQM` capability probe that used to turn `wait_frame` off entirely.
//! One caveat learned while upgrading: a pin only earns its keep if it
//! asserts the *mechanism*. `the_clipboard_write_is_unobservable` kept
//! passing against 0.4 because all it ever checked was that base64 stays off
//! the grid — still true, and never the claim its name made.
//!
//! Run against termlens 0.4.0. `docs/TERMLENS-COVERAGE.md` §3 summarises.

mod common;

use std::time::Duration;

use common::{spawn, spawn_args, spawn_sized};
use termlens::{Color, Error, Key, Screen, Style};

// ============================================================ the board tab

/// Three lanes, each with its own count, and a cursor that moves sideways.
#[test]
fn the_board_lays_out_three_lanes() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Tab)?;
    t.wait_frame(|s| s.contains("todo (") && s.contains("doing (") && s.contains("done ("))?;

    let screen = t.screen();
    assert!(screen.contains("todo (8)"), "{screen}");
    assert!(screen.contains("doing (2)"), "{screen}");
    assert!(screen.contains("done (3)"), "{screen}");
    assert!(screen.contains("Board todo 1/8"), "{screen}");
    Ok(())
}

#[test]
fn h_and_l_move_between_lanes() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Tab)?;
    t.wait_frame(|s| s.contains("Board todo 1/8"))?;

    t.send(Key::Char('l'))?;
    t.wait_frame(|s| s.contains("Board doing 1/2"))?;

    t.send(Key::Char('l'))?;
    t.wait_frame(|s| s.contains("Board done 1/3"))?;

    // Clamped at the last lane.
    t.send(Key::Char('l'))?;
    t.wait_frame(|s| s.contains("Board done 1/3"))?;

    t.send(Key::Char('h'))?;
    t.wait_frame(|s| s.contains("Board doing 1/2"))?;
    Ok(())
}

/// The focused lane's border is yellow; the others are dim grey. A style
/// difference in a *border*, which text-only snapshots cannot see and
/// `with_styles` can.
#[test]
fn the_focused_lane_is_the_one_with_a_coloured_border() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Tab)?;
    t.wait_frame(|s| s.contains("Board todo 1/8"))?;

    let screen = t.screen();
    let (row, col) = screen.find("todo (8)").expect("lane title");
    let focused = *screen.cell(row, col).unwrap().style();
    assert_eq!(focused.fg, Color::Indexed(3), "yellow while focused");

    let (row, col) = screen.find("doing (2)").expect("lane title");
    let unfocused = *screen.cell(row, col).unwrap().style();
    assert_eq!(unfocused.fg, Color::Indexed(8), "dim grey otherwise");
    Ok(())
}

/// Each lane is a third of the width, so long titles are cut — including
/// through a double-width glyph. `rect_text` is the tool for asserting on
/// one lane without the others bleeding in.
#[test]
fn lane_width_truncates_titles_including_wide_glyphs() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Tab)?;
    t.wait_frame(|s| s.contains("doing (2)"))?;

    let screen = t.screen();
    // The middle lane spans columns 30..60 of a 90-column terminal.
    let doing = screen.rect_text(30..60, ..);
    assert!(
        doing.contains("帳票"),
        "the CJK title is in this lane:\n{doing}"
    );
    assert!(
        !doing.contains("Wire up the PTY reader"),
        "and the todo lane's content is not:\n{doing}"
    );
    Ok(())
}

// ====================================================== a burst of frames

/// `r` runs the selected task: the event loop paints 0%, 10%, … 100% and a
/// closing frame back to back, with nothing pacing them — twelve complete
/// DEC 2026 frames in one burst, against the real application rather than a
/// synthetic `printf`.
///
/// Under 0.2 only the last was observable. As of 0.4 the newest **8** are
/// retained, so most of the gauge is assertable step by step — and the four
/// oldest are past the bound, which is the limitation that replaced the old
/// one. Both halves are asserted here, in that order: the bound first,
/// because asking for a retained frame advances the cursor past it.
#[test]
fn a_progress_burst_is_observable_up_to_the_retention_bound() -> termlens::Result<()> {
    let mut t = spawn_args(&[], Duration::from_secs(5));

    t.send(Key::Char('r'))?;
    // Settle on the live screen; `wait_until` reads the grid, not the frame
    // ring, so it consumes nothing.
    t.wait_until(|s| s.contains("finished Wire up the PTY reader"))?;

    // 12 frames, 8 retained: 0%–30% are gone.
    let dropped = t.wait_frame_for(|s| s.contains("running 0%"), Duration::from_millis(500));
    assert!(
        matches!(dropped, Err(Error::Timeout { .. })),
        "the start of the burst is past the retention bound, got {dropped:?}"
    );

    // …and 40% onwards is there, in the order the gauge drew it.
    for pct in [40, 50, 60, 70, 80, 90, 100] {
        let frame = t.wait_frame(|s| s.contains(&format!("running {pct}%")))?;
        assert!(frame.contains(&format!("running {pct}%")), "{frame}");
    }
    let frame = t.wait_frame(|s| s.contains("finished Wire up the PTY reader"))?;
    assert!(frame.contains("[x] HIGH Wire up"), "{frame}");
    Ok(())
}

/// The transient notice *is* reachable, because it is the last complete
/// frame rather than one in the middle of a burst. This is the same
/// property that makes the `SAVING` frame catchable on SIGTERM.
#[test]
fn a_transient_toast_is_catchable_when_it_ends_the_burst() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('y'))?;
    t.wait_frame(|s| s.contains("· copied Wire up the PTY reader"))?;

    // And it is spent: the next key clears it.
    t.send(Key::Char('j'))?;
    t.wait_frame(|s| s.contains("Tasks 2/13"))?;
    assert!(!t.screen().contains("· copied"), "{}", t.screen());
    Ok(())
}

// ================================================= state around the grid

/// The window title now tracks state rather than being set once, so it is
/// an assertion target that changes as the app runs.
#[test]
fn the_window_title_follows_the_open_count() -> termlens::Result<()> {
    let mut t = spawn();
    assert_eq!(t.screen().title(), "taskboard — 10 open");

    // Space toggles the first task (done -> open): one more open task.
    t.send(Key::Char(' '))?;
    t.wait_frame(|s| s.contains("[ ] HIGH Wire up"))?;
    t.wait_until(|s| s.title() == "taskboard — 11 open")?;

    t.send(Key::Char(' '))?;
    t.wait_until(|s| s.title() == "taskboard — 10 open")?;
    Ok(())
}

// ======================================================= covered: styles

/// A finished task is struck through (`SGR 9`) *and* dimmed. Both survive
/// the trip through the grid model as of 0.4; under 0.2 only the dim did, so
/// the two attributes were indistinguishable from one and this test pinned
/// that as a gap.
#[test]
fn done_titles_are_struck_through_as_well_as_dimmed() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    // A done row that is *not* the selected one, so the highlight's reverse
    // video doesn't muddy the comparison.
    let (row, col) = screen.find("Snapshot the screen grid").expect("done task");
    let done = *screen.cell(row, col).unwrap().style();

    let (row, col) = screen.find("Handle SIGWINCH").expect("open task");
    let open = *screen.cell(row, col).unwrap().style();

    // Two differences now, where 0.2 could see only one.
    assert!(done.dim && done.strikethrough, "done title: {done:?}");
    assert!(!open.dim && !open.strikethrough, "open title: {open:?}");
    assert_eq!(
        done,
        Style {
            dim: true,
            strikethrough: true,
            ..open
        },
        "and nothing else about the two differs"
    );
    Ok(())
}

/// The overdue badge blinks (`SGR 5`). Under 0.2 nothing in the model
/// recorded it, so `!` was styled exactly like any other red cell — a
/// blinking warning and a static one were the same value.
#[test]
fn the_overdue_badge_blinks_and_plain_red_does_not() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let (row, col) = screen.find("! Handle SIGWINCH").expect("overdue badge");
    let badge = *screen.cell(row, col).unwrap().style();
    assert_eq!(
        badge,
        Style {
            fg: Color::Indexed(1),
            blink: true,
            ..Style::default()
        },
        "the badge is blinking red"
    );

    // `find_by` reaches it directly now, which is the assertion an author
    // actually wants: "is anything on screen demanding attention?"
    assert_eq!(
        screen.find_by(|c| c.style().blink),
        Some((row, col)),
        "the badge is the only blinking cell"
    );
    Ok(())
}

/// The secret field is drawn with `SGR 8` (conceal): a real terminal shows
/// nothing there. The characters are in the grid regardless — that part is
/// correct, and unchanged — but as of 0.4 they carry a marker saying so.
///
/// This is the sharpest of the three style gaps, because before the marker
/// existed a test asserting "the secret is masked" **passed against an
/// application that printed it in the clear**. The two renderings were the
/// same value.
#[test]
fn a_concealed_field_is_marked_concealed() -> termlens::Result<()> {
    let mut t = spawn();

    // Select the task that carries a secret.
    t.send(Key::Char('/'))?;
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.paste("secret")?;
    t.send(Key::Enter)?;
    let screen = t.wait_frame(|s| s.contains("tasks (1) filtered"))?;

    // Still in the grid, exactly as a real terminal holds it…
    assert!(
        screen.contains("hunter2-rotate-me"),
        "concealed text is still in the grid:\n{screen}"
    );
    // …and now distinguishable from text that was never concealed.
    let (row, col) = screen.find("hunter2-rotate-me").unwrap();
    let secret = *screen.cell(row, col).unwrap().style();
    assert!(secret.conceal, "the secret is marked concealed: {secret:?}");
    assert!(
        (0..screen.cols()).any(|c| screen
            .cell(row, c)
            .is_some_and(|cell| !cell.style().conceal && !cell.contents().trim().is_empty())),
        "and its row also holds unconcealed text, so this is not a blanket flag"
    );

    // The assertion a test author actually writes: every cell of the secret
    // is masked, and the label next to it is not.
    let masked: String = (col..col + 17)
        .filter(|&c| screen.cell(row, c).is_some_and(|cell| cell.style().conceal))
        .count()
        .to_string();
    assert_eq!(
        masked,
        "17",
        "all of it, not just the first cell:\n{}",
        screen.with_styles()
    );
    Ok(())
}

// ============================================ unreachable: outside the grid

/// The detail pane's `open ref` label is an `OSC 8` hyperlink. The label is
/// on the grid; the target is nowhere — not in the text, not in the title,
/// not in any accessor.
#[test]
fn the_hyperlink_target_is_unobservable() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    assert!(screen.contains("link     open ref"), "{screen}");
    assert!(
        !screen.contains("example.invalid"),
        "the URL is not on the grid:\n{screen}"
    );
    assert!(!screen.title().contains("example.invalid"));
    Ok(())
}

/// `y` writes the selected title to the system clipboard with `OSC 52`. The
/// application's own UI proves the code path ran; the write itself is
/// invisible, so "did it copy the right thing?" cannot be asserted.
#[test]
fn the_clipboard_payload_is_observable() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('y'))?;
    let screen = t.wait_frame(|s| s.contains("· copied"))?;

    // Under 0.2 the toast was the only evidence, and this test asserted only
    // that the base64 stays off the grid — which is still true, and was never
    // the claim its name made. The payload itself is now readable, so the
    // question "did it copy the *right* thing?" is answerable.
    assert!(!screen.contains("V2lyZSB1cCB0aGU"), "{screen}");

    let clip = screen.clipboard().expect("the OSC 52 write was captured");
    assert_eq!(clip.text(), Some("Wire up the PTY reader"));
    assert_eq!(clip.targets(), "c", "the clipboard selection, not primary");

    // It tracks the selection, so it is the real payload rather than a
    // constant that happens to match.
    t.send(Key::Char('j'))?;
    t.wait_frame(|s| s.contains("Tasks 2/13"))?;
    t.send(Key::Char('y'))?;
    let screen = t.wait_frame(|s| s.contains("· copied Snapshot"))?;
    assert_eq!(
        screen.clipboard().and_then(|c| c.text()),
        Some("Snapshot the screen grid")
    );
    Ok(())
}

/// A rejected key rings the bell. `BEL` never reaches the grid, so the
/// screen stays byte-identical — which is precisely why "an invalid key
/// does nothing" and "an invalid key is refused with a bell" used to be the
/// same test. **0.5 counts bells**, so the two are now different claims
/// about the same unchanged screen.
///
/// This test was green before that change and its claim was false anyway:
/// it asserted the symptom (the grid is untouched) rather than the absence
/// it was named for. Kept in the stronger form — both halves, together.
#[test]
fn the_bell_on_rejected_input_is_counted_though_the_grid_is_untouched() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('d'))?;
    t.wait_frame(|s| s.contains("CONFIRM"))?;
    let before = t.screen();
    let rung_before = before.bells();
    let text_before = before.to_string();

    // 'z' is not a valid answer to the modal: the app rings and redraws.
    t.send(Key::Char('z'))?;
    t.wait_frame(|s| s.contains("CONFIRM"))?;
    let after = t.screen();

    assert_eq!(text_before, after.to_string(), "the bell left no trace");
    assert_eq!(
        after.bells(),
        rung_before + 1,
        "and yet the complaint is observable: {} -> {}",
        rung_before,
        after.bells()
    );
    Ok(())
}

/// The app switches the cursor to a bar (`DECSCUSR 5`) while typing and
/// back to a block otherwise. The cursor's *position* and *visibility* are
/// modelled; its shape is not.
#[test]
fn the_cursor_shape_change_is_unobservable() -> termlens::Result<()> {
    let mut t = spawn();
    let (_, _, visible_before) = t.screen().cursor();
    assert!(!visible_before, "hidden in normal mode");

    t.send(Key::Char('/'))?;
    t.wait_frame(|s| s.contains("FILTER"))?;

    let (row, col, visible) = t.screen().cursor();
    assert!(visible, "the input shows a cursor");
    assert_eq!((row, col), (24, 1), "and it is where the caret belongs");
    // Whether it is a bar or a block — the thing the app just changed — is
    // not part of the snapshot, so nothing here can assert it.
    Ok(())
}

/// `T` redefines palette slots 1-6 with `OSC 4`. Every styled cell keeps
/// reporting its palette *index*, so a colour assertion is against the
/// default palette rather than what the terminal would now paint.
#[test]
fn a_palette_override_does_not_change_what_the_grid_reports() -> termlens::Result<()> {
    let mut t = spawn();

    let (row, col) = t.screen().find("HIGH ! Handle").expect("high priority");
    let before = *t.screen().cell(row, col).unwrap().style();
    assert_eq!(before.fg, Color::Indexed(1));

    t.send(Key::Char('T'))?;
    t.wait_frame(|s| s.contains("high contrast on"))?;

    let screen = t.screen();
    assert!(
        screen.contains("HC"),
        "the app says it applied it:\n{screen}"
    );
    let (row, col) = screen.find("HIGH ! Handle").expect("high priority");
    assert_eq!(
        *screen.cell(row, col).unwrap().style(),
        before,
        "slot 1 now renders as ff0040, and the grid still just says index 1"
    );
    Ok(())
}

// ======================================== unreachable: input the app wants

/// The app dims its status bar when the terminal reports the window lost
/// focus (mode 1004). Until 0.5 there was no API to deliver a focus event,
/// so the unfocused rendering was not merely unasserted but **unreachable**:
/// the branch never ran, in any test, ever.
///
/// The 0.4 version of this test asserted the focused styling and stopped —
/// so it stayed green when the branch became reachable, proving nothing
/// about the thing it was named for. This one crosses the boundary in both
/// directions, which is the only shape that can fail if focus stops working.
#[test]
fn the_unfocused_view_is_reachable_and_returns() -> termlens::Result<()> {
    let mut t = spawn();

    let badge = |s: &Screen| -> Style {
        let (row, col) = s.find("NORMAL").expect("mode badge");
        *s.cell(row, col).unwrap().style()
    };

    let opening = t.screen();
    assert!(
        opening.focus_events(),
        "the app asked for mode 1004, which is what makes the rest deliverable"
    );
    let focused = badge(&opening);
    assert_eq!(focused.bg, Color::Indexed(4), "focused: blue background");
    assert!(!focused.dim);

    t.focus_out()?;
    let unfocused = t.wait_frame(|s| badge(s).dim)?;
    assert!(badge(&unfocused).dim, "the branch that never used to run");

    t.focus_in()?;
    let refocused = t.wait_frame(|s| !badge(s).dim)?;
    assert_eq!(badge(&refocused).bg, Color::Indexed(4), "and back again");
    Ok(())
}

// ================================== the capability probe, end to end

/// The headline case, and the single highest-leverage change across these
/// three releases. With `--probe-sync` the application does what a careful
/// application does: it asks `CSI ? 2026 $ p` whether the terminal supports
/// synchronized output, and brackets its repaints only if the answer says
/// yes.
///
/// Under 0.2 termlens implemented DEC 2026 — `wait_frame` is built on it —
/// but did not recognise the query that advertises it. So the probe went
/// unanswered, the app concluded the terminal had no support, and
/// `wait_frame` could then never succeed against it, with a failure message
/// that blamed the application for not emitting frames. This test pinned
/// that as a gap.
///
/// 0.3 answers `DECRQM`. The same unmodified binary is now fully
/// frame-testable, which is the difference between a harness you write
/// subjects for and one you point at real programs.
#[test]
fn a_probing_application_is_told_that_synchronized_output_works() -> termlens::Result<()> {
    let mut t = spawn_args(&["--probe-sync"], Duration::from_secs(5));

    // What the application concluded from the answer it got.
    t.send(Key::Tab)?;
    t.send(Key::Tab)?;
    t.send(Key::Tab)?;
    t.wait_until(|s| s.contains("logs (41)"))?;
    let screen = t.screen();
    assert!(
        screen.contains("DECRQM ?2026 supported: yes"),
        "the app asked and was told yes:\n{screen}"
    );

    // And because it believed the answer, it brackets its repaints — so the
    // frame path works, on a binary that was never modified for the harness.
    t.send(Key::Tab)?;
    let frame = t.wait_frame(|s| s.contains("tasks (13)"))?;
    assert!(frame.contains("NORMAL"), "a complete frame:\n{frame}");
    Ok(())
}

/// Without the flag the app brackets unconditionally, so the same binary is
/// fully frame-testable. The difference is one capability probe.
#[test]
fn without_the_probe_the_same_binary_is_frame_testable() -> termlens::Result<()> {
    let mut t = spawn_args(&[], Duration::from_millis(600));
    t.send(Key::Tab)?;
    t.wait_frame(|s| s.contains("todo (8)"))?;
    Ok(())
}

/// The batch of six startup probes: termlens answers DA1, DA2, DSR, both
/// OSC colour queries — and, since 0.5, `XTGETTCAP`, which had been the last
/// common startup probe with no reply. An application that asks all six and
/// waits for all six now starts unmodified.
#[test]
fn all_six_startup_capability_probes_are_answered() -> termlens::Result<()> {
    let mut t = spawn_args(&["--probe-caps"], Duration::from_secs(3));

    t.send(Key::Tab)?;
    t.send(Key::Tab)?;
    t.send(Key::Tab)?;
    t.wait_until(|s| s.contains("capability probes:"))?;

    let screen = t.screen();
    assert!(
        screen.contains("capability probes: 6/6"),
        "XTGETTCAP was the one that used to go unanswered:\n{screen}"
    );
    Ok(())
}

// =========================================== text fidelity, in the real app

/// The credentials task carries a decomposed 'é'. It renders identically to
/// the precomposed form and used to compare differently, so the obvious
/// needle missed — in an application, not a contrived `printf`.
///
/// **0.5 folds both sides to NFC**, which is the fix that matters for a
/// human writing the test: the author types what their editor produces,
/// and the application holds whatever its data source handed it. Neither
/// form is privileged now, and both land on the same cell.
#[test]
fn either_normalization_of_the_title_matches() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    assert!(
        screen.contains("café credentials"),
        "the precomposed needle a test author would type:\n{screen}"
    );
    assert!(
        screen.contains("cafe\u{301} credentials"),
        "and the decomposed form the application actually holds"
    );
    assert_eq!(
        screen.find("café credentials"),
        screen.find("cafe\u{301} credentials"),
        "the same cell, whichever form is asked for"
    );
    Ok(())
}

/// The audit task's title mixes a ZWJ sequence, a regional-indicator flag
/// and a VS16 emoji. The grid's column accounting for all three differs
/// from what a real terminal draws, so the row's rendered width here is not
/// the width a user sees.
#[test]
fn mixed_emoji_widths_land_differently_than_a_real_terminal_draws_them() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let (row, _) = screen.find("glyph widths").expect("the audit task");
    let cells: Vec<(u16, String, bool)> = (0..30)
        .filter_map(|col| {
            let cell = screen.cell(row, col)?;
            (!cell.contents().is_empty())
                .then(|| (col, cell.contents().to_string(), cell.is_wide()))
        })
        .collect();
    println!("row {row}: {cells:?}");

    // The flag is two separate narrow cells rather than one wide glyph.
    assert!(screen.cell(row, 0).is_some(), "sanity: the row exists");
    assert!(screen.contains("🇻🇳"), "the flag is present as two cells");
    Ok(())
}

/// One task's title ends in three real spaces. Inside a bordered pane they
/// are followed by the list's own padding, so nothing can tell the app's
/// trailing spaces from the widget's — the identical assertion passes for a
/// title that has none. A trailing-whitespace regression in a padded pane
/// is therefore not assertable at all.
#[test]
fn trailing_space_in_a_title_is_indistinguishable_from_padding() -> termlens::Result<()> {
    let mut t = spawn_sized(46, 26); // narrow: the list runs full width

    t.wait_until(|s| s.contains("Trim trailing space"))?;
    let screen = t.screen();

    assert!(
        screen.contains("Trim trailing space   "),
        "the three real spaces are there:\n{screen}"
    );
    assert!(
        screen.contains("Windows ConPTY support   "),
        "and so are three padding spaces after a title that has none"
    );

    // Only counting cells to the pane border tells them apart, and that
    // means knowing the layout rather than asserting on the text.
    let (row, col) = screen.find("Trim trailing space").unwrap();
    let after = (col + 19..45)
        .map_while(|c| screen.cell(row, c))
        .take_while(|cell| cell.contents() == " " || cell.contents().is_empty())
        .count();
    println!("padding cells after the title: {after}");
    Ok(())
}

// ================================================= mouse at a wide terminal

/// crossterm asks for SGR encoding, so a click past the legacy 222-column
/// limit works. The same click against an application that had asked for
/// mode 1005 would be refused (see the 0.2.1 notes) even though that
/// encoding exists to carry it.
#[test]
fn clicking_past_column_222_works_under_sgr() -> termlens::Result<()> {
    let mut t = spawn_sized(240, 26);
    // No second `wait_frame(NORMAL)` here: `spawn_sized` already consumed
    // that frame, and as of 0.4 a frame satisfies exactly one wait. This is
    // the one migration cost of the frame cursor, and it fails loudly (a
    // full-timeout hang) rather than quietly.
    assert_eq!(t.screen().mouse_mode(), termlens::MouseMode::AnyMotion);

    // Row 6 of the list, at a column no legacy report could encode.
    t.click(230, 6)?;
    t.wait_frame(|s| s.contains("Tasks 3/13"))?;
    Ok(())
}
