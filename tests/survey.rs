//! A broad survey of what termlens can and cannot observe, probed against
//! plain `/bin/sh` so each finding is isolated from any application.
//! Every claim is reproduced against a real process, never inferred.

mod common;

use std::time::Duration;

use common::spawn_sh;
use termlens::{Color, Key, MouseMode, Screen, Style, Terminal};

fn sh(script: &str) -> Terminal {
    spawn_sh(script, Duration::from_secs(2))
}

/// A TUI-like child: raw mode, no echo, blocks on one byte.
fn raw(script: &str) -> Terminal {
    spawn_sh(
        &format!("stty raw -echo; {script}; head -c 1 >/dev/null"),
        Duration::from_secs(2),
    )
}

fn style_of(s: &Screen, needle: &str) -> Style {
    let (r, c) = s.find(needle).unwrap_or_else(|| panic!("{needle} missing\n{s}"));
    *s.cell(r, c).unwrap().style()
}

// ============================================================ A. style model

#[test]
fn a1_strikethrough_and_blink_are_not_in_the_style_model() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[9mSTRIKE\033[0m \033[5mBLINK\033[0m \033[1mBOLD\033[0m'");
    t.wait_until(|s| s.contains("BOLD"))?;
    let s = t.screen();

    assert_eq!(style_of(&s, "STRIKE"), Style::default(), "strikethrough tracked?");
    assert_eq!(style_of(&s, "BLINK"), Style::default(), "blink tracked?");
    assert!(style_of(&s, "BOLD").bold, "sanity: bold is tracked");
    // and therefore invisible in a styled snapshot
    let dump = s.with_styles().to_string();
    println!("--- a1 styles ---\n{dump}");
    Ok(())
}

#[test]
fn a2_conceal_sgr8_still_renders_the_text() -> termlens::Result<()> {
    let mut t = raw(r"printf 'pw: \033[8mhunter2\033[0m END'");
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    assert!(s.contains("hunter2"), "concealed text is in the grid:\n{s}");
    assert_eq!(style_of(&s, "hunter2"), Style::default());
    Ok(())
}

#[test]
fn a3_underline_variants_and_colors_collapse() -> termlens::Result<()> {
    let mut t = raw(
        r"printf 'A\033[4mSINGLE\033[0m B\033[4:3mCURLY\033[0m C\033[21mDOUBLE\033[0m D\033[4m\033[58;5;9mCOLORED\033[0m END'",
    );
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    println!(
        "single={:?}\ncurly={:?}\ndouble={:?}\ncolored={:?}",
        style_of(&s, "SINGLE"),
        style_of(&s, "CURLY"),
        style_of(&s, "DOUBLE"),
        style_of(&s, "COLORED"),
    );
    println!("--- a3 text ---\n{s}");
    Ok(())
}

#[test]
fn a4_overline_and_bright_variants() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[53mOVER\033[0m \033[91mBRIGHTRED\033[0m \033[38;5;9mIDX9\033[0m END'");
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    println!(
        "over={:?} bright={:?} idx9={:?}",
        style_of(&s, "OVER"),
        style_of(&s, "BRIGHTRED"),
        style_of(&s, "IDX9")
    );
    Ok(())
}

// ==================================================== B. outside the model

#[test]
fn b1_osc8_hyperlink_target_is_unobservable() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033]8;;https://example.com/secret\033\\CLICK ME\033]8;;\033\\ END'");
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    assert!(s.contains("CLICK ME"));
    assert!(!s.contains("example.com"), "URL nowhere in the grid:\n{s}");
    assert_eq!(s.title(), "", "and it is not the title either");
    Ok(())
}

#[test]
fn b2_osc52_clipboard_write_is_unobservable() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033]52;c;aGVsbG8=\033\\COPIED'");
    t.wait_until(|s| s.contains("COPIED"))?;
    let s = t.screen();
    assert!(!s.contains("aGVsbG8"), "{s}");
    assert_eq!(s.title(), "");
    Ok(())
}

#[test]
fn b3_bell_is_unobservable() -> termlens::Result<()> {
    let mut t = raw(r"printf 'before\007after'");
    t.wait_until(|s| s.contains("after"))?;
    let s = t.screen();
    println!("--- b3 ---\n{s:?}");
    assert!(s.contains("beforeafter") || s.contains("before after"), "{s}");
    Ok(())
}

#[test]
fn b4_cursor_shape_decscusr_is_unobservable() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[5 qBAR'");
    t.wait_until(|s| s.contains("BAR"))?;
    let s = t.screen();
    println!("cursor={:?}", s.cursor());
    Ok(())
}

#[test]
fn b5_osc1_icon_name_is_ignored_and_title_never_resets() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033]1;icon-only\007\033]2;real title\007SET'");
    t.wait_until(|s| s.contains("SET"))?;
    assert_eq!(t.screen().title(), "real title");
    Ok(())
}

// ========================================================= C. text fidelity

#[test]
fn c1_trailing_whitespace_is_invisible_in_text_and_contains() -> termlens::Result<()> {
    let mut t = raw(r"printf 'padded   \r\nnext'");
    t.wait_until(|s| s.contains("next"))?;
    let s = t.screen();
    assert!(!s.contains("padded   "), "trailing spaces trimmed from text()");
    assert!(s.row_text(0).starts_with("padded   "), "row_text keeps them");
    // an all-blank row and a row of spaces are the same string
    assert_eq!(s.text().lines().nth(2).unwrap_or(""), "");
    Ok(())
}

#[test]
fn c2_hidden_cursor_drops_its_position_from_the_snapshot() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?25l\033[5;10HHIDDEN'");
    t.wait_until(|s| s.contains("HIDDEN"))?;
    let s = t.screen();
    let (row, col, visible) = s.cursor();
    assert!(!visible);
    assert_eq!((row, col), (4, 15), "position is still tracked");
    assert!(s.to_string().starts_with("size: 80x24  cursor: hidden"), "{s}");
    Ok(())
}

#[test]
fn c3_zwj_emoji_and_combining_marks() -> termlens::Result<()> {
    let mut t = raw("printf 'A👨‍👩‍B🇻🇳C❤️D END'");
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    println!("--- c3 row0 --- {:?}", s.row_text(0).trim_end());
    for col in 0..16 {
        let c = s.cell(0, col).unwrap();
        println!(
            "col {col}: {:?} wide={} cont={}",
            c.contents(),
            c.is_wide(),
            c.is_wide_continuation()
        );
    }
    Ok(())
}

#[test]
fn c5_nfd_text_does_not_match_an_nfc_needle() -> termlens::Result<()> {
    // The app prints a decomposed 'é' (e + U+0301), as plenty of real data
    // and every macOS filename does.
    let mut t = raw("printf 'caf\u{65}\u{301} END'");
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    assert!(!s.contains("café"), "the obvious needle silently misses:\n{s}");
    assert!(s.contains("caf\u{65}\u{301}"), "only the decomposed form hits");
    Ok(())
}

#[test]
fn c6_tab_expansion() -> termlens::Result<()> {
    let mut t = raw(r"printf 'a\tb\tEND'");
    t.wait_until(|s| s.contains("END"))?;
    println!("--- c6 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

#[test]
fn c7_env_clear_leaves_the_child_with_no_locale_or_path() -> termlens::Result<()> {
    let mut t = spawn_sh(
        r#"printf 'LANG=[%s] LC_ALL=[%s] PATH=[%s] TERM=[%s]\n' "$LANG" "$LC_ALL" "$PATH" "$TERM"; read x"#,
        Duration::from_secs(2),
    );
    t.wait_until(|s| s.contains("TERM="))?;
    println!("--- c7 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

#[test]
fn c4_screen_has_no_partial_eq_and_string_compare_ignores_style() -> termlens::Result<()> {
    let mut t = raw(r"printf 'HELLO'; sleep 0.15; printf '\033[H\033[31mHELLO\033[0m'");
    t.wait_until(|s| s.contains("HELLO"))?;
    let before = t.screen();
    t.wait_until(|s| s.cell(0, 0).unwrap().style().fg == Color::Indexed(1))?;
    let after = t.screen();
    assert_eq!(
        before.to_string(),
        after.to_string(),
        "identical text: a color-only change is invisible to a string compare"
    );
    assert_ne!(before.with_styles().to_string(), after.with_styles().to_string());
    Ok(())
}

// =========================================================== D. modes/state

#[test]
fn d1_mouse_encoding_is_not_observable() -> termlens::Result<()> {
    let mut legacy = raw(r"printf '\033[?1000hLEGACY'");
    legacy.wait_until(|s| s.contains("LEGACY"))?;
    let mut sgr = raw(r"printf '\033[?1000h\033[?1006hSGR'");
    sgr.wait_until(|s| s.contains("SGR"))?;

    assert_eq!(legacy.screen().mouse_mode(), MouseMode::PressRelease);
    assert_eq!(sgr.screen().mouse_mode(), MouseMode::PressRelease);
    // Same reported mode; the wire encoding differs and nothing exposes it.
    Ok(())
}

#[test]
fn d2_focus_reporting_mode_1004_is_neither_tracked_nor_sendable() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?1004hFOCUSMODE'");
    t.wait_until(|s| s.contains("FOCUSMODE"))?;
    let s = t.screen();
    // No accessor exists; the only observable modes are these:
    println!(
        "alt={} paste={} appcursor={} mouse={:?}",
        s.alternate_screen(),
        s.bracketed_paste(),
        s.application_cursor(),
        s.mouse_mode()
    );
    Ok(())
}

#[test]
fn d3_decrqm_probe_for_2026_is_neither_answered_nor_named() {
    // The standard way an app asks "do you support synchronized output?"
    let mut t = sh(r#"printf '\033[?2026$p'; read -r r; printf 'GOT'; read x"#);
    let err = t.wait_until(|s| s.contains("GOT")).expect_err("no reply");
    let msg = err.to_string();
    println!("--- d3 message ---\n{msg}");
    assert!(!msg.contains("queried the terminal"), "not even diagnosed: {msg}");
}

#[test]
fn d3b_kitty_probe_is_named_for_contrast() {
    let mut t = sh(r#"printf '\033[?u'; read -r r; printf 'GOT'; read x"#);
    let err = t.wait_until(|s| s.contains("GOT")).expect_err("no reply");
    assert!(err.to_string().contains("queried the terminal"), "{err}");
}

#[test]
fn d4_osc10_foreground_is_always_white() -> termlens::Result<()> {
    // `min 0 time 10` = return whatever arrived within 1s, so a reply with
    // no newline can still be captured.
    let script = r#"stty raw -echo min 0 time 10
printf '\033]11;?\033\\'
BG=$(dd bs=1 count=64 2>/dev/null | tr -d '\033\\')
printf '\033]10;?\033\\'
FG=$(dd bs=1 count=64 2>/dev/null | tr -d '\033\\')
stty sane
printf 'BG=<%s>\r\nFG=<%s>\r\nDONE\r\n' "$BG" "$FG"
head -c 1 >/dev/null"#;
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .background_rgb(0x1e, 0x1e, 0x2e)
        .args(["-c", script])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- d4 ---\n{}", t.screen());
    Ok(())
}

#[test]
fn d5_pixel_size_query_is_unanswerable() {
    let mut t = sh(r#"printf '\033[14t'; read -r r; printf 'GOT'; read x"#);
    let err = t.wait_until(|s| s.contains("GOT")).expect_err("no reply");
    println!("--- d5 ---\n{err}");
}

// ============================================================= E. wait_frame

#[test]
fn e1_wait_frame_matches_a_stale_frame_after_resize() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?2026h\033[2J\033[HFRAME-1\033[?2026l'");
    t.wait_frame(|s| s.contains("FRAME-1"))?;

    t.resize(40, 10)?;
    // No new frame can exist: the child never repaints. This still passes.
    t.wait_frame(|s| {
        println!("frame seen after resize: {}x{}", s.cols(), s.rows());
        s.contains("FRAME-1") && s.cols() == 80
    })?;
    assert_eq!(t.screen().cols(), 40, "live screen did resize");
    Ok(())
}

#[test]
fn e2_wait_frame_can_resolve_on_a_frame_older_than_the_keystroke() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?2026h\033[2J\033[HSTATE-A\033[?2026l'");
    t.wait_frame(|s| s.contains("STATE-A"))?;

    t.send(Key::Char('x')); // the child never repaints in response
    // A test asserting "still in STATE-A after the key" passes vacuously.
    t.wait_frame(|s| s.contains("STATE-A"))?;
    Ok(())
}

#[test]
fn e3_the_frame_that_matched_is_unreachable_screen_is_a_later_instant() -> termlens::Result<()> {
    let mut t = raw(
        r"printf '\033[?2026h\033[2J\033[HFRAME-DONE\033[?2026l'; sleep 0.1; printf '\033[?2026h\033[2J\033[HTORN-HALF'",
    );
    t.wait_until(|s| s.contains("TORN-HALF"))?;

    // The retained complete frame still matches...
    t.wait_frame(|s| s.contains("FRAME-DONE"))?;
    // ...but the only screen you can read is the half-painted one.
    let s = t.screen();
    assert!(s.contains("TORN-HALF"), "{s}");
    assert!(!s.contains("FRAME-DONE"), "the matched frame is gone:\n{s}");
    Ok(())
}

// ============================================================== F. process

#[test]
fn f1_signal_death_reports_a_string_not_a_number() -> termlens::Result<()> {
    let mut t = sh("printf 'UP\\n'; while :; do sleep 0.05; done");
    t.wait_until(|s| s.contains("UP"))?;
    t.signal(termlens::Signal::Kill)?;
    let status = t.wait_exit()?;
    println!(
        "--- f1 --- code={} success={} signal={:?} display={status}",
        status.code(),
        status.success(),
        status.signal()
    );
    Ok(())
}

#[test]
fn f2_send_after_exit() -> termlens::Result<()> {
    let mut t = sh("printf 'BYE\\n'");
    t.wait_until(|s| s.contains("BYE"))?;
    let status = t.wait_exit()?;
    println!("exited: {status}");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        t.send(Key::Char('z'));
    }));
    println!("--- f2 --- send after exit panicked: {}", result.is_err());
    Ok(())
}

#[test]
fn f3_char_above_ascii_is_sent_as_utf8_not_a_raw_byte() -> termlens::Result<()> {
    let mut t = sh(
        r#"stty raw -echo; printf 'BYTES='; od -An -tx1 -N3 | tr -d '\n'; printf '\r\nDONE\r\n'; head -c 1 >/dev/null"#,
    );
    t.wait_until(|s| s.contains("BYTES="))?;
    t.send(Key::Char('\u{80}')); // one codepoint...
    t.send(Key::Char('!'));
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- f3 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

#[test]
fn f6_sigterm_signal_string() -> termlens::Result<()> {
    let mut t = sh("printf 'UP\\n'; while :; do sleep 0.05; done");
    t.wait_until(|s| s.contains("UP"))?;
    t.signal(termlens::Signal::Term)?;
    let status = t.wait_exit()?;
    println!(
        "--- f6 --- code={} signal={:?} display={status}",
        status.code(),
        status.signal()
    );
    Ok(())
}

#[test]
fn f7_send_after_exit_is_a_silent_no_op() -> termlens::Result<()> {
    let mut t = sh("printf 'BYE\\n'");
    t.wait_until(|s| s.contains("BYE"))?;
    t.wait_exit()?;
    // The doc contract says this panics with the screen attached.
    for _ in 0..5 {
        t.send(Key::Char('z'));
        t.send_str("typed into the void");
    }
    println!("--- f7 --- 10 writes to a dead child, no panic, no error");
    Ok(())
}

// ============================================================== H. more gaps

#[test]
#[should_panic(expected = "no CSI-modifier chord form")]
fn h1_ctrl_enter_is_unsendable() {
    let _ = Key::Enter.ctrl();
}

#[test]
#[should_panic(expected = "no CSI-modifier chord form")]
fn h2_shift_tab_as_a_chord_is_unsendable() {
    let _ = Key::Tab.shift();
}

#[test]
#[should_panic(expected = "only F1-F12")]
fn h3_f13_and_above_are_unsendable() {
    let _ = Key::F(13).encode();
}

#[test]
fn h4_app_initiated_resize_csi_8t_is_silently_ignored() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[8;10;40tRESIZE-ASKED'");
    t.wait_until(|s| s.contains("RESIZE-ASKED"))?;
    assert_eq!(t.screen().size(), (80, 24), "the request did nothing");
    Ok(())
}

#[test]
fn h5_osc4_palette_redefinition_is_ignored() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033]4;1;rgb:00/ff/00\033\\\033[31mRED-IS-NOW-GREEN\033[0m END'");
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    assert_eq!(
        style_of(&s, "RED-IS-NOW-GREEN").fg,
        Color::Indexed(1),
        "still reported as palette slot 1, whatever it now renders as"
    );
    Ok(())
}

#[test]
fn h6_wide_char_at_the_last_column_vanishes() -> termlens::Result<()> {
    // A double-width glyph with only one column left, then a marker on row 3
    // so nothing can overwrite where it might have wrapped to.
    let mut t = raw(r"printf '\033[1;80H漢\033[3;1HMARKER'");
    t.wait_until(|s| s.contains("MARKER"))?;
    let s = t.screen();
    println!("--- h6 --- row0={:?} row1={:?}", s.row_text(0).trim_end(), s.row_text(1).trim_end());
    for col in 78..80 {
        let c = s.cell(0, col).unwrap();
        println!("col {col}: {:?} wide={} cont={}", c.contents(), c.is_wide(), c.is_wide_continuation());
    }
    // Correct xterm behaviour: it wraps whole to the next row.
    assert_eq!(s.row_text(1).trim_end(), "漢");
    Ok(())
}

#[test]
fn h11_paste_sends_lf_where_a_real_terminal_sends_cr() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?2004hPASTE='; od -An -c -N10 | tr -d '\n'; printf '\r\nDONE\r\n'");
    t.wait_until(|s| s.contains("PASTE="))?;
    t.paste("a\nb");
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- h11 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

#[test]
fn h9_a_hang_inside_a_synchronized_update_makes_wait_idle_impossible() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?2026hHALF-PAINTED'");
    t.wait_until(|s| s.contains("HALF-PAINTED"))?;
    // The stream has been silent for ages, but the frame never closed.
    let err = t
        .wait_idle(Duration::from_millis(20))
        .expect_err("never idle");
    println!("--- h9 --- {err}");
    Ok(())
}

#[test]
fn h10_light_background_makes_the_terminal_claim_white_on_white() -> termlens::Result<()> {
    let script = r#"stty raw -echo min 0 time 10
printf '\033]11;?\033\\'; BG=$(dd bs=1 count=64 2>/dev/null | tr -d '\033\\')
printf '\033]10;?\033\\'; FG=$(dd bs=1 count=64 2>/dev/null | tr -d '\033\\')
stty sane; printf 'BG=<%s>\r\nFG=<%s>\r\nDONE\r\n' "$BG" "$FG"; head -c 1 >/dev/null"#;
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .background_rgb(0xff, 0xff, 0xff) // a light theme
        .args(["-c", script])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- h10 ---\n{}", t.screen().rect_text(.., ..3));
    Ok(())
}

#[test]
fn h7_alternate_screen_round_trip_preserves_the_primary() -> termlens::Result<()> {
    let mut t = raw(
        r"printf 'PRIMARY-TEXT'; sleep 0.05; printf '\033[?1049h\033[2J\033[HALT-TEXT'; sleep 0.15; printf '\033[?1049l'",
    );
    t.wait_until(|s| s.contains("ALT-TEXT"))?;
    assert!(t.screen().alternate_screen());
    t.wait_until(|s| !s.alternate_screen())?;
    let s = t.screen();
    println!("--- h7 --- after leaving alt: {:?}", s.row_text(0).trim_end());
    Ok(())
}

#[test]
fn h8_no_way_to_ask_whether_wait_frame_can_work() -> termlens::Result<()> {
    // frames_seen is internal: the only way to learn an app emits DEC 2026
    // is to call wait_frame and burn the full timeout when it doesn't.
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_millis(400))
        .args(["-c", "stty raw -echo; printf 'NO-SYNC'; head -c 1 >/dev/null"])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("NO-SYNC"))?;
    let start = std::time::Instant::now();
    let err = t.wait_frame(|_| true).expect_err("never emits frames");
    println!("--- h8 --- burned {:?} to find out: {}", start.elapsed(), err);
    Ok(())
}

#[test]
fn f4_resize_is_destructive_not_clipping() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[HLEFT\033[1;60HRIGHTMOST'");
    t.wait_until(|s| s.contains("RIGHTMOST"))?;
    t.resize(40, 24)?;
    assert!(!t.screen().contains("RIGHTMOST"));
    t.resize(80, 24)?;
    let s = t.screen();
    assert!(!s.contains("RIGHTMOST"), "growing back does not restore it:\n{s}");
    assert!(s.contains("LEFT"));
    Ok(())
}

#[test]
fn f5_no_reflow_on_narrowing() -> termlens::Result<()> {
    let mut t = raw(r"printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaTAILEND'");
    t.wait_until(|s| s.contains("TAILEND"))?;
    t.resize(20, 24)?;
    let s = t.screen();
    println!("--- f5 ---\n{s}");
    assert!(!s.contains("TAILEND"), "no reflow onto a second row");
    Ok(())
}

// ============================================================ G. ergonomics

#[test]
fn g1_click_takes_col_row_while_find_returns_row_col() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?1000h\033[?1006h\033[3;20HTARGET'");
    t.wait_until(|s| s.contains("TARGET"))?;
    let (row, col) = t.screen().find("TARGET").unwrap();
    println!("find -> (row={row}, col={col})");
    // The type system accepts the swapped call, which clicks somewhere else.
    t.click(row, col)?;
    t.click(col, row)?;
    Ok(())
}

#[test]
fn g2_query_reply_is_painted_onto_the_screen_in_cooked_mode() -> termlens::Result<()> {
    let mut t = sh(r#"printf 'prompt> \033[6n'; read -r r; printf 'DONE\n'; read x"#);
    // The app never gets past `read`: the reply carries no newline, so a
    // cooked-mode reader blocks — and the line discipline echoes the reply
    // into the rendered screen.
    t.wait_until(|s| s.contains("^["))?;
    let s = t.screen();
    println!("--- g2 --- {:?}", s.row_text(0).trim_end());
    assert!(!s.contains("DONE"), "the reader is still blocked:\n{s}");
    Ok(())
}
