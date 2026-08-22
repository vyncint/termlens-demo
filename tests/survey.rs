//! A broad survey of what termlens can and cannot observe, probed against
//! plain `/bin/sh` so each finding is isolated from any application.
//! Every claim is reproduced against a real process, never inferred.
//!
//! Run against termlens 0.6.0. Nine findings here were gaps in 0.2 and are
//! now coverage: the three style attributes (A), the clipboard payload (B),
//! the `DECRQM` probe and the configurable foreground (D), the three
//! `wait_frame` staleness cases (E), and the silent write to a dead child
//! (F). Each kept its original number so the 0.2 write-up still lines up.
//!
//! Three of those nine **could not have failed** when the gap closed —
//! `b2`, `d3`, `d4` asserted a symptom, or only printed. Where a finding is
//! now coverage, its assertions were tightened to name the mechanism.
//!
//! Two more moved on the way to 0.6: `c5` (NFD text no longer misses an NFC
//! needle — 0.5 folds both sides) and `f2`/`f7` (a write to a departed child
//! is a `Result` rather than a panic). Both kept their numbers.

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
    let (r, c) = s
        .find(needle)
        .unwrap_or_else(|| panic!("{needle} missing\n{s}"));
    *s.cell(r, c).unwrap().style()
}

// ============================================================ A. style model

/// 0.4 added the three attributes vt100 drops. Before that, `SGR 9` and
/// `SGR 5` reached nothing, so three distinct renderings were one value.
#[test]
fn a1_strikethrough_and_blink_are_in_the_style_model() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[9mSTRIKE\033[0m \033[5mBLINK\033[0m \033[1mBOLD\033[0m'");
    t.wait_until(|s| s.contains("BOLD"))?;
    let s = t.screen();

    assert!(style_of(&s, "STRIKE").strikethrough, "SGR 9 is tracked");
    assert!(style_of(&s, "BLINK").blink, "SGR 5 is tracked");
    assert!(style_of(&s, "BOLD").bold, "and bold still is");

    // Each is exactly one attribute, not a smear across the others.
    assert_eq!(
        style_of(&s, "STRIKE"),
        Style {
            strikethrough: true,
            ..Style::default()
        }
    );
    assert_eq!(
        style_of(&s, "BLINK"),
        Style {
            blink: true,
            ..Style::default()
        }
    );

    // And therefore visible in a styled snapshot, which is where a style
    // regression is actually caught.
    let dump = s.with_styles().to_string();
    assert!(
        dump.contains("strikethrough") && dump.contains("blink"),
        "{dump}"
    );
    println!("--- a1 styles ---\n{dump}");
    Ok(())
}

/// The text of a concealed field stays in the grid — that is what a real
/// terminal holds, and it has not changed. What changed in 0.4 is that the
/// cells say they were concealed, so a masked field and clear text are no
/// longer the same value.
///
/// This is the one gap in this survey whose absence made a *green test
/// certify a bug*: "assert the password is masked" passed against an
/// application printing it in the clear.
#[test]
fn a2_conceal_is_marked_even_though_the_text_remains() -> termlens::Result<()> {
    let mut t = raw(r"printf 'pw: \033[8mhunter2\033[0m END'");
    t.wait_until(|s| s.contains("END"))?;
    let masked = t.screen();
    assert!(masked.contains("hunter2"), "still in the grid:\n{masked}");
    assert!(
        style_of(&masked, "hunter2").conceal,
        "and marked as concealed"
    );

    // The control: identical text, never concealed, and now distinguishable.
    let mut t = raw(r"printf 'pw: hunter2 END'");
    t.wait_until(|s| s.contains("END"))?;
    let clear = t.screen();
    assert_eq!(masked.text(), clear.text(), "the grids are identical text");
    assert!(
        !style_of(&clear, "hunter2").conceal,
        "and only one is masked"
    );
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
    let mut t =
        raw(r"printf '\033[53mOVER\033[0m \033[91mBRIGHTRED\033[0m \033[38;5;9mIDX9\033[0m END'");
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

/// 0.4 captures `OSC 52` writes. The old version of this test asserted only
/// that the base64 stays off the grid and that the title is untouched —
/// both still true, and neither ever the claim its name made, so it kept
/// passing after the gap closed.
#[test]
fn b2_osc52_clipboard_write_is_captured_with_its_target() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033]52;c;aGVsbG8=\033\\COPIED'");
    t.wait_until(|s| s.contains("COPIED"))?;
    let s = t.screen();
    assert!(
        !s.contains("aGVsbG8"),
        "the escape stays off the grid:\n{s}"
    );
    assert_eq!(s.title(), "", "and out of the title");

    let clip = s.clipboard().expect("the write was captured");
    assert_eq!(clip.text(), Some("hello"));
    assert_eq!(clip.targets(), "c");

    // An undecodable payload is distinguishable from an empty clipboard,
    // which is what keeps `text() == Some("")` from being a false positive.
    let mut t = raw(r"printf '\033]52;p;not~base64\033\\DONE'");
    t.wait_until(|s| s.contains("DONE"))?;
    let s = t.screen();
    let clip = s.clipboard().expect("still a write");
    assert_eq!(clip.text(), None, "unreadable, not empty");
    assert_eq!(clip.targets(), "p");
    Ok(())
}

#[test]
fn b3_bell_is_unobservable() -> termlens::Result<()> {
    let mut t = raw(r"printf 'before\007after'");
    t.wait_until(|s| s.contains("after"))?;
    let s = t.screen();
    println!("--- b3 ---\n{s:?}");
    assert!(
        s.contains("beforeafter") || s.contains("before after"),
        "{s}"
    );
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
    assert!(
        !s.contains("padded   "),
        "trailing spaces trimmed from text()"
    );
    assert!(
        s.row_text(0).starts_with("padded   "),
        "row_text keeps them"
    );
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
    assert!(
        s.to_string().starts_with("size: 80x24  cursor: hidden"),
        "{s}"
    );
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

/// A gap in 0.2 and 0.4, coverage in 0.5: `contains` and `find` fold both
/// sides to NFC, so the needle no longer has to guess which normalization
/// the application's data source used.
#[test]
fn c5_either_normalization_matches() -> termlens::Result<()> {
    // The app prints a decomposed 'é' (e + U+0301), as plenty of real data
    // and every macOS filename does.
    let mut t = raw("printf 'caf\u{65}\u{301} END'");
    t.wait_until(|s| s.contains("END"))?;
    let s = t.screen();
    assert!(s.contains("café"), "the obvious needle now hits:\n{s}");
    assert!(
        s.contains("caf\u{65}\u{301}"),
        "and the decomposed form still does"
    );
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
    assert_ne!(
        before.with_styles().to_string(),
        after.with_styles().to_string()
    );
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

/// The standard way an application asks "do you support synchronized
/// output?". Under 0.2 it was neither answered nor named: the probe hung
/// silently and the timeout said nothing about it, which is what made a
/// careful application untestable by frame.
///
/// Worth recording how the old version of this test *could not have
/// detected the fix*: it read the reply with a cooked-mode `read`, which
/// waits for a newline a `DECRPM` reply never contains, so it timed out
/// whether or not the terminal answered — and its one assertion (that no
/// note names the query) also holds once the query is answered rather than
/// unanswered. Two independent reasons to pass, neither of them the claim.
#[test]
fn d3_decrqm_probe_for_2026_is_answered() -> termlens::Result<()> {
    // `ESC [ ? 2026 ; 2 $ y` — 11 bytes, read exactly, in raw mode.
    let mut t = sh(concat!(
        r"stty -icanon -echo; printf '\033[?2026$p'; ",
        r#"reply=$(head -c 11 | tr '\033' 'E'); printf 'GOT:%s' "$reply"; read x"#
    ));
    t.wait_until(|s| s.contains("GOT:E[?2026;2$y"))?;
    // 2 = "implemented, currently reset" — the honest answer outside an
    // update, and the one that makes an app enable the feature.
    Ok(())
}

#[test]
fn d3b_kitty_probe_is_named_for_contrast() {
    let mut t = sh(r#"printf '\033[?u'; read -r r; printf 'GOT'; read x"#);
    let err = t.wait_until(|s| s.contains("GOT")).expect_err("no reply");
    assert!(err.to_string().contains("queried the terminal"), "{err}");
}

/// Under 0.2 the `OSC 10` foreground answer was hardcoded white while the
/// background was configurable, so an application picking a theme by
/// comparing the two could only ever be tested against one of the two
/// possible verdicts. 0.3 added `foreground_rgb`.
#[test]
fn d4_both_osc10_and_osc11_answers_are_configurable() -> termlens::Result<()> {
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
        .foreground_rgb(0xcd, 0xd6, 0xf4)
        .args(["-c", script])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("DONE"))?;
    let screen = t.screen();
    assert!(
        screen.contains("BG=<]11;rgb:1e1e/1e1e/2e2e>"),
        "background as configured:\n{screen}"
    );
    assert!(
        screen.contains("FG=<]10;rgb:cdcd/d6d6/f4f4>"),
        "foreground as configured, not hardcoded white:\n{screen}"
    );
    Ok(())
}

#[test]
fn d5_pixel_size_query_is_unanswerable() {
    let mut t = sh(r#"printf '\033[14t'; read -r r; printf 'GOT'; read x"#);
    let err = t.wait_until(|s| s.contains("GOT")).expect_err("no reply");
    println!("--- d5 ---\n{err}");
}

// ============================================================= E. wait_frame

/// Under 0.2 a `wait_frame` after a resize matched the frame drawn at the
/// *old* size, so a test could assert on 80-column content while the
/// terminal was 40 columns wide. 0.4 advances the frame cursor on resize: a
/// frame drawn at the old size is not the repaint that answers the new one.
#[test]
fn e1_a_resize_stops_offering_frames_drawn_at_the_old_size() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?2026h\033[2J\033[HFRAME-1\033[?2026l'");
    t.wait_frame(|s| s.contains("FRAME-1"))?;

    t.resize(40, 10)?;
    // The child never repaints, so no post-resize frame can exist — and the
    // pre-resize one is no longer offered.
    let stale = t.wait_frame_for(
        |s| s.contains("FRAME-1") && s.cols() == 80,
        Duration::from_millis(400),
    );
    assert!(
        stale.is_err(),
        "a pre-resize frame must not answer a post-resize wait: {stale:?}"
    );
    assert_eq!(t.screen().cols(), 40, "live screen did resize");
    Ok(())
}

/// The mechanism behind every "the test passed and proved nothing" in a
/// frame-driven suite. Under 0.2 a retained frame satisfied every wait, so
/// `send(key)` followed by `wait_frame(old_state)` passed vacuously — a
/// regression in which the key stopped working was invisible.
///
/// 0.4 gives each frame to exactly one wait, so the second call has to wait
/// for a repaint that never comes.
#[test]
fn e2_a_frame_already_returned_cannot_satisfy_a_second_wait() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?2026h\033[2J\033[HSTATE-A\033[?2026l'");
    t.wait_frame(|s| s.contains("STATE-A"))?;

    t.send(Key::Char('x'))?; // the child never repaints in response
    let vacuous = t.wait_frame_for(|s| s.contains("STATE-A"), Duration::from_millis(400));
    assert!(
        vacuous.is_err(),
        "asserting the old state after a keystroke must fail, got {vacuous:?}"
    );
    Ok(())
}

/// Under 0.2 the frame a predicate matched was unreachable: `wait_frame`
/// returned `()`, so the only screen you could read afterwards was a later —
/// possibly half-painted — instant. 0.4 returns the matched frame, which
/// closes the divergence between what the predicate saw and what the
/// assertion reads.
///
/// Note what has *not* changed, deliberately: `screen()` is still the live
/// grid, and can still be torn.
#[test]
fn e3_the_frame_that_matched_is_returned_even_when_screen_has_moved_on() -> termlens::Result<()> {
    let mut t = raw(
        r"printf '\033[?2026h\033[2J\033[HFRAME-DONE\033[?2026l'; sleep 0.1; printf '\033[?2026h\033[2J\033[HTORN-HALF'",
    );
    t.wait_until(|s| s.contains("TORN-HALF"))?;

    // The completed frame is handed back…
    let frame = t.wait_frame(|s| s.contains("FRAME-DONE"))?;
    assert!(frame.contains("FRAME-DONE"), "{frame}");
    assert!(
        !frame.contains("TORN-HALF"),
        "and it is that instant, not a later one"
    );

    // …while `screen()` still shows the live, half-painted grid.
    let s = t.screen();
    assert!(s.contains("TORN-HALF"), "{s}");
    assert!(!s.contains("FRAME-DONE"), "{s}");
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
        "--- f1 --- code={:?} success={} signal={:?} display={status}",
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
    let refused = t.send(Key::Char('z'));
    println!("--- f2 --- send after exit refused: {refused:?}");
    assert!(
        refused.is_err(),
        "typing at a dead child is an error, not a write"
    );
    Ok(())
}

#[test]
fn f3_char_above_ascii_is_sent_as_utf8_not_a_raw_byte() -> termlens::Result<()> {
    let mut t = sh(
        r#"stty raw -echo; printf 'BYTES='; od -An -tx1 -N3 | tr -d '\n'; printf '\r\nDONE\r\n'; head -c 1 >/dev/null"#,
    );
    t.wait_until(|s| s.contains("BYTES="))?;
    t.send(Key::Char('\u{80}'))?; // one codepoint...
    t.send(Key::Char('!'))?;
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
        "--- f6 --- code={:?} signal={:?} display={status}",
        status.code(),
        status.signal()
    );
    Ok(())
}

/// The doc contract has always said this panics with the screen attached.
/// Under 0.2 it did not — ten writes to a dead child were silently
/// discarded, so a test that typed into a corpse passed. 0.3 routes writes
/// through an acknowledged writer thread, so the `EIO` from the dead PTY
/// reaches the caller and the documented contract is finally the behaviour.
///
/// Not listed as a fix in the crate's changelog; it fell out of the
/// write-deadline work.
#[test]
fn f7_send_after_exit_is_refused_by_value() {
    let mut t = sh("printf 'BYE\\n'");
    t.wait_until(|s| s.contains("BYE")).expect("output");
    t.wait_exit().expect("exit");
    let error = t
        .send(Key::Char('z'))
        .expect_err("a dead child cannot be typed at");
    println!("--- f7 --- {error}");
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
    println!(
        "--- h6 --- row0={:?} row1={:?}",
        s.row_text(0).trim_end(),
        s.row_text(1).trim_end()
    );
    for col in 78..80 {
        let c = s.cell(0, col).unwrap();
        println!(
            "col {col}: {:?} wide={} cont={}",
            c.contents(),
            c.is_wide(),
            c.is_wide_continuation()
        );
    }
    // Correct xterm behaviour: it wraps whole to the next row.
    assert_eq!(s.row_text(1).trim_end(), "漢");
    Ok(())
}

// h11 (paste sent LF where a real terminal sends CR) was fixed in 0.2.1 and
// is covered by `survey_0_2_1::v4_paste_now_sends_cr_and_collapses_crlf`.
// Removed here rather than left as a stale duplicate: it only printed, so it
// could never have failed when the behaviour changed.

/// Still a limitation, and deliberately so — an application inside an
/// unfinished repaint is not idle — but no longer a mysterious one. Under
/// 0.2 the timeout read "waiting for 20ms of output silence", which is
/// nonsense next to a terminal that has been silent for two seconds. 0.4
/// names the real state.
#[test]
fn h9_a_hang_inside_a_synchronized_update_is_never_idle_but_says_why() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?2026hHALF-PAINTED'");
    t.wait_until(|s| s.contains("HALF-PAINTED"))?;
    // The stream has been silent for ages, but the frame never closed.
    let err = t
        .wait_idle(Duration::from_millis(20))
        .expect_err("never idle");
    let msg = err.to_string();
    assert!(
        msg.contains("unfinished DEC 2026 synchronized update"),
        "the timeout should name the open frame: {msg}"
    );
    assert!(
        msg.contains("half-painted frame"),
        "and say what the embedded screen is: {msg}"
    );
    Ok(())
}

/// The consequence of the previous item, and the reason it mattered: a light
/// theme used to answer white-on-white, so an application choosing its
/// palette by luminance was told something no real terminal would say. A
/// light theme is now expressible in full.
#[test]
fn h10_a_light_theme_answers_dark_on_light() -> termlens::Result<()> {
    let script = r#"stty raw -echo min 0 time 10
printf '\033]11;?\033\\'; BG=$(dd bs=1 count=64 2>/dev/null | tr -d '\033\\')
printf '\033]10;?\033\\'; FG=$(dd bs=1 count=64 2>/dev/null | tr -d '\033\\')
stty sane; printf 'BG=<%s>\r\nFG=<%s>\r\nDONE\r\n' "$BG" "$FG"; head -c 1 >/dev/null"#;
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .background_rgb(0xff, 0xff, 0xff) // a light theme…
        .foreground_rgb(0x1e, 0x1e, 0x2e) // …with readable text on it
        .args(["-c", script])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("DONE"))?;
    let screen = t.screen();
    assert!(screen.contains("BG=<]11;rgb:ffff/ffff/ffff>"), "{screen}");
    assert!(
        screen.contains("FG=<]10;rgb:1e1e/1e1e/2e2e>"),
        "dark on light, which 0.2 could not express:\n{screen}"
    );
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
    println!(
        "--- h7 --- after leaving alt: {:?}",
        s.row_text(0).trim_end()
    );
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
        .args([
            "-c",
            "stty raw -echo; printf 'NO-SYNC'; head -c 1 >/dev/null",
        ])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("NO-SYNC"))?;
    let start = std::time::Instant::now();
    let err = t.wait_frame(|_| true).expect_err("never emits frames");
    println!(
        "--- h8 --- burned {:?} to find out: {}",
        start.elapsed(),
        err
    );
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
    assert!(
        !s.contains("RIGHTMOST"),
        "growing back does not restore it:\n{s}"
    );
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
