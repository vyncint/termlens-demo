//! What termlens 0.2 covers: everything the user can see, type, click or
//! signal — plus the terminal state around the grid.
//!
//! Every wait here is `wait_frame`, which evaluates only complete DEC 2026
//! frames. Compare `git show HEAD~1 -- tests/tui.rs`: under 0.1 each of
//! these needed a single combined predicate anchored on the last-painted
//! region, because a predicate could fire on half a frame.

mod common;

use std::time::Duration;

use common::{spawn, spawn_sized, style_at};
use termlens::{Color, Key, MouseMode, Scroll, Signal, Terminal};

// ---------------------------------------------------------------- navigation

#[test]
fn boots_into_the_tasks_tab_with_the_first_row_selected() {
    let t = spawn();
    let screen = t.screen();

    assert!(screen.contains("tasks (10)"), "{screen}");
    assert!(screen.contains("Tasks 1/10"), "{screen}");
    assert!(screen.contains("Wire up the PTY reader"), "{screen}");
}

#[test]
fn arrow_and_vim_keys_move_the_selection() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Down);
    t.wait_frame(|s| s.contains("Tasks 2/10"))?;

    t.send(Key::Char('j'));
    // A complete frame means the detail pane is already consistent with the
    // status bar — no second wait, no combined predicate.
    t.wait_frame(|s| s.contains("Tasks 3/10"))?;
    assert!(t.screen().contains("priority med"), "{}", t.screen());

    t.send(Key::Char('k'));
    t.wait_frame(|s| s.contains("Tasks 2/10"))?;

    t.send(Key::Up);
    t.wait_frame(|s| s.contains("Tasks 1/10"))?;
    Ok(())
}

#[test]
fn selection_clamps_at_both_ends() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Up);
    t.send(Key::Up);
    t.wait_frame(|s| s.contains("Tasks 1/10"))?;

    t.send(Key::End);
    t.wait_frame(|s| s.contains("Tasks 10/10"))?;
    t.send(Key::Down);
    t.wait_frame(|s| s.contains("Tasks 10/10"))?;

    t.send(Key::Home);
    t.wait_frame(|s| s.contains("Tasks 1/10"))?;
    Ok(())
}

#[test]
fn page_keys_move_by_a_screenful() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Tab);
    t.send(Key::Tab);
    t.wait_frame(|s| s.contains("logs (40)"))?;

    t.send(Key::PageDown);
    t.wait_frame(|s| s.contains("Logs 21/40"))?;

    t.send(Key::PageUp);
    t.wait_frame(|s| s.contains("Logs 1/40"))?;
    Ok(())
}

#[test]
fn tab_and_backtab_cycle_the_tabs() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Tab);
    t.wait_frame(|s| s.contains("total    10"))?;

    t.send(Key::Tab);
    t.wait_frame(|s| s.contains("logs (40)"))?;

    t.send(Key::Tab);
    t.wait_frame(|s| s.contains("tasks (10)"))?;

    t.send(Key::BackTab);
    t.wait_frame(|s| s.contains("logs (40)"))?;
    Ok(())
}

/// `Chord` covers modifier + special key, which 0.1 could only reach by
/// hand-writing `"\x1b[1;5C"`.
#[test]
fn ctrl_arrow_chords_switch_tabs() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Right.ctrl());
    t.wait_frame(|s| s.contains("total    10"))?;

    t.send(Key::Left.ctrl());
    t.wait_frame(|s| s.contains("tasks (10)"))?;
    Ok(())
}

// -------------------------------------------------------------------- filter

#[test]
fn filter_mode_shows_a_live_cursor_and_narrows_the_list() -> termlens::Result<()> {
    let mut t = spawn();
    assert!(!t.screen().cursor().2, "cursor should start hidden");

    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;

    t.send_str("core");
    t.wait_frame(|s| s.contains("/core"))?;

    // Cursor state is part of the frame, so it can be read straight off the
    // screen rather than folded into the predicate.
    let screen = t.screen();
    assert_eq!(screen.cursor(), (screen.rows() - 2, 5, true), "{screen}");

    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("tasks (4) filtered"))?;

    let screen = t.screen();
    assert!(screen.contains("filter:core"), "{screen}");
    assert!(!screen.contains("Add bracketed paste"), "{screen}");
    assert!(!screen.cursor().2, "cursor hidden again after commit");
    Ok(())
}

#[test]
fn backspace_edits_the_draft_before_it_is_applied() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.send_str("docs");
    t.wait_frame(|s| s.contains("/docs"))?;

    t.send(Key::Backspace);
    t.send(Key::Backspace);
    t.wait_frame(|s| s.contains("/do") && !s.contains("/docs"))?;
    assert!(t.screen().contains("tasks (10)"), "draft is not applied yet");

    // "do" matches the `docs` tag and the title "Windows ConPTY support".
    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("tasks (3) filtered"))?;
    assert!(t.screen().contains("Document the wait"), "{}", t.screen());
    Ok(())
}

/// `paste` wraps in `ESC[200~ … ESC[201~` **only because the app enabled
/// mode 2004** — termlens reads the mode off the emulator. 0.1 had no paste
/// API at all.
#[test]
fn pasting_delivers_one_paste_event() -> termlens::Result<()> {
    let mut t = spawn();
    assert!(t.screen().bracketed_paste(), "app should have enabled 2004");

    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;

    t.paste("core");
    t.wait_frame(|s| s.contains("/core"))?;

    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("tasks (4) filtered"))?;
    Ok(())
}

#[test]
fn filter_matches_tags_as_well_as_titles() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.paste("i18n");
    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("tasks (2) filtered"))?;

    let screen = t.screen();
    assert!(screen.contains("帳票をレンダリングする"), "{screen}");
    assert!(screen.contains("emoji width"), "{screen}");
    Ok(())
}

#[test]
fn esc_abandons_the_filter_draft() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.send_str("perf");
    t.wait_frame(|s| s.contains("/perf"))?;

    t.send(Key::Esc);
    t.wait_frame(|s| s.contains("NORMAL"))?;

    let screen = t.screen();
    assert!(screen.contains("tasks (10)"), "{screen}");
    assert!(!screen.contains("filter:"), "{screen}");
    Ok(())
}

#[test]
fn esc_in_normal_mode_clears_an_applied_filter() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.send_str("perf");
    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("filter:perf"))?;

    t.send(Key::Esc);
    // In 0.1 this was the flakiest test in the suite: waiting on the list
    // and then reading the status bar caught the frame half-painted.
    t.wait_frame(|s| s.contains("tasks (10)"))?;
    assert!(!t.screen().contains("filter:"), "{}", t.screen());
    Ok(())
}

// --------------------------------------------------------------- modal + help

#[test]
fn confirm_dialog_deletes_the_selected_task() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('d'));
    t.wait_frame(|s| s.contains("delete this task?"))?;
    let screen = t.screen();
    assert!(screen.contains("CONFIRM"), "{screen}");
    assert!(screen.contains("[y] delete   [n] cancel"), "{screen}");

    t.send(Key::Char('y'));
    t.wait_frame(|s| s.contains("tasks (9)"))?;

    let screen = t.screen();
    assert!(!screen.contains("Wire up the PTY reader"), "{screen}");
    assert!(screen.contains("NORMAL"), "{screen}");
    Ok(())
}

#[test]
fn confirm_dialog_cancels_without_deleting() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('d'));
    t.wait_frame(|s| s.contains("CONFIRM"))?;

    t.send(Key::Char('n'));
    t.wait_frame(|s| s.contains("NORMAL"))?;

    let screen = t.screen();
    assert!(screen.contains("tasks (10)"), "{screen}");
    assert!(screen.contains("Wire up the PTY reader"), "{screen}");
    assert!(!screen.contains("delete this task?"), "{screen}");
    Ok(())
}

#[test]
fn help_overlay_opens_from_question_mark_and_f1() -> termlens::Result<()> {
    let mut t = spawn();

    t.send(Key::Char('?'));
    t.wait_frame(|s| s.contains("move cursor"))?;
    assert!(t.screen().contains("HELP"), "{}", t.screen());

    t.send(Key::Esc);
    t.wait_frame(|s| s.contains("NORMAL"))?;
    assert!(!t.screen().contains("move cursor"), "{}", t.screen());

    t.send(Key::F(1));
    t.wait_frame(|s| s.contains("HELP"))?;
    Ok(())
}

#[test]
fn space_toggles_the_done_marker() -> termlens::Result<()> {
    let mut t = spawn();

    let screen = t.screen();
    assert!(screen.contains("[x] HIGH Wire up"), "{screen}");
    assert!(screen.contains("status   done"), "{screen}");

    t.send(Key::Char(' '));
    t.wait_frame(|s| s.contains("[ ] HIGH Wire up"))?;
    assert!(t.screen().contains("status   open"), "{}", t.screen());

    t.send(Key::Char(' '));
    t.wait_frame(|s| s.contains("[x] HIGH Wire up"))?;
    Ok(())
}

// --------------------------------------------------------------------- mouse

/// `click` encodes for whichever tracking mode the *application* enabled,
/// and refuses if it enabled none. 0.1 required hand-rolled SGR bytes with
/// no such check.
#[test]
fn clicking_a_row_selects_it() -> termlens::Result<()> {
    let mut t = spawn();
    // crossterm's EnableMouseCapture turns on 1003 (any-motion) + SGR.
    assert_eq!(t.screen().mouse_mode(), MouseMode::AnyMotion);

    // Screen row 6 is the third list entry (3 tab rows + 1 border).
    t.click(10, 6)?;
    t.wait_frame(|s| s.contains("Tasks 3/10"))?;
    assert!(t.screen().contains("priority med"), "{}", t.screen());
    Ok(())
}

#[test]
fn the_wheel_moves_the_selection() -> termlens::Result<()> {
    let mut t = spawn();

    t.scroll(10, 6, Scroll::Down)?;
    t.wait_frame(|s| s.contains("Tasks 2/10"))?;

    t.scroll(10, 6, Scroll::Down)?;
    t.wait_frame(|s| s.contains("Tasks 3/10"))?;

    t.scroll(10, 6, Scroll::Up)?;
    t.wait_frame(|s| s.contains("Tasks 2/10"))?;
    Ok(())
}

#[test]
fn clicking_an_app_without_mouse_tracking_is_an_error() {
    // `cat` never enables mouse tracking.
    let mut t = common::spawn_sh("cat", Duration::from_secs(5));
    let err = t.click(0, 0).expect_err("should refuse");
    assert!(
        err.to_string().contains("has not enabled mouse tracking"),
        "unexpected error: {err}"
    );
}

// ---------------------------------------------------- terminal state accessors

/// Out-of-band terminal state — none of this was reachable in 0.1.
#[test]
fn terminal_state_around_the_grid_is_readable() -> termlens::Result<()> {
    let mut t = spawn();
    let screen = t.screen();

    assert_eq!(screen.title(), "taskboard", "OSC 0 window title");
    assert!(screen.alternate_screen(), "TUI should be on the alt screen");
    assert!(screen.bracketed_paste(), "mode 2004");
    assert_eq!(screen.mouse_mode(), MouseMode::AnyMotion);
    // ratatui does not set DECCKM, so cursor keys keep their CSI forms.
    assert!(!screen.application_cursor());

    t.send(Key::Char('q'));
    t.wait_exit()?;
    // Leaving the alternate screen is now directly observable.
    t.wait_until(|s| !s.alternate_screen())?;
    Ok(())
}

// -------------------------------------------------------------------- styles

#[test]
fn the_selected_row_is_drawn_in_reverse_video() -> termlens::Result<()> {
    let mut t = spawn();

    let screen = t.screen();
    let selected = style_at(&screen, "[x] HIGH Wire up");
    assert!(selected.reverse && selected.bold);
    assert!(!style_at(&screen, "[ ] HIGH Handle SIGWINCH").reverse);

    // `find_by` locates the highlight directly instead of guessing which
    // text carries it — new in 0.2.
    let first_reverse = screen.find_by(|c| c.style().reverse);
    assert_eq!(first_reverse, Some((4, 1)), "highlight on the first row");

    t.send(Key::Down);
    t.wait_frame(|s| s.contains("Tasks 2/10"))?;
    assert_eq!(t.screen().find_by(|c| c.style().reverse), Some((5, 1)));
    Ok(())
}

#[test]
fn priority_and_status_are_colour_coded() {
    let t = spawn();
    let screen = t.screen();

    let high = style_at(&screen, "HIGH Handle SIGWINCH");
    assert_eq!(high.fg, Color::Indexed(1), "HIGH should be red");
    assert!(high.bold);

    assert_eq!(style_at(&screen, "med  帳票").fg, Color::Indexed(3));
    assert_eq!(style_at(&screen, "low  Add bracketed").fg, Color::Indexed(2));

    // ratatui's `White` is the bright white (SGR 97 → palette 15).
    let status = style_at(&screen, "NORMAL");
    assert_eq!(status.fg, Color::Indexed(15));
    assert_eq!(status.bg, Color::Indexed(4));
    assert!(status.bold);
}

#[test]
fn completed_tasks_are_dimmed() {
    let t = spawn();
    let screen = t.screen();
    assert!(style_at(&screen, "Benchmark the parser").dim, "{screen}");
    assert!(!style_at(&screen, "Handle SIGWINCH").dim, "{screen}");
}

// ------------------------------------------------------------------- unicode

#[test]
fn wide_glyphs_occupy_two_columns() {
    let t = spawn();
    let screen = t.screen();

    let (row, col) = screen.find("帳票").expect("CJK title on screen");
    let first = screen.cell(row, col).unwrap();
    assert!(first.is_wide());
    assert_eq!(first.contents(), "帳");
    assert!(screen.cell(row, col + 1).unwrap().is_wide_continuation());
    assert_eq!(screen.cell(row, col + 2).unwrap().contents(), "票");

    let (row, col) = screen.find("🚀").expect("emoji on screen");
    assert!(screen.cell(row, col).unwrap().is_wide());
}

#[test]
fn wide_glyphs_do_not_break_the_box_drawing() {
    let t = spawn();
    let screen = t.screen();

    let (cjk_row, _) = screen.find("帳票").unwrap();
    let (ascii_row, _) = screen.find("Handle SIGWINCH").unwrap();
    let border_cols = |row: u16| {
        (0..screen.cols())
            .filter(|&c| screen.cell(row, c).map(|x| x.contents()) == Some("│"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        border_cols(cjk_row),
        border_cols(ascii_row),
        "wide glyphs shifted the pane borders\n{screen}"
    );
}

// ------------------------------------------------------------------- layout

/// `rect_text` reads one pane without the other bleeding in — 0.1 needed a
/// hand-written column-slicing helper.
#[test]
fn rect_text_isolates_a_pane() {
    let t = spawn();
    let screen = t.screen();

    let list = screen.rect_text(0..40, 4..8);
    assert!(list.contains("Wire up the PTY reader"), "{list}");
    assert!(!list.contains("Drain continuously"), "detail pane bled in:\n{list}");

    let detail = screen.rect_text(41.., ..);
    assert!(detail.contains("Drain continuously"), "{detail}");
    assert!(!detail.contains("Benchmark the parser"), "{detail}");
}

/// In 0.1 `find` was single-row and returned `None` for a needle spanning
/// rows, even where `contains` matched. 0.2 gives it `contains` semantics.
#[test]
fn find_locates_text_that_spans_rows() {
    let t = spawn();
    let screen = t.screen();

    let (row, col) = screen
        .find("[x] HIGH Wire up the PTY reader")
        .expect("single-row needle");
    assert_eq!(col, 1, "reports the real column, inside the border");

    let across = format!(
        "{}\n{}",
        screen.row_text(row).trim_end(),
        screen.row_text(row + 1).trim_end()
    );
    assert!(screen.contains(&across));
    assert_eq!(
        screen.find(&across),
        Some((row, 0)),
        "multi-row needles are located, not rejected"
    );
}

#[test]
fn resizing_narrow_drops_the_detail_pane() -> termlens::Result<()> {
    let mut t = spawn();
    assert!(t.screen().contains("detail"));

    t.resize(50, 20)?;
    // Under 0.1 the stale frame satisfied content predicates until the app
    // repainted, so this needed a geometry-specific probe. A frame is only
    // published when the repaint completes, so the obvious predicate works.
    t.wait_frame(|s| s.cols() == 50 && !s.contains("detail"))?;
    assert_eq!(t.screen().size(), (50, 20));

    t.resize(100, 30)?;
    t.wait_frame(|s| s.cols() == 100 && s.contains("detail"))?;
    assert_eq!(t.screen().size(), (100, 30));
    Ok(())
}

#[test]
fn the_app_is_usable_at_an_awkward_size() -> termlens::Result<()> {
    let mut t = spawn_sized(40, 12);

    let screen = t.screen();
    assert_eq!(screen.size(), (40, 12));
    assert!(screen.contains("tasks (10)"), "{screen}");

    t.send(Key::End);
    t.wait_frame(|s| s.contains("Tasks 10/10"))?;
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
    assert_eq!(status.signal(), None, "the app chose the code itself");
    assert_eq!(status.code(), 130, "status: {status}");
    Ok(())
}

/// Graceful shutdown on SIGTERM — untestable in 0.1, which exposed neither
/// the pid nor a way to signal the child.
#[test]
fn sigterm_shuts_down_gracefully() -> termlens::Result<()> {
    let mut t = spawn();
    assert!(t.pid().is_some(), "pid should be exposed while running");

    t.signal(Signal::Term)?;

    // The shutdown frame is drawn and then immediately wiped by the
    // alt-screen teardown. `wait_frame` still catches it, because a
    // completed frame is retained; `wait_until` races the teardown.
    t.wait_frame(|s| s.contains("SAVING"))?;

    let status = t.wait_exit()?;
    assert_eq!(status.code(), 143, "128 + SIGTERM; status: {status}");
    assert_eq!(status.signal(), None, "the app handled it, not the kernel");
    Ok(())
}

#[test]
fn signalling_a_reaped_child_is_refused() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('q'));
    t.wait_exit()?;

    let err = t.signal(Signal::Term).expect_err("pid may have been reused");
    assert!(err.to_string().contains("already exited"), "{err}");
    Ok(())
}

// -------------------------------------------------------------- builder extras

#[test]
fn current_dir_sets_the_child_working_directory() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .current_dir("/usr")
        .args(["-c", "pwd; read x"])
        .spawn("/bin/sh")?;

    t.wait_until(|s| s.contains("/usr"))?;
    Ok(())
}

#[test]
fn a_single_slow_wait_can_have_its_own_timeout() -> termlens::Result<()> {
    // Builder timeout is deliberately short; one known-slow step overrides
    // it without slowing every other wait in the suite.
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_millis(200))
        .args(["-c", "sleep 0.6; echo late; read x"])
        .spawn("/bin/sh")?;

    assert!(t.wait_until(|s| s.contains("late")).is_err(), "short timeout");
    t.wait_until_for(|s| s.contains("late"), Duration::from_secs(5))?;
    Ok(())
}

// ------------------------------------------------------------------ snapshots
//
// `wait_frame` means a snapshot never has to be settled first: the frame it
// returns on is by construction complete.

#[test]
fn snapshot_initial_view() {
    let t = spawn();
    termlens::assert_screen_snapshot!(t.screen());
}

/// The styled snapshot catches what a text snapshot cannot — move the
/// highlight and this diff changes. New in 0.2.
#[test]
fn snapshot_initial_view_with_styles() {
    let t = spawn();
    termlens::assert_screen_snapshot!(t.screen().with_styles());
}

#[test]
fn snapshot_help_overlay() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('?'));
    t.wait_frame(|s| s.contains("move cursor"))?;
    termlens::assert_screen_snapshot!(t.screen());
    Ok(())
}

#[test]
fn snapshot_filtered_with_confirm_dialog() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('/'));
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.paste("core");
    t.send(Key::Enter);
    t.wait_frame(|s| s.contains("filter:core"))?;
    t.send(Key::Char('d'));
    t.wait_frame(|s| s.contains("CONFIRM"))?;
    termlens::assert_screen_snapshot!(t.screen());
    Ok(())
}

#[test]
fn snapshot_narrow_layout() {
    let t = spawn_sized(46, 16);
    termlens::assert_screen_snapshot!(t.screen());
}
