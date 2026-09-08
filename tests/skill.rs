//! The **skill** termlens ships for coding agents, executed against a real
//! application.
//!
//! `skills/termlens/SKILL.md` is the file an agent copies from, and termlens
//! checks it with `check-skill-snippets.sh` — which *compiles* every Rust
//! block against a stub whose `main` is `fn main() {}`. Compiling proves the
//! API exists. It cannot prove the advice is true, because the stub draws
//! nothing: rule 8's claim about synchronized updates, rule 11's list of what
//! survives `env_clear`, and rule 6's two coordinate orders are all claims
//! about a running program.
//!
//! taskboard is a running program. These are the skill's rules and recipes
//! as mechanisms, measured — the assertion a reader of the skill would end up
//! writing, run against something that actually draws.
//!
//! Transcribed from the skill as shipped with **0.10.1**. It asserts
//! mechanisms rather than the skill's prose, so wording changes do not break
//! it; a *behaviour* change should.

use std::time::Duration;

use termlens::{Color, Key, MouseMode, Terminal};

mod common;

use common::{spawn, spawn_sized};

/// **Rule 8**, both halves. The skill says `wait_frame` works only for
/// applications that bracket repaints in DEC 2026, that stock ratatui with
/// crossterm does *not*, and that `snapshot_after` is the default choice.
///
/// taskboard is the other case — it brackets every repaint by hand — so this
/// pins the conditional from the side the skill's reader is warned about:
/// the app opts in, `repaints()` counts, and `wait_frame` resolves.
#[test]
fn rule_8_wait_frame_needs_the_application_to_opt_in() -> termlens::Result<()> {
    let mut t = spawn();
    let before = t.screen().repaints();
    assert!(before > 0, "taskboard brackets its repaints");

    t.send(Key::Char('j'))?;
    let frame = t.wait_frame(|s| s.contains("Tasks 2/13"))?;
    assert!(frame.repaints() > before, "the repaint was counted");

    // And the skill's default advice reaches the same place.
    t.send(Key::Char('k'))?;
    let settled = t.snapshot_after(|s| s.contains("Tasks 1/13"))?;
    assert!(settled.contains("NORMAL"), "{settled}");
    Ok(())
}

/// **Rule 7**: finish the process. Send the quit key, assert the status, and
/// assert the terminal was restored — an application that leaves the user in
/// the alternate screen fails the test rather than the next command.
#[test]
fn rule_7_a_test_finishes_the_process_and_checks_the_terminal_was_restored() -> termlens::Result<()>
{
    let mut t = spawn();
    assert!(
        t.screen().alternate_screen(),
        "a TUI runs on the alt screen"
    );

    t.send(Key::Char('q'))?;
    let status = t.wait_exit()?;
    assert!(status.success(), "clean exit: {status}");
    assert_eq!(status.code(), Some(0));
    assert_eq!(status.signal(), None, "exited, not killed");
    assert!(
        !t.screen().alternate_screen(),
        "taskboard put the terminal back:\n{}",
        t.screen()
    );
    Ok(())
}

/// **Rule 11**: the environment is hermetic, and the skill names exactly what
/// survives `env_clear` — `TERM=xterm-256color`, `SHELL=/bin/sh`, and
/// nothing else. Measured rather than trusted, because it is the rule most
/// likely to be quietly wrong after a release touching the PTY layer.
#[test]
fn rule_11_env_clear_leaves_only_what_the_skill_says_it_does() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .args(["-c", "env | sort; printf 'ENVDONE\\n'; read x"])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("ENVDONE"))?;
    let dump = t.screen().full_text();
    println!("--- rule 11 --- child environment:\n{dump}");

    assert!(
        dump.contains("TERM=xterm-256color"),
        "TERM is pinned:\n{dump}"
    );
    assert!(dump.contains("SHELL=/bin/sh"), "SHELL is pinned:\n{dump}");
    for leaked in ["HOME=", "LANG=", "COLORTERM=", "NO_COLOR=", "LS_COLORS="] {
        assert!(!dump.contains(leaked), "{leaked} leaked in:\n{dump}");
    }
    Ok(())
}

/// **Rule 5**: geometry is 2..=1000 per axis, and both bounds are refused
/// before anything is spawned.
#[test]
fn rule_5_geometry_is_bounded_at_both_ends() {
    for (cols, rows) in [(1u16, 24u16), (80, 1), (1001, 24), (80, 1001)] {
        let err = Terminal::builder()
            .size(cols, rows)
            .spawn(env!("CARGO_BIN_EXE_taskboard"))
            .expect_err("outside the bounds");
        assert!(
            matches!(err, termlens::Error::Size(_)),
            "{cols}x{rows}: {err:?}"
        );
    }
    // And the default the skill recommends is inside them.
    let t = spawn_sized(80, 24);
    assert_eq!(t.screen().size(), (80, 24));
}

/// **Rule 6**: everything addressing a cell is row-first, everything speaking
/// of geometry or a pointer is column-first. The skill's fix for the mix-up
/// is to destructure and pass each coordinate deliberately — so this clicks
/// where `find` pointed and checks the application agreed.
#[test]
fn rule_6_find_returns_row_col_and_click_takes_col_row() -> termlens::Result<()> {
    let mut t = spawn();
    t.wait_until(|s| s.mouse_mode() != MouseMode::None)?;

    let (row, col) = t.screen().find("Add bracketed paste").expect("a task row");
    // Column-first into the pointer API, from a row-first result.
    t.click(col, row)?;
    let after = t.wait_frame(|s| s.contains("Add bracketed paste"))?;
    assert!(
        after.contains("bracketed"),
        "the click selected the row it was pointed at:\n{after}"
    );
    // The detail pane follows the selection, which is how we know the click
    // landed on that row rather than merely somewhere.
    assert!(
        after.find("title    Add bracketed paste").is_some(),
        "{after}"
    );
    Ok(())
}

/// **Rule 12**: think in cells. A double-width glyph occupies two, `find`
/// reports real terminal columns, and `contains` folds to NFC so a needle
/// typed one way finds text normalized the other.
#[test]
fn rule_12_the_grid_is_unicode_aware_and_find_reports_real_columns() {
    let t = spawn();
    let screen = t.screen();

    let (row, col) = screen.find("帳票").expect("the CJK task");
    assert!(
        screen.cell(row, col).unwrap().is_wide(),
        "one glyph, two cells"
    );
    assert!(screen.cell(row, col + 1).unwrap().is_wide_continuation());
    // The next glyph starts two columns on, not one.
    assert_eq!(screen.find("票"), Some((row, col + 2)));

    // NFC folding: `café` composed and decomposed are the same needle.
    assert!(screen.contains("café"), "composed:\n{screen}");
    assert!(screen.contains("cafe\u{301}"), "decomposed finds it too");
}

/// **Rule 9**: return `termlens::Result<()>` and use `?`, because every
/// error's `Display` ends with the screen — the reason a CI log is enough to
/// diagnose a failure without a rerun.
#[test]
fn rule_9_a_failure_carries_the_screen_into_the_message() {
    let mut t = spawn();
    let err = t
        .wait_until_for(|s| s.contains("never appears"), Duration::from_millis(300))
        .expect_err("must time out");
    let message = err.to_string();

    assert!(message.contains("timed out after"), "{message}");
    assert!(message.contains("--- screen at timeout ---"), "{message}");
    assert!(
        message.contains("Wire up the PTY reader"),
        "the grid is in it:\n{message}"
    );
    assert!(err.screen().is_some(), "and reachable as a value");
}

/// **Recipe D**, against a real application: targeted cell and style
/// assertions, which is the recipe an agent adapts most often.
#[test]
fn recipe_d_targeted_cell_and_style_assertions() {
    let t = spawn();
    let screen = t.screen();

    // The selected row is drawn in reverse video.
    let (row, col) = screen
        .find("Wire up the PTY reader")
        .expect("the selection");
    assert!(
        screen.cell(row, col).unwrap().style().reverse,
        "{}",
        screen.with_styles()
    );
    assert_eq!(
        screen.find_by(|cell| cell.style().reverse),
        Some((row, col - 9))
    );

    // The overdue badge is blinking red — two attributes, one cell.
    let (brow, bcol) = screen.find("! Handle SIGWINCH").expect("the badge");
    let badge = screen.cell(brow, bcol).unwrap().style();
    assert_eq!(badge.fg, Color::Indexed(1));
    assert!(badge.blink);

    // A region, and the cursor.
    let pane = screen.rect_text(0..40, 4..8);
    assert!(pane.contains("Snapshot the screen grid"), "{pane}");
    assert!(!screen.cursor().2, "a list view hides the cursor");
}

/// **The pitfalls table**: `send_str` sends the bytes given, so a trailing
/// `\n` is a line feed and not the carriage return a terminal sends for
/// Return. The skill's fix is to send `Key::Enter` — this shows the
/// difference against an application that acts on it.
#[test]
fn pitfall_send_str_newline_is_not_the_enter_key() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('/'))?;
    t.wait_frame(|s| s.contains("FILTER"))?;

    // The text arrives either way...
    t.send_str("core")?;
    t.wait_frame(|s| s.contains("core"))?;
    assert!(t.screen().contains("FILTER"), "still editing the filter");

    // ...but only Key::Enter commits it, which is the whole pitfall.
    t.send(Key::Enter)?;
    let committed = t.wait_frame(|s| s.contains("filter:core"))?;
    assert!(
        !committed.contains("FILTER"),
        "the mode ended:\n{committed}"
    );
    Ok(())
}

/// **What the skill's Recipe A cannot be exercised against here**, recorded
/// rather than skipped: Recipe A snapshots a hermetic CLI's `--help`, and
/// taskboard has no `--help` — it is a pure TUI whose only arguments are two
/// probe flags. The mechanism the recipe teaches (wait for the *last* thing
/// printed before waiting for exit, so a fast program cannot lose its tail
/// to PTY teardown) is still worth asserting, and `--probe-caps` is the
/// closest thing this subject has to a run-and-report mode.
#[test]
fn recipe_a_the_mechanism_without_a_help_flag() -> termlens::Result<()> {
    let mut t = common::spawn_args(&["--probe-caps"], Duration::from_secs(5));
    // Wait on content before waiting on exit — the recipe's actual advice.
    t.wait_until(|s| s.contains("NORMAL"))?;
    t.send(Key::Char('q'))?;
    let status = t.wait_exit()?;
    assert!(status.success(), "{status}");
    Ok(())
}
