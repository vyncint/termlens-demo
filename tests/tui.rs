//! What termlens covers well: everything the user can see and type.
//!
//! Each test drives the real binary through a real PTY and asserts on the
//! rendered grid — text, styles, cursor, size — or on the exit status.

mod common;

use common::{spawn, spawn_sized, style_at};
use termlens::{Color, Key};

// ---------------------------------------------------------------- navigation

#[test]
fn boots_into_the_tasks_tab_with_the_first_row_selected() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    assert!(screen.contains("tasks (10)"), "{screen}");
    assert!(screen.contains("Tasks 1/10"), "{screen}");
    // The detail pane follows the selection.
    assert!(screen.contains("Wire up the PTY reader"), "{screen}");
    Ok(())
}

#[test]
fn arrow_and_vim_keys_move_the_selection() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Down);
    t.wait_until(|s| s.contains("Tasks 2/10"))?;

    t.send(Key::Char('j'));
    t.wait_until(|s| s.contains("Tasks 3/10"))?;
    // The detail pane tracks the cursor.
    t.wait_until(|s| s.contains("priority med"))?;

    t.send(Key::Char('k'));
    t.wait_until(|s| s.contains("Tasks 2/10"))?;

    t.send(Key::Up);
    t.wait_until(|s| s.contains("Tasks 1/10"))?;
    Ok(())
}

#[test]
fn selection_clamps_at_both_ends() -> termlens::Result<()> {
    let mut t = spawn();

    // Already at the top; Up must not wrap or underflow.
    t.send(Key::Up);
    t.send(Key::Up);
    t.wait_until(|s| s.contains("Tasks 1/10"))?;

    t.send(Key::End);
    t.wait_until(|s| s.contains("Tasks 10/10"))?;
    t.send(Key::Down);
    t.wait_until(|s| s.contains("Tasks 10/10"))?;

    t.send(Key::Home);
    t.wait_until(|s| s.contains("Tasks 1/10"))?;
    Ok(())
}

#[test]
fn page_keys_move_by_a_screenful() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Tab);
    t.send(Key::Tab);
    t.wait_until(|s| s.contains("logs (40)"))?;

    // The list pane is 20 rows tall at 26 rows total.
    t.send(Key::PageDown);
    t.wait_until(|s| s.contains("Logs 21/40"))?;

    t.send(Key::PageUp);
    t.wait_until(|s| s.contains("Logs 1/40"))?;
    Ok(())
}

#[test]
fn tab_and_backtab_cycle_the_tabs() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Tab);
    t.wait_until(|s| s.contains("stats") && s.contains("total    10"))?;

    t.send(Key::Tab);
    t.wait_until(|s| s.contains("logs (40)"))?;

    // Wraps around to Tasks.
    t.send(Key::Tab);
    t.wait_until(|s| s.contains("tasks (10)"))?;

    t.send(Key::BackTab);
    t.wait_until(|s| s.contains("logs (40)"))?;
    Ok(())
}

// -------------------------------------------------------------------- filter

#[test]
fn filter_mode_shows_a_live_cursor_and_narrows_the_list() -> termlens::Result<()> {
    let mut t = spawn();

    // Normal mode hides the cursor.
    assert!(!t.screen().cursor().2, "cursor should start hidden");

    t.send(Key::Char('/'));
    t.wait_until(|s| s.contains("FILTER"))?;

    t.send_str("core");
    t.wait_until(|s| s.contains("/core"))?;

    // The cursor is visible and sits just past the typed text: the input
    // line is the second-to-last row, after "/" plus four characters. The
    // cursor is repositioned at the end of the frame, so it belongs in the
    // same predicate as the text rather than a follow-up read.
    t.wait_until(|s| s.cursor() == (s.rows() - 2, 5, true))?;

    t.send(Key::Enter);
    t.wait_until(|s| {
        s.contains("tasks (4) filtered")
            && s.contains("filter:core")
            && !s.contains("Add bracketed paste")
            && !s.cursor().2
    })?;
    Ok(())
}

#[test]
fn backspace_edits_the_draft_before_it_is_applied() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_until(|s| s.contains("FILTER"))?;
    t.send_str("docs");
    t.wait_until(|s| s.contains("/docs"))?;

    t.send(Key::Backspace);
    t.send(Key::Backspace);
    t.wait_until(|s| s.contains("/do") && !s.contains("/docs"))?;

    // Still a draft — the list is untouched until Enter.
    assert!(t.screen().contains("tasks (10)"), "{}", t.screen());

    // "do" matches the `docs` tag and the title "Windows ConPTY support".
    t.send(Key::Enter);
    t.wait_until(|s| s.contains("tasks (3) filtered") && s.contains("Document the wait"))?;
    Ok(())
}

#[test]
fn filter_matches_tags_as_well_as_titles() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_until(|s| s.contains("FILTER"))?;
    t.send_str("i18n");
    t.send(Key::Enter);
    t.wait_until(|s| {
        s.contains("tasks (2) filtered")
            && s.contains("帳票をレンダリングする")
            && s.contains("emoji width")
    })?;
    Ok(())
}

#[test]
fn esc_abandons_the_filter_draft() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_until(|s| s.contains("FILTER"))?;
    t.send_str("perf");
    t.wait_until(|s| s.contains("/perf"))?;

    t.send(Key::Esc);
    t.wait_until(|s| {
        s.contains("NORMAL") && s.contains("tasks (10)") && !s.contains("/perf")
    })?;
    Ok(())
}

#[test]
fn esc_in_normal_mode_clears_an_applied_filter() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_until(|s| s.contains("FILTER"))?;
    t.send_str("perf");
    t.send(Key::Enter);
    t.wait_until(|s| s.contains("filter:perf"))?;

    t.send(Key::Esc);
    // Both halves in ONE predicate. Splitting them — wait for the list, then
    // read the status bar — is a race: a `Screen` is a consistent instant,
    // but a *frame* is not atomic, and the app repaints the status bar
    // (last row) after the list (row 4). See docs/TERMLENS-COVERAGE.md §2.
    t.wait_until(|s| s.contains("tasks (10)") && !s.contains("filter:"))?;
    Ok(())
}

// --------------------------------------------------------------- modal + help

#[test]
fn confirm_dialog_deletes_the_selected_task() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('d'));
    t.wait_until(|s| s.contains("CONFIRM") && s.contains("delete this task?"))?;
    // The dialog names the task it is about to remove.
    assert!(t.screen().contains("[y] delete   [n] cancel"), "{}", t.screen());

    t.send(Key::Char('y'));
    t.wait_until(|s| {
        s.contains("tasks (9)")
            && s.contains("NORMAL")
            && !s.contains("Wire up the PTY reader")
    })?;
    Ok(())
}

#[test]
fn confirm_dialog_cancels_without_deleting() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('d'));
    t.wait_until(|s| s.contains("CONFIRM"))?;

    t.send(Key::Char('n'));
    t.wait_until(|s| {
        s.contains("NORMAL")
            && s.contains("tasks (10)")
            && s.contains("Wire up the PTY reader")
            && !s.contains("delete this task?")
    })?;
    Ok(())
}

#[test]
fn help_overlay_opens_from_question_mark_and_f1() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('?'));
    t.wait_until(|s| s.contains("HELP") && s.contains("move cursor"))?;

    t.send(Key::Esc);
    t.wait_until(|s| s.contains("NORMAL") && !s.contains("move cursor"))?;

    // Function keys reach the app too.
    t.send(Key::F(1));
    t.wait_until(|s| s.contains("HELP"))?;
    Ok(())
}

#[test]
fn space_toggles_the_done_marker() -> termlens::Result<()> {
    let mut t = spawn();

    // Task 1 starts done.
    assert!(t.screen().contains("[x] HIGH Wire up"), "{}", t.screen());
    assert!(t.screen().contains("status   done"), "{}", t.screen());

    t.send(Key::Char(' '));
    t.wait_until(|s| s.contains("[ ] HIGH Wire up"))?;
    t.wait_until(|s| s.contains("status   open"))?;

    t.send(Key::Char(' '));
    t.wait_until(|s| s.contains("[x] HIGH Wire up"))?;
    Ok(())
}

// -------------------------------------------------------------------- styles

#[test]
fn the_selected_row_is_drawn_in_reverse_video() -> termlens::Result<()> {
    let mut t = spawn();

    let screen = t.screen();
    let selected = style_at(&screen, "[x] HIGH Wire up");
    assert!(selected.reverse, "selected row should be reversed");
    assert!(selected.bold, "selected row should be bold");

    let other = style_at(&screen, "[ ] HIGH Handle SIGWINCH");
    assert!(!other.reverse, "unselected rows must not be reversed");

    // Moving the cursor moves the highlight.
    t.send(Key::Down);
    t.wait_until(|s| s.contains("Tasks 2/10"))?;
    let screen = t.screen();
    assert!(!style_at(&screen, "[x] HIGH Wire up").reverse);
    assert!(style_at(&screen, "[x] HIGH Snapshot").reverse);
    Ok(())
}

#[test]
fn priority_and_status_are_colour_coded() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    // HIGH is bold red, on a row that isn't stealing the selection highlight.
    let high = style_at(&screen, "HIGH Handle SIGWINCH");
    assert_eq!(high.fg, Color::Indexed(1), "HIGH should be red");
    assert!(high.bold, "HIGH should be bold");

    let medium = style_at(&screen, "med  帳票");
    assert_eq!(medium.fg, Color::Indexed(3), "med should be yellow");

    let low = style_at(&screen, "low  Add bracketed");
    assert_eq!(low.fg, Color::Indexed(2), "low should be green");

    // Status bar: white on blue, bold. ratatui's `White` is the *bright*
    // white (SGR 97 → palette 15); palette 7 is its `Gray`.
    let status = style_at(&screen, "NORMAL");
    assert_eq!(status.fg, Color::Indexed(15));
    assert_eq!(status.bg, Color::Indexed(4));
    assert!(status.bold);
    Ok(())
}

#[test]
fn completed_tasks_are_dimmed() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    // "Benchmark the parser" is done and unselected.
    assert!(style_at(&screen, "Benchmark the parser").dim, "{screen}");
    assert!(!style_at(&screen, "Handle SIGWINCH").dim, "{screen}");
    Ok(())
}

// ------------------------------------------------------------------- unicode

#[test]
fn wide_glyphs_occupy_two_columns() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let (row, col) = screen.find("帳票").expect("CJK title on screen");
    let first = screen.cell(row, col).unwrap();
    assert!(first.is_wide(), "帳 should be double-width");
    assert_eq!(first.contents(), "帳");
    assert!(
        screen.cell(row, col + 1).unwrap().is_wide_continuation(),
        "the column after a wide glyph is its continuation cell"
    );
    // The next real glyph starts two columns along, not one.
    assert_eq!(screen.cell(row, col + 2).unwrap().contents(), "票");

    let (row, col) = screen.find("🚀").expect("emoji on screen");
    assert!(screen.cell(row, col).unwrap().is_wide(), "🚀 is double-width");
    Ok(())
}

#[test]
fn wide_glyphs_do_not_break_the_box_drawing() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    // Every list row must end with the pane's right border in the same
    // column — the classic failure mode when wide glyphs are miscounted.
    let (cjk_row, _) = screen.find("帳票").unwrap();
    let (ascii_row, _) = screen.find("Handle SIGWINCH").unwrap();
    let border_col = |row: u16| {
        (0..screen.cols())
            .filter(|&c| screen.cell(row, c).map(|x| x.contents()) == Some("│"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        border_col(cjk_row),
        border_col(ascii_row),
        "wide glyphs shifted the pane borders\n{screen}"
    );
    Ok(())
}

// ------------------------------------------------------------------- resize

/// True once a *complete* status bar occupies the last row of the current
/// geometry — a stand-in for "the app has redrawn since the resize". Matches
/// the tail of the line, so a half-written row doesn't satisfy it.
fn status_bar_repainted(screen: &termlens::Screen) -> bool {
    screen.row_text(screen.rows() - 1).contains("q quit")
}

#[test]
fn resizing_narrow_drops_the_detail_pane() -> termlens::Result<()> {
    let mut t = spawn();
    assert!(t.screen().contains("detail"), "{}", t.screen());

    t.resize(50, 20)?;
    // `resize` re-shapes the grid *immediately*, but the old frame is still
    // painted on it (merely clipped) until the app handles SIGWINCH and
    // redraws. So neither `cols() == 50` nor `contains("tasks (10)")` proves
    // a repaint happened — both are true of the stale frame. Wait instead
    // for something only the *new* geometry can produce: the status bar
    // sitting on the last row of a 20-row screen.
    t.wait_until(status_bar_repainted)?;

    let screen = t.screen();
    assert_eq!(screen.size(), (50, 20));
    assert!(!screen.contains("detail"), "detail pane should be gone\n{screen}");

    // And back again.
    t.resize(100, 30)?;
    t.wait_until(status_bar_repainted)?;
    let screen = t.screen();
    assert_eq!(screen.size(), (100, 30));
    assert!(screen.contains("detail"), "{screen}");
    Ok(())
}

#[test]
fn the_app_is_usable_at_an_awkward_size() -> termlens::Result<()> {
    let mut t = spawn_sized(40, 12);

    let screen = t.screen();
    assert_eq!(screen.size(), (40, 12));
    assert!(screen.contains("tasks (10)"), "{screen}");

    // Paging still moves by whatever fits.
    t.send(Key::End);
    t.wait_until(|s| s.contains("Tasks 10/10"))?;
    Ok(())
}

// -------------------------------------------------------------- exit statuses

#[test]
fn q_quits_cleanly() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('q'));

    let status = t.wait_exit()?;
    assert!(status.success(), "status: {status}");
    assert_eq!(status.code(), 0, "status: {status}");
    assert_eq!(status.signal(), None, "status: {status}");
    Ok(())
}

#[test]
fn ctrl_c_exits_with_130() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Ctrl('c'));

    let status = t.wait_exit()?;
    assert!(!status.success(), "status: {status}");
    // Not a signal death: the app catches the key and chooses the code.
    assert_eq!(status.signal(), None, "status: {status}");
    assert_eq!(status.code(), 130, "status: {status}");
    Ok(())
}

#[test]
fn quitting_restores_the_main_screen() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('q'));
    t.wait_exit()?;

    // Leaving the alternate screen reveals the (empty) primary screen —
    // the TUI's frame is gone rather than left painted over the shell.
    t.wait_until(|s| !s.contains("taskboard"))?;
    Ok(())
}

// ------------------------------------------------------------------ snapshots
//
// A whole-screen snapshot asserts on every cell — including parts of the
// frame the test never named. That makes these the one place a targeted
// predicate isn't sufficient: each waits for its distinguishing content,
// then settles so the remainder of the repaint has landed.

#[test]
fn snapshot_initial_view() {
    let mut t = spawn();
    common::settle(&mut t);
    termlens::assert_screen_snapshot!(t.screen());
}

#[test]
fn snapshot_help_overlay() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('?'));
    t.wait_until(|s| s.contains("move cursor") && s.contains("HELP"))?;
    common::settle(&mut t);
    termlens::assert_screen_snapshot!(t.screen());
    Ok(())
}

#[test]
fn snapshot_filtered_with_confirm_dialog() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('/'));
    t.wait_until(|s| s.contains("FILTER"))?;
    t.send_str("core");
    t.send(Key::Enter);
    t.wait_until(|s| s.contains("filter:core"))?;
    t.send(Key::Char('d'));
    t.wait_until(|s| s.contains("CONFIRM"))?;
    common::settle(&mut t);
    termlens::assert_screen_snapshot!(t.screen());
    Ok(())
}

#[test]
fn snapshot_narrow_layout() -> termlens::Result<()> {
    let mut t = spawn_sized(46, 16);
    common::settle(&mut t);
    termlens::assert_screen_snapshot!(t.screen());
    Ok(())
}
