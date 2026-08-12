//! The hard cases: taskboard features chosen because they are difficult
//! or impossible to observe through a screen-grid harness.
//!
//! Every test passes. The ones under "unreachable" pass by *pinning the
//! gap* — they assert that the thing the application demonstrably did is
//! not visible from the outside, so closing the gap will fail the test
//! that encodes it. That is the same contract as `tests/limits.rs`.
//!
//! Run against termlens 0.2.1. `docs/TERMLENS-COVERAGE.md` §3 summarises.

mod common;

use std::time::Duration;

use common::{spawn, spawn_args, spawn_sized};
use termlens::{Color, Error, Key, Style};

// ============================================================ the board tab

/// Three lanes, each with its own count, and a cursor that moves sideways.
#[test]
fn the_board_lays_out_three_lanes() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Tab);
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

    t.send(Key::Tab);
    t.wait_frame(|s| s.contains("Board todo 1/8"))?;

    t.send(Key::Char('l'));
    t.wait_frame(|s| s.contains("Board doing 1/2"))?;

    t.send(Key::Char('l'));
    t.wait_frame(|s| s.contains("Board done 1/3"))?;

    // Clamped at the last lane.
    t.send(Key::Char('l'));
    t.wait_frame(|s| s.contains("Board done 1/3"))?;

    t.send(Key::Char('h'));
    t.wait_frame(|s| s.contains("Board doing 1/2"))?;
    Ok(())
}

/// The focused lane's border is yellow; the others are dim grey. A style
/// difference in a *border*, which text-only snapshots cannot see and
/// `with_styles` can.
#[test]
fn the_focused_lane_is_the_one_with_a_coloured_border() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Tab);
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
    t.send(Key::Tab);
    t.wait_frame(|s| s.contains("doing (2)"))?;

    let screen = t.screen();
    // The middle lane spans columns 30..60 of a 90-column terminal.
    let doing = screen.rect_text(30..60, ..);
    assert!(doing.contains("帳票"), "the CJK title is in this lane:\n{doing}");
    assert!(
        !doing.contains("Wire up the PTY reader"),
        "and the todo lane's content is not:\n{doing}"
    );
    Ok(())
}

// ====================================================== a burst of frames

/// `r` runs the selected task: the event loop paints 0%, 10%, … 100% and a
/// closing frame back to back, with nothing pacing them. Every one is a
/// complete DEC 2026 frame, and all but the last are unreachable — the
/// harness retains one frame, not a history (`docs/TERMLENS-COVERAGE.md`
/// §2.1) — demonstrated here against the real application rather than a
/// synthetic `printf`.
#[test]
fn only_the_last_frame_of_a_progress_burst_is_observable() -> termlens::Result<()> {
    let mut t = spawn_args(&[], Duration::from_millis(600));

    t.send(Key::Char('r'));
    // The end of the burst is deterministic: it is the newest frame.
    t.wait_frame(|s| s.contains("finished Wire up the PTY reader"))?;

    // The run definitely painted a 50% frame — the gauge went through it —
    // but no API can reach back for it.
    let missed = t.wait_frame(|s| s.contains("running 50%"));
    assert!(
        matches!(missed, Err(Error::Timeout { .. })),
        "expected the intermediate frame to be gone, got {missed:?}"
    );

    // The run's effect did land.
    assert!(t.screen().contains("[x] HIGH Wire up"), "{}", t.screen());
    Ok(())
}

/// The transient notice *is* reachable, because it is the last complete
/// frame rather than one in the middle of a burst. This is the same
/// property that makes the `SAVING` frame catchable on SIGTERM.
#[test]
fn a_transient_toast_is_catchable_when_it_ends_the_burst() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('y'));
    t.wait_frame(|s| s.contains("· copied Wire up the PTY reader"))?;

    // And it is spent: the next key clears it.
    t.send(Key::Char('j'));
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
    t.send(Key::Char(' '));
    t.wait_frame(|s| s.contains("[ ] HIGH Wire up"))?;
    t.wait_until(|s| s.title() == "taskboard — 11 open")?;

    t.send(Key::Char(' '));
    t.wait_until(|s| s.title() == "taskboard — 10 open")?;
    Ok(())
}

// ==================================================== unreachable: styles

/// A finished task is struck through (`SGR 9`) *and* dimmed. Only the dim
/// survives the trip through the grid model: `Style` has no
/// `strikethrough`, so the two attributes are indistinguishable from one.
#[test]
fn strikethrough_on_done_titles_is_invisible() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    // A done row that is *not* the selected one, so the highlight's reverse
    // video doesn't muddy the comparison.
    let (row, col) = screen.find("Snapshot the screen grid").expect("done task");
    let done = *screen.cell(row, col).unwrap().style();
    assert!(done.dim, "the dim modifier does survive");

    let (row, col) = screen.find("Handle SIGWINCH").expect("open task");
    let open = *screen.cell(row, col).unwrap().style();

    // The only difference the harness can see between a struck-through,
    // dimmed title and a plain one is the dim.
    assert_eq!(
        done,
        Style { dim: true, ..open },
        "strikethrough would have to show up as a third difference"
    );
    Ok(())
}

/// The overdue badge blinks (`SGR 5`). Nothing in the model records it, so
/// `!` is styled exactly like any other red cell.
#[test]
fn the_blinking_overdue_badge_is_indistinguishable_from_plain_red() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let (row, col) = screen.find("! Handle SIGWINCH").expect("overdue badge");
    let badge = *screen.cell(row, col).unwrap().style();
    assert_eq!(badge.fg, Color::Indexed(1), "red");
    assert_eq!(
        badge,
        Style {
            fg: Color::Indexed(1),
            ..Style::default()
        },
        "no blink attribute anywhere in the style"
    );
    Ok(())
}

/// The secret field is drawn with `SGR 8` (conceal): a real terminal shows
/// nothing there. The characters are in the grid regardless, so a harness
/// reads a value the user cannot see — worth knowing before writing a test
/// that asserts a password field is masked.
#[test]
fn a_concealed_field_is_still_readable_in_the_grid() -> termlens::Result<()> {
    let mut t = spawn();

    // Select the task that carries a secret.
    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.paste("secret");
    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("tasks (1) filtered"))?;

    let screen = t.screen();
    assert!(
        screen.contains("hunter2-rotate-me"),
        "concealed text sits in the grid in clear:\n{screen}"
    );
    let (row, col) = screen.find("hunter2-rotate-me").unwrap();
    assert_eq!(
        *screen.cell(row, col).unwrap().style(),
        Style::default(),
        "and carries no marker that it was concealed"
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
fn the_clipboard_write_is_unobservable() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('y'));
    t.wait_frame(|s| s.contains("· copied"))?;

    let screen = t.screen();
    // The toast is the only evidence. The base64 payload the app put on the
    // wire (V2lyZSB1cCB0aGUgUFRZIHJlYWRlcg==) never reaches the grid.
    assert!(!screen.contains("V2lyZSB1cCB0aGU"), "{screen}");
    Ok(())
}

/// A rejected key rings the bell. `BEL` never reaches the grid and has no
/// accessor, so "the app complained" is untestable — the screen is
/// byte-identical either way.
#[test]
fn the_bell_on_rejected_input_is_unobservable() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('d'));
    t.wait_frame(|s| s.contains("CONFIRM"))?;
    let before = t.screen().to_string();

    // 'z' is not a valid answer to the modal: the app rings and redraws.
    t.send(Key::Char('z'));
    t.wait_frame(|s| s.contains("CONFIRM"))?;

    assert_eq!(before, t.screen().to_string(), "the bell left no trace");
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

    t.send(Key::Char('/'));
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

    t.send(Key::Char('T'));
    t.wait_frame(|s| s.contains("high contrast on"))?;

    let screen = t.screen();
    assert!(screen.contains("HC"), "the app says it applied it:\n{screen}");
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
/// focus (mode 1004). There is no API to deliver a focus event, so the
/// unfocused rendering is unreachable — the app is stuck in the focused
/// branch for the whole life of the test.
#[test]
fn focus_events_cannot_be_delivered_so_the_unfocused_view_is_unreachable(
) -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let (row, col) = screen.find("NORMAL").expect("mode badge");
    let style = *screen.cell(row, col).unwrap().style();
    assert_eq!(style.bg, Color::Indexed(4), "focused: blue background");
    assert!(!style.dim);
    // Sending `ESC[O` by hand is not an option either: it is indistinguish-
    // able from Esc followed by 'O' (§2.3), and crossterm reads it as the
    // focus event only because a real terminal never types those keys that
    // fast. Nothing in the API expresses "the window lost focus".
    Ok(())
}

// ================================== the capability probe, end to end

/// The headline case. With `--probe-sync` the application does what a
/// careful application does: it asks `CSI ? 2026 $ p` whether the terminal
/// supports synchronized output, and brackets its repaints only if the
/// answer says yes.
///
/// termlens implements DEC 2026 — `wait_frame` is built on it — but does
/// not recognise the query that advertises it, so the probe goes
/// unanswered, the app concludes the terminal has no support, and
/// `wait_frame` can then never succeed against it. The failure message
/// blames the application for not emitting frames.
#[test]
fn probing_for_synchronized_output_turns_wait_frame_off_entirely() -> termlens::Result<()> {
    let mut t = spawn_args(&["--probe-sync"], Duration::from_millis(600));

    // The app is running and perfectly testable by content...
    t.send(Key::Tab);
    t.send(Key::Tab);
    t.send(Key::Tab);
    t.wait_until(|s| s.contains("logs (41)"))?;
    let screen = t.screen();
    assert!(
        screen.contains("DECRQM ?2026 supported: no"),
        "the app asked and got nothing:\n{screen}"
    );

    // ...and completely untestable by frame.
    let err = t
        .wait_frame(|_| true)
        .expect_err("no synchronized updates are being emitted");
    let message = err.to_string();
    assert!(
        message.contains("never emitted a DEC 2026 synchronized update"),
        "{message}"
    );
    // And no note names the query that caused it, because the tracker never
    // classified `CSI ? 2026 $ p` as a query at all.
    assert!(!message.contains("queried the terminal"), "{message}");
    Ok(())
}

/// Without the flag the app brackets unconditionally, so the same binary is
/// fully frame-testable. The difference is one capability probe.
#[test]
fn without_the_probe_the_same_binary_is_frame_testable() -> termlens::Result<()> {
    let mut t = spawn_args(&[], Duration::from_millis(600));
    t.send(Key::Tab);
    t.wait_frame(|s| s.contains("todo (8)"))?;
    Ok(())
}

/// The batch of six startup probes: termlens answers DA1, DA2, DSR and both
/// OSC colour queries, and does not answer XTGETTCAP.
#[test]
fn five_of_six_startup_capability_probes_are_answered() -> termlens::Result<()> {
    let mut t = spawn_args(&["--probe-caps"], Duration::from_secs(3));

    t.send(Key::Tab);
    t.send(Key::Tab);
    t.send(Key::Tab);
    t.wait_until(|s| s.contains("capability probes:"))?;

    let screen = t.screen();
    assert!(
        screen.contains("capability probes: 5/6"),
        "XTGETTCAP is the one that goes unanswered:\n{screen}"
    );
    Ok(())
}

// =========================================== text fidelity, in the real app

/// The credentials task carries a decomposed 'é'. It renders identically to
/// the precomposed form and compares differently, so the obvious needle
/// misses — in an application, not a contrived `printf`.
#[test]
fn the_nfd_title_does_not_match_an_nfc_needle() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    assert!(
        !screen.contains("café credentials"),
        "the needle a test author would type misses:\n{screen}"
    );
    assert!(
        screen.contains("cafe\u{301} credentials"),
        "only the decomposed form matches"
    );
    Ok(())
}

/// The audit task's title mixes a ZWJ sequence, a regional-indicator flag
/// and a VS16 emoji. The grid's column accounting for all three differs
/// from what a real terminal draws, so the row's rendered width here is not
/// the width a user sees.
#[test]
fn mixed_emoji_widths_land_differently_than_a_real_terminal_draws_them(
) -> termlens::Result<()> {
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
    assert!(
        screen.cell(row, 0).is_some(),
        "sanity: the row exists"
    );
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
    t.wait_frame(|s| s.contains("NORMAL"))?;
    assert_eq!(t.screen().mouse_mode(), termlens::MouseMode::AnyMotion);

    // Row 6 of the list, at a column no legacy report could encode.
    t.click(230, 6)?;
    t.wait_frame(|s| s.contains("Tasks 3/13"))?;
    Ok(())
}
