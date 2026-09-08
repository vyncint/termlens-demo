//! What termlens **0.10** added, probed against the real binary rather than
//! inferred from the crate. This repository sat on 0.6.1 while 0.7, 0.8, 0.9
//! and 0.10 shipped, so this suite is the catch-up: the search and mask
//! surface, the export surface (`diff`, the three renderings, `serde`,
//! `Screen::parse`), recording, the rebuilt snapshot macro, the emulator's
//! own honesty accessors, and the smaller things 0.7–0.9 added that nothing
//! here had exercised.
//!
//! Every claim is reproduced against a real process, and everything under
//! test comes from crates.io.

use std::time::Duration;

use termlens::{Color, Key, Screen, Terminal};

mod common;

use common::{spawn, spawn_sh};

/// A plain grid from `/bin/sh`, for the probes that need rows without a
/// pane border on the end of them.
fn lines(script: &str) -> Terminal {
    spawn_sh(
        &format!("stty raw -echo; {script}; head -c 1 >/dev/null"),
        Duration::from_secs(5),
    )
}

// ===================================================== 1. search (0.10)

/// `find` returns the first match; `find_all` returns every one, in reading
/// order, non-overlapping. taskboard's status markers are the natural probe:
/// thirteen tasks, each with a `[ ]`, `[x]` or `[~]`.
#[test]
fn find_all_returns_every_occurrence_not_just_the_first() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let done = screen.find_all("[x]");
    assert_eq!(done.len(), 3, "three done tasks:\n{screen}");
    assert_eq!(
        screen.find("[x]"),
        Some(done[0]),
        "find is find_all's first"
    );

    // Reading order: rows ascending, and all in the list pane's column.
    let rows: Vec<u16> = done.iter().map(|(row, _)| *row).collect();
    let mut sorted = rows.clone();
    sorted.sort_unstable();
    assert_eq!(rows, sorted, "reading order");
    assert!(done.iter().all(|(_, col)| *col == done[0].1), "one column");

    // Every marker, of every kind, is thirteen — one per task.
    let total = screen.find_all("[").len();
    assert_eq!(total, 13, "one marker per task:\n{screen}");
    Ok(())
}

/// A needle that spans a soft wrap is found by `logical_text`, not by
/// `contains` — the grid has no idea two rows are one line unless the
/// backend recorded the wrap, which `row_wrapped` reports.
#[test]
fn a_soft_wrapped_line_is_two_rows_and_one_logical_line() -> termlens::Result<()> {
    // 20 columns, one 30-character word-free line: the terminal wraps it.
    let mut t = spawn_sh(
        r"stty raw -echo; printf 'ABCDEFGHIJKLMNOPQRSTUVWXYZ0123'; head -c 1 >/dev/null",
        Duration::from_secs(5),
    );
    t.resize(20, 6)?;
    t.wait_until(|s| s.contains("ABCDEFGHIJ"))?;
    let screen = t.screen();

    assert!(screen.row_wrapped(0), "row 0 wrapped:\n{screen}");
    assert!(!screen.row_wrapped(1), "row 1 did not");
    // The needle straddles the wrap, so the per-row read cannot see it...
    assert!(!screen.contains("STUVWXYZ0123"), "not within one row");
    // ...and the logical read can.
    assert!(
        screen
            .logical_text()
            .contains("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123"),
        "logical_text joins the wrap:\n{:?}",
        screen.logical_text()
    );
    Ok(())
}

/// `locate` answers *which region* holds a needle. On the grid it agrees
/// with `find`; the history half is pinned in `limits.rs`.
#[test]
fn locate_reports_the_region_as_well_as_the_position() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();
    let (row, col) = screen
        .find("Wire up the PTY reader")
        .expect("the first task");
    match screen.locate("Wire up the PTY reader") {
        Some(termlens::Location::Screen { row: r, col: c }) => assert_eq!((r, c), (row, col)),
        other => panic!("expected a grid location, got {other:?}"),
    }
    assert!(screen.locate("no such text anywhere").is_none());
    Ok(())
}

// ============================================== 2. the regex feature (0.10)

/// A pattern over a *row of the screen*: the match still lands on cells with
/// coordinates, which is what separates this from stream matching.
#[test]
fn a_regex_matches_a_row_and_reports_real_columns() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let count = regex::Regex::new(r"tasks \((\d+)\)").unwrap();
    let (row, col, text) = screen.find_match(&count).expect("the list pane title");
    assert_eq!(text, "tasks (13)");
    assert_eq!(
        screen.find("tasks (13)"),
        Some((row, col)),
        "same cell as find"
    );

    // Every priority label, by shape rather than by literal.
    let priority = regex::Regex::new(r"\b(HIGH|med|low)\b").unwrap();
    assert_eq!(
        screen.find_all_matches(&priority).len(),
        14,
        "thirteen tasks plus the detail pane's own line:\n{screen}"
    );
    assert!(screen.matches(&priority), "matches() is the boolean form");
    Ok(())
}

/// The expect-style wait, on the rendered screen rather than the byte stream.
#[test]
fn wait_until_matches_is_an_expect_style_wait_on_the_grid() -> termlens::Result<()> {
    let mut t = spawn();
    // No `$`: the pane title row continues past the count in box drawing.
    let filtered = regex::Regex::new(r"tasks \([1-9]\)").unwrap();
    t.send(Key::Char('/'))?;
    t.wait_frame(|s| s.contains("FILTER"))?;
    t.paste("core")?;
    t.send(Key::Enter)?;
    // The count drops to a single digit once the filter applies.
    let screen = t.wait_until_matches(&filtered)?;
    assert!(screen.contains("filter:core"), "{screen}");
    Ok(())
}

// ==================================================== 3. masks (0.10)

/// A mask replaces cell *contents* and changes nothing else: the size, the
/// cursor, every style and the two columns of a wide character all survive.
/// That is the whole reason it lives in the crate rather than being a text
/// filter over the rendering.
#[test]
fn a_mask_keeps_the_geometry_and_the_styles() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let masked = screen.mask_matching("Wire up the PTY reader", '#');
    assert_eq!(masked.size(), screen.size(), "same grid");
    assert_eq!(masked.cursor(), screen.cursor(), "same cursor");
    assert!(!masked.contains("Wire up the PTY reader"), "{masked}");
    assert!(masked.contains("######################"), "{masked}");

    // The style under the mask is the original's, so a styled snapshot of a
    // masked screen still catches a colour regression.
    let (row, col) = screen.find("Wire up the PTY reader").unwrap();
    assert_eq!(
        masked.cell(row, col).unwrap().style(),
        screen.cell(row, col).unwrap().style(),
        "the mask is not a restyle"
    );

    // A wide character under a mask becomes two fill cells, so the row keeps
    // its width rather than shifting every column after it.
    let (crow, ccol) = screen.find("帳票").expect("the CJK task");
    assert!(screen.cell(crow, ccol).unwrap().is_wide());
    let wide = screen.mask_matching("帳票", '*');
    assert!(
        !wide.cell(crow, ccol).unwrap().is_wide(),
        "two narrow fills"
    );
    assert!(!wide.cell(crow, ccol + 1).unwrap().is_wide_continuation());
    // Column preservation is the invariant, and it is *columns* — comparing
    // `row_text().len()` would compare bytes, where one CJK glyph is three
    // and its two fill cells are two.
    assert_eq!(
        wide.find("をレンダリング"),
        screen.find("をレンダリング"),
        "the text after the mask did not move"
    );
    Ok(())
}

/// The three ways to choose what to mask, plus the regex form.
#[test]
fn masks_select_by_rectangle_by_literal_by_predicate_and_by_pattern() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();
    let (rows, cols) = (screen.rows(), screen.cols());

    // By rectangle: the status bar, which carries a volatile selection index.
    let bar = rows - 1;
    let rect = screen.mask_rect(0..cols, bar..rows);
    assert!(!rect.contains("Tasks 1/13"), "{rect}");
    assert!(rect.contains("tasks (13)"), "only that row:\n{rect}");

    // By predicate: every blinking cell, whatever it says.
    let blink = screen.mask_cells(|cell| cell.style().blink);
    assert!(
        blink.find_by(|cell| cell.style().blink).is_some(),
        "still blinking"
    );
    assert!(
        !blink.contains("! Handle SIGWINCH"),
        "the badge text is gone:\n{blink}"
    );

    // By pattern.
    let digits = regex::Regex::new(r"\d+").unwrap();
    let masked = screen.mask_matches(&digits, '0');
    assert!(!masked.contains("tasks (13)"), "{masked}");
    assert!(masked.contains("tasks (00)"), "same width:\n{masked}");
    Ok(())
}

/// A needle that spans rows is masked across all of them. 0.10.0 shipped
/// this reporting the match through `find_all` and then masking nothing;
/// 0.10.1 gave the two one engine.
#[test]
fn a_mask_covers_a_needle_that_spans_two_rows() -> termlens::Result<()> {
    let mut t = lines(r"printf 'SECRET-ONE\r\nSECRET-TWO\r\nKEEP\r\n'");
    t.wait_until(|s| s.contains("KEEP"))?;
    let screen = t.screen();

    assert_eq!(screen.find_all("SECRET-ONE\nSECRET-TWO"), vec![(0, 0)]);
    let masked = screen.mask_matching("SECRET-ONE\nSECRET-TWO", '*');
    assert!(
        masked.find_all("SECRET-ONE\nSECRET-TWO").is_empty(),
        "the needle survived the mask:\n{masked}"
    );
    assert_eq!(masked.row_text(0).trim_end(), "**********");
    assert_eq!(masked.row_text(1).trim_end(), "**********");
    assert_eq!(
        masked.row_text(2).trim_end(),
        "KEEP",
        "and nothing else moved"
    );
    Ok(())
}

// ================================================= 4. Screen::diff (0.10)

/// The documented way to compare two screens outside insta: only the rows
/// that changed, side by side, with the style runs before and after.
#[test]
fn a_diff_names_what_changed_and_is_empty_when_nothing_did() -> termlens::Result<()> {
    let mut t = spawn();
    let before = t.screen();

    assert!(before.diff(&before).is_empty(), "a screen equals itself");

    t.send(Key::Char('j'))?;
    let after = t.wait_frame(|s| s.contains("Tasks 2/13"))?;
    let diff = before.diff(&after);
    assert!(!diff.is_empty(), "moving the cursor changed the picture");

    // The highlight moved, so the two list rows and the status bar changed —
    // and the detail pane repainted with the newly selected task.
    let changed: Vec<u16> = {
        let mut rows: Vec<u16> = diff.cells().map(|(row, ..)| row).collect();
        rows.dedup();
        rows
    };
    assert!(changed.len() >= 3, "rows changed: {changed:?}\n{diff}");
    let rendered = diff.to_string();
    assert!(
        rendered.contains("Tasks 1/13"),
        "the before side:\n{rendered}"
    );
    assert!(
        rendered.contains("Tasks 2/13"),
        "the after side:\n{rendered}"
    );
    assert!(
        rendered.contains("rows unchanged"),
        "and a count of the rest"
    );
    Ok(())
}

// =========================================== 5. the three renderings (0.10)

/// ANSI for a terminal, SVG for a report, HTML for a summary — all pure
/// functions of the screen, all needing no dependency.
#[test]
fn a_screen_renders_to_ansi_svg_and_html() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let ansi = screen.to_ansi();
    assert!(ansi.contains("\x1b["), "it paints with SGR");
    assert!(ansi.contains("taskboard"), "and carries the text");

    let svg = screen.to_svg();
    assert!(svg.starts_with("<svg"), "{}", &svg[..60.min(svg.len())]);
    assert!(svg.contains("</svg>"));
    assert!(svg.contains("taskboard"), "the title bar is in the image");

    let html = screen.to_html();
    assert!(html.contains("<pre"), "an embeddable fragment");
    assert!(html.contains("taskboard"));

    // The ANSI rendering is the same picture, painted: strip the SGR and
    // every row is the row the screen holds.
    //
    // Deliberately *not* replayed through a second PTY. A 90x26 repaint is
    // tens of kilobytes and the tty input queue holds about four (README,
    // "a reply the terminal's own input queue cannot hold may not arrive"),
    // so writing one through `send_str` measures the kernel's buffer rather
    // than the rendering.
    let sgr = regex::Regex::new(r"\x1b\[[0-9;]*m").unwrap();
    let painted: Vec<String> = sgr
        .replace_all(&ansi, "")
        .lines()
        .map(|line| line.trim_end().to_owned())
        .collect();
    let held: Vec<String> = (0..screen.rows())
        .map(|row| screen.row_text(row).trim_end().to_owned())
        .collect();
    assert_eq!(
        painted, held,
        "the ANSI paints exactly the rows it came from"
    );
    Ok(())
}

// ============================================== 6. the serde feature (0.10)

/// A `Screen` leaves the test as JSON and comes back the same picture.
#[test]
fn a_screen_round_trips_through_json() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let json = serde_json::to_string(&screen).expect("a Screen serializes");
    let back: Screen = serde_json::from_str(&json).expect("and deserializes");

    assert!(screen.diff(&back).is_empty(), "{}", screen.diff(&back));
    assert_eq!(back.size(), screen.size());
    assert_eq!(back.cursor(), screen.cursor());

    // Styles survive, which is the half a text snapshot would have dropped.
    let (row, col) = screen.find("! Handle SIGWINCH").expect("the badge");
    assert!(back.cell(row, col).unwrap().style().blink);
    assert_eq!(back.cell(row, col).unwrap().style().fg, Color::Indexed(1));
    Ok(())
}

// ============================================== 7. Screen::parse (0.10)

/// The snapshot text format reads back: the format round-trips, which is
/// what lets a saved screen be diffed or rendered outside the run that
/// produced it.
#[test]
fn the_snapshot_text_format_round_trips() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();

    let saved = screen.with_styles().to_string();
    let parsed = Screen::parse(&saved)?;
    assert_eq!(parsed.with_styles().to_string(), saved, "byte-for-byte");
    assert!(screen.diff(&parsed).is_empty(), "{}", screen.diff(&parsed));

    // taskboard hides its cursor, and the format does not record where a
    // hidden cursor sat — which 0.10.0 reported as a difference nobody could
    // see, and 0.10.1 stopped reporting.
    assert!(!screen.cursor().2, "taskboard hides the cursor");
    assert_eq!(
        parsed.cursor(),
        (0, 0, false),
        "the position is not recorded"
    );
    Ok(())
}

/// Malformed input is an error naming the line, never an unwind — the CLI
/// shares this parser, so a panic here would take a consumer down.
#[test]
fn a_malformed_saved_screen_is_an_error_not_a_panic() {
    for bad in [
        "",
        "80x24",
        "size: 2x1  cursor: 9,9",
        "size: 2x1  cursor: 0,0\nabcdef",
        // Six *bytes* of hex that are not six hex digits: this unwound in
        // 0.10.0 and is an error in 0.10.1.
        "size: 2x2  cursor: 0,0\nx\n\nstyles:\n0: 0 fg=#a\u{20ac}bc",
    ] {
        let result = std::panic::catch_unwind(|| Screen::parse(bad));
        let parsed = result.unwrap_or_else(|_| panic!("parse panicked on {bad:?}"));
        assert!(parsed.is_err(), "{bad:?} should not parse");
    }
}

// ================================================ 8. recording (0.10)

/// Every complete frame from the moment `record()` is called, in order, with
/// the time each was completed.
#[test]
fn a_recording_holds_the_frames_an_interaction_produced() -> termlens::Result<()> {
    let mut t = spawn();
    let recorder = t.record();

    t.send(Key::Char('j'))?;
    t.wait_frame(|s| s.contains("Tasks 2/13"))?;
    t.send(Key::Char('j'))?;
    t.wait_frame(|s| s.contains("Tasks 3/13"))?;

    let recording = recorder.stop()?;
    assert!(recording.len() >= 2, "at least the two repaints");
    assert!(!recording.is_empty());
    assert_eq!(recording.dropped(), 0, "well inside the default budget");

    // Ordered, and each frame is a whole screen.
    let times: Vec<Duration> = recording.frames().iter().map(|(at, _)| *at).collect();
    let mut sorted = times.clone();
    sorted.sort_unstable();
    assert_eq!(times, sorted, "frames arrive in order");
    let last = &recording.frames().last().expect("a frame").1;
    assert!(last.contains("Tasks 3/13"), "{last}");
    Ok(())
}

/// The asciicast export is a *repaint* per frame. 0.10.0 ended each one with
/// the newline `to_ansi` puts after the bottom row, which scrolled the whole
/// picture up by one on replay; 0.10.1 drops exactly that newline.
#[test]
fn the_asciicast_export_does_not_scroll_the_frame_it_paints() -> termlens::Result<()> {
    let mut t = spawn();
    let recorder = t.record();
    t.send(Key::Char('j'))?;
    t.wait_frame(|s| s.contains("Tasks 2/13"))?;
    let recording = recorder.stop()?;

    let cast = recording.to_asciicast();
    let mut lines = cast.lines();
    let header: serde_json::Value =
        serde_json::from_str(lines.next().expect("a header")).expect("valid JSON");
    assert_eq!(header["version"], 2);
    assert_eq!(header["width"], i64::from(common::COLS));
    assert_eq!(header["height"], i64::from(common::ROWS));

    let event: serde_json::Value =
        serde_json::from_str(lines.next().expect("an event")).expect("valid JSON");
    assert_eq!(event[1], "o", "an output event");
    let data = event[2].as_str().expect("the payload");
    assert!(data.starts_with("\x1b[H\x1b[2J"), "home and clear first");
    assert!(
        !data.ends_with("\r\n"),
        "a trailing linefeed on the last row scrolls the frame away"
    );
    assert!(data.contains("q quit"), "the bottom row is painted");
    Ok(())
}

/// The budget is in cells, and a recording says how many frames it dropped
/// rather than quietly keeping the newest.
#[test]
fn the_record_budget_drops_the_oldest_and_says_so() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(common::COLS, common::ROWS)
        .env_clear()
        .timeout(Duration::from_secs(10))
        // Two frames' worth of cells, so a third displaces the first.
        .record_budget(2 * usize::from(common::COLS) * usize::from(common::ROWS))
        .spawn(env!("CARGO_BIN_EXE_taskboard"))?;
    t.wait_frame(|s| s.contains("NORMAL"))?;
    let recorder = t.record();
    for want in ["Tasks 2/13", "Tasks 3/13", "Tasks 4/13"] {
        t.send(Key::Char('j'))?;
        t.wait_frame(|s| s.contains(want))?;
    }
    let recording = recorder.stop()?;
    assert!(recording.len() <= 2, "bounded: {} frames", recording.len());
    assert!(recording.dropped() > 0, "and it says what it dropped");
    Ok(())
}

// ======================================== 9. the snapshot macro (0.10)

/// The macro now takes the terminal, waits for a predicate, settles, and
/// records styles — the three decisions every TUI snapshot needs, in one
/// line. `tui.rs` keeps the plain form; this is the new one.
#[test]
fn the_snapshot_macro_settles_and_keeps_styles() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('?'))?;
    termlens::assert_screen_snapshot!(t, after = |s| s.contains("move cursor"));
    Ok(())
}

// ================================= 10. what the emulator admits (0.10)

/// `unsupported()` is the honesty accessor: every sequence the emulator did
/// not implement, so a plausible-looking grid can be told from a right one.
///
/// **This pins a defect, reported upstream as termlens#320.** taskboard's
/// blinking badge and struck-through titles are both *observable* —
/// `hard.rs` asserts them — because the attribute shadow recovers exactly
/// these SGR parameters after vt100 drops them. They are nevertheless
/// reported here as unimplemented. `^[[59m` (underline colour) in the same
/// list is correct: nothing models it.
#[test]
fn unsupported_reports_sequences_the_shadow_parser_does_implement() -> termlens::Result<()> {
    let t = spawn();
    let screen = t.screen();
    let listed: Vec<&str> = screen.unsupported().iter().map(|q| &**q).collect();

    // The badge blinks — measured, not assumed.
    let (row, col) = screen.find("! Handle SIGWINCH").expect("the overdue badge");
    assert!(screen.cell(row, col).unwrap().style().blink, "it blinks");

    // And the sequence that made it blink is named as unimplemented.
    assert!(
        listed.contains(&"^[[5m"),
        "termlens#320 — if this fails the defect is fixed, and this pin \
         should become `assert!(!listed.contains(...))`: {listed:?}"
    );
    assert!(listed.contains(&"^[[59m"), "correctly reported: {listed:?}");
    assert_eq!(screen.unsupported_overflow(), 0, "well under the cap");
    assert_eq!(screen.visual_bells(), 0, "taskboard rings no visual bell");
    Ok(())
}

/// A well-behaved application leaves insert mode alone; an application that
/// left `IRM` on would push the rest of every row right, and this says so.
#[test]
fn insert_mode_is_off_for_an_application_that_never_sets_it() {
    let t = spawn();
    assert!(!t.screen().insert_mode(), "taskboard never sets IRM");
}

// ====================== 11. the 0.7 – 0.9 surface nothing here exercised

/// `bin!` (0.9) is the builder chain every integration test repeats, with a
/// compile-time-checked path.
#[test]
fn the_bin_macro_spawns_this_package_s_binary() -> termlens::Result<()> {
    let mut t = termlens::bin!("taskboard", size(common::COLS, common::ROWS))?;
    t.wait_frame(|s| s.contains("NORMAL"))?;
    assert_eq!(t.screen().size(), (common::COLS, common::ROWS));
    t.send(Key::Char('q'))?;
    assert!(t.wait_exit()?.success());
    Ok(())
}

/// `snapshot_after` (0.9) is the safe sequence for a whole-screen read:
/// predicate, then stillness, then the screen. `wait_stable` is the settle
/// on its own.
#[test]
fn snapshot_after_and_wait_stable_settle_the_picture() -> termlens::Result<()> {
    let mut t = spawn();
    t.send(Key::Char('?'))?;
    let overlay = t.snapshot_after(|s| s.contains("move cursor"))?;
    assert!(overlay.contains("help"), "{overlay}");

    // `wait_stable` is the settle on its own — it returns as soon as the
    // picture has held still, which for a screen that was *already* still is
    // immediately. It is not a wait for something to change, so the close is
    // waited for on its own terms first.
    t.send(Key::Esc)?;
    t.wait_frame(|s| !s.contains("move cursor"))?;
    let settled = t.wait_stable(Duration::from_millis(100))?;
    assert!(settled.contains("NORMAL"), "back to the list:\n{settled}");
    assert_eq!(settled, t.screen(), "still means still");
    Ok(())
}

/// `mouse_modes` (0.9) reports the whole set the application enabled, where
/// `mouse_mode` collapses it to the one termlens will encode for.
#[test]
fn mouse_modes_reports_every_mode_the_application_enabled() {
    let t = spawn();
    let screen = t.screen();
    let modes = screen.mouse_modes();
    assert!(!modes.is_empty(), "taskboard enables mouse tracking");
    assert_ne!(screen.mouse_mode(), termlens::MouseMode::None);
    println!(
        "--- mouse modes --- {modes:?} -> encoding for {:?}",
        screen.mouse_mode()
    );
}

/// `cursor_shape` / `cursor_blink` (0.7): two facts from one `DECSCUSR`
/// parameter, reported apart because a test usually wants only one.
#[test]
fn the_cursor_shape_and_its_blink_are_reported_apart() {
    let t = spawn();
    let screen = t.screen();
    println!(
        "--- cursor --- shape={:?} blink={:?} visible={}",
        screen.cursor_shape(),
        screen.cursor_blink(),
        screen.cursor().2
    );
    assert!(!screen.cursor().2, "a list view hides the cursor");
}

/// `Screen: PartialEq` (0.8) compares the whole observation — cells, cursor,
/// size *and* the out-of-band counters — so two visually identical snapshots
/// either side of a bell are unequal. `diff().is_empty()` is the picture.
#[test]
fn screen_equality_is_the_observation_and_diff_is_the_picture() -> termlens::Result<()> {
    let t = spawn();
    let a = t.screen();
    let b = t.screen();
    assert_eq!(a, b, "two reads of a quiescent terminal");
    assert!(a.diff(&b).is_empty());
    Ok(())
}

/// `envs` (0.7) sets several variables at once; the child reads what it was
/// given and nothing from the machine.
#[test]
fn envs_sets_several_variables_and_env_clear_leaks_nothing() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(60, 6)
        .env_clear()
        .envs([("DEMO_ONE", "alpha"), ("DEMO_TWO", "beta")])
        .timeout(Duration::from_secs(5))
        .args([
            "-c",
            "printf '%s-%s-%s\\n' \"$DEMO_ONE\" \"$DEMO_TWO\" \"${HOME:-nohome}\"; read x",
        ])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("alpha-beta"))?;
    assert!(t.screen().contains("alpha-beta-nohome"), "{}", t.screen());
    Ok(())
}
