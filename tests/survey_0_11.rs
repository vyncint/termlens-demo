//! What termlens **0.11** added, probed against the real binary rather than
//! inferred from the crate.
//!
//! 0.11 is termlens's **stability candidate**: from it, no item its
//! `docs/STABILITY.md` promises changes incompatibly before its 1.0. That
//! makes this survey a different kind of document from the ones before it.
//! The earlier surveys asked "what is new, and is it true?"; this one also
//! asks "is what was just frozen the thing a consumer can actually build
//! on?" — so every probe here is written the way a user would write it,
//! against the published crate, and a failure is a reason to reconsider the
//! freeze rather than to work around it.
//!
//! The surface: the `Unsupported` view (the release's one breaking change),
//! `cursor_visible`, the `Location` accessors, `ScreenDiff::changed_rows`
//! and `style_changes`, `Display for Color`, the nameable
//! `ScreenWithStyles`, and the versioned JSON. `tests/cli.rs` covers the
//! other half of the release — `inspect`'s stdout became a saved screen —
//! because that one needs the installed binary.
//!
//! Everything under test comes from crates.io.

use std::time::Duration;

use termlens::{Color, Key, Location, Screen, ScreenWithStyles};

mod common;

use common::{spawn, spawn_sh};

// ============================== 1. the Unsupported view (the one break)

/// `unsupported()` returns a view rather than the `Arc<str>` slice it used
/// to, and `unsupported_overflow()` is gone. The whole surface, exercised
/// the way the migration table says to write it.
///
/// The interesting property is the equality: it holds only when the
/// retained shapes match **and** nothing overflowed the bound. Under 0.10
/// "this list exactly, and nothing was dropped" took two calls and it was
/// possible to write only the first; here the short form is the strict one.
#[test]
fn the_unsupported_view_carries_the_overflow_count_with_the_list() {
    let screen = spawn().screen();
    let view = screen.unsupported();

    // taskboard emits exactly one sequence nothing models: ratatui's
    // underline-colour reset, which changes no cell.
    assert_eq!(view, ["^[[59m"], "{view:?}");
    assert_eq!(view.len(), 1);
    assert!(!view.is_empty());
    assert_eq!(view.overflow(), 0);
    assert!(view.contains("^[[59m"));
    assert!(!view.contains("^[[5m"), "the shadow recovers blink");

    // Three ways to read the same list, all plain `&str` — no `&**q` dance.
    assert_eq!(view.iter().collect::<Vec<&str>>(), ["^[[59m"]);
    assert_eq!(view.into_iter().collect::<Vec<&str>>(), ["^[[59m"]);
    assert_eq!(format!("{view:?}"), r#"["^[[59m"]"#);

    // It is Copy and borrows the screen, so it can be taken twice and
    // compared, which is what lets an assertion read as one expression.
    let again = screen.unsupported();
    assert_eq!(view, again);
    assert_eq!(view.iter().count(), again.len());
}

/// The half of the equality no application here can produce on its own: a
/// record that overflowed. Driven with hand-written escapes so the bound is
/// reached on purpose — 40 distinct unimplemented modes against a retention
/// of 32.
///
/// This is the case the old two-call shape could get wrong. `is_empty()` is
/// false, the equality against the retained list is false, and `Debug` says
/// how many were lost.
#[test]
fn a_record_past_the_bound_is_not_equal_to_the_part_of_it_that_fits() -> termlens::Result<()> {
    let mut script = String::from("printf '");
    for mode in 20..60u16 {
        script.push_str(&format!("\\033[{mode}h"));
    }
    script.push_str("READY'; head -c 1 >/dev/null");
    let mut t = spawn_sh(&script, Duration::from_secs(10));
    t.wait_until(|s| s.contains("READY"))?;
    let screen = t.screen();
    let view = screen.unsupported();

    assert_eq!(view.len(), 32, "the retention bound: {view:?}");
    assert_eq!(view.overflow(), 8, "and the rest are counted: {view:?}");
    assert!(!view.is_empty());

    // The list of what was kept is *not* the record, and the view says so.
    let kept: Vec<&str> = view.iter().collect();
    assert_eq!(kept.len(), 32);
    assert_ne!(
        view,
        kept.as_slice(),
        "eight shapes are missing from `kept`"
    );
    assert!(
        format!("{view:?}").ends_with("] (+8 more)"),
        "Debug names the loss: {view:?}"
    );
    // A shape past the bound is counted, not findable.
    assert!(!view.contains("^[[59h"), "{view:?}");
    Ok(())
}

/// An empty array is the whole "nothing was unsupported" assertion, and it
/// is strict: a screen with a shape retained, or with an overflow, is not
/// equal to it.
#[test]
fn an_empty_array_pins_a_clean_record() -> termlens::Result<()> {
    let mut t = spawn_sh(
        "printf 'plain text only'; head -c 1 >/dev/null",
        Duration::from_secs(10),
    );
    t.wait_until(|s| s.contains("plain text only"))?;
    assert_eq!(
        t.screen().unsupported(),
        [],
        "{:?}",
        t.screen().unsupported()
    );
    assert!(t.screen().unsupported().is_empty());

    // taskboard's is not clean, and the same comparison says so.
    assert_ne!(spawn().screen().unsupported(), []);
    Ok(())
}

// ================================================ 2. the small accessors

/// `cursor_visible()` is the third element of `cursor()` on its own. The
/// tuple is unchanged — this is additive — but an assertion about
/// visibility now reads as one.
#[test]
fn cursor_visible_says_at_the_call_site_what_cursor_2_did_not() {
    let screen = spawn().screen();
    let (row, col, visible) = screen.cursor();
    assert_eq!(screen.cursor_visible(), visible, "the same fact, named");
    assert!(
        !screen.cursor_visible(),
        "taskboard is a list view and hides the cursor"
    );
    // The position is still reported for a hidden cursor; it is simply not
    // part of the picture.
    let _ = (row, col);
    assert!(
        screen
            .to_string()
            .starts_with("size: 90x26  cursor: hidden")
    );
}

/// `Location` answers "screen or history?" and "what column?" without a
/// `match`. Deliberately no `row()`: a grid row and a history row are
/// different things, and this asserts both halves of that.
#[test]
fn location_answers_the_one_fact_questions() -> termlens::Result<()> {
    // A screen with history: more lines than rows, so the first ones scroll.
    // Both needles are unique strings rather than prefixes — `line 1` would
    // also match `line 10` on the grid, and the grid wins a tie.
    let mut t = spawn_sh(
        "printf '  SCROLLED-AWAY\\n'; \
         i=1; while [ $i -le 40 ]; do echo \"line $i\"; i=$((i+1)); done; \
         printf 'STILL-ON-SCREEN\\n'; head -c 1 >/dev/null",
        Duration::from_secs(10),
    );
    t.wait_until(|s| s.contains("STILL-ON-SCREEN"))?;
    let screen = t.screen();

    let here = screen.locate("STILL-ON-SCREEN").expect("on the grid");
    assert!(here.is_on_screen() && !here.is_in_history(), "{here:?}");
    assert_eq!(here.col(), 0);

    let gone = screen.locate("SCROLLED-AWAY").expect("scrolled off");
    assert!(gone.is_in_history() && !gone.is_on_screen(), "{gone:?}");
    // The column is the one fact both regions report the same way: a real
    // column on the grid, the display column of the row *as captured* in
    // history. Two leading spaces, so this is not incidentally zero.
    assert_eq!(gone.col(), 2, "{gone:?}");

    // The accessors agree with the variants they replace reading.
    assert!(matches!(here, Location::Screen { .. }));
    assert!(matches!(gone, Location::History { .. }));
    assert!(screen.locate("no such text").is_none());
    Ok(())
}

// ========================================= 3. what a ScreenDiff exposes

/// `ScreenDiff` computed which rows and which style runs changed and
/// exposed neither; 0.11 added both. The assertion this was added for —
/// "the highlight moved and nothing else changed" — is now an API call
/// rather than a dedup over `cells()` or a substring of the rendering.
#[test]
fn a_diff_reports_its_changed_rows_and_style_runs() -> termlens::Result<()> {
    let mut t = spawn();
    let before = t.screen();
    t.send(Key::Down)?;
    let after = t.wait_frame(|s| s.contains("NORMAL"))?;
    let diff = before.diff(&after);
    assert!(!diff.is_empty(), "moving the selection changes the picture");

    let rows: Vec<u16> = diff.changed_rows().collect();
    assert!(!rows.is_empty(), "{diff}");
    assert!(
        rows.windows(2).all(|w| w[0] < w[1]),
        "ascending, no dupes: {rows:?}"
    );

    // Every changed row is one that `cells()` also reports, and every row
    // `cells()` reports is in the list — the same fact, without the dance.
    let mut from_cells: Vec<u16> = diff.cells().map(|(row, ..)| row).collect();
    from_cells.dedup();
    assert_eq!(rows, from_cells);

    // Moving a highlight is a style change, and the runs are readable as
    // the tokens `with_styles` writes.
    let styles: Vec<(u16, &str, &str)> = diff.style_changes().collect();
    assert!(!styles.is_empty(), "the selection is styled:\n{diff}");
    for (row, from, to) in &styles {
        assert!(rows.contains(row), "a style change is on a changed row");
        assert_ne!(from, to);
    }

    // A screen against itself reports nothing on either accessor.
    let same = before.diff(&before);
    assert_eq!(same.changed_rows().count(), 0);
    assert_eq!(same.style_changes().count(), 0);
    t.send(Key::Char('q'))?;
    Ok(())
}

// ==================================== 4. Color, and the nameable rendering

/// `Color` implements `Display`, producing exactly the token the `styles:`
/// block writes — so the two halves of a documented format sit together,
/// and a test can print a colour in its own message.
///
/// The round trip is the claim worth measuring: the token this writes is
/// the token `Screen::parse` reads back.
#[test]
fn a_colour_prints_as_the_token_the_snapshot_format_uses() -> termlens::Result<()> {
    assert_eq!(Color::Indexed(4).to_string(), "4");
    assert_eq!(Color::Rgb(0x1e, 0x1e, 0x2e).to_string(), "#1e1e2e");
    assert_eq!(Color::Default.to_string(), "default");

    // Against a real screen: every colour the application draws renders as a
    // token that appears in the styles block, and parses back as itself.
    let screen = spawn().screen();
    let styled = screen.with_styles().to_string();
    let block = styled.split("styles:").nth(1).expect("a styles block");

    let mut seen = 0;
    for row in 0..screen.rows() {
        for col in 0..screen.cols() {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            if cell.style().fg == Color::Default {
                continue;
            }
            let token = format!("fg={}", cell.style().fg);
            assert!(block.contains(&token), "{token} is in the block:\n{block}");
            seen += 1;
        }
    }
    assert!(seen > 0, "taskboard colours something");

    let parsed = Screen::parse(&styled)?;
    assert_eq!(parsed.with_styles().to_string(), styled, "byte for byte");
    Ok(())
}

/// `ScreenWithStyles` is exported, so the rendering can be stored, returned
/// from a helper or taken as a parameter. Under 0.10 the type was reachable
/// and unnameable: `with_styles()` could only be passed straight on.
#[test]
fn the_styled_rendering_is_a_type_a_consumer_can_name() {
    fn styled(screen: &Screen) -> ScreenWithStyles<'_> {
        screen.with_styles()
    }
    struct Held<'a> {
        rendering: ScreenWithStyles<'a>,
    }

    let screen = spawn().screen();
    let held = Held {
        rendering: styled(&screen),
    };
    assert_eq!(held.rendering.to_string(), screen.with_styles().to_string());
    assert!(held.rendering.to_string().contains("styles:"));
}

// =================================================== 5. the versioned JSON

/// The JSON a `Screen` serialises to carries a format number, first in the
/// document. It is a persisted artifact — `termlens diff` and `render` read
/// it back — so it says which shape it is rather than leaving a reader to
/// guess from the fields.
#[test]
fn the_json_says_which_format_it_is_and_still_reads_a_file_without_one() {
    let screen = spawn().screen();
    let json = serde_json::to_string(&screen).expect("serialises");
    assert!(json.starts_with("{\"format\":1,"), "{}", &json[..40]);

    let back: Screen = serde_json::from_str(&json).expect("round trips");
    assert_eq!(back, screen, "an equal Screen, out-of-band state included");

    // A file written by 0.10, which had no such field: it *is* format 1.
    let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(value.as_object_mut().unwrap().remove("format").is_some());
    let old: Screen = serde_json::from_value(value.clone()).expect("a 0.10 file still reads");
    assert_eq!(old, screen);

    // A file written by some later termlens: refused, naming both numbers,
    // rather than read as far as the fields happen to line up.
    value["format"] = serde_json::json!(2);
    let err = serde_json::from_value::<Screen>(value)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("format 2") && err.contains("reads format 1"),
        "{err}"
    );
}
