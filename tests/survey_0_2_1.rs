//! Probe suite for what termlens **0.2.1** changed, and what the new code
//! brought with it. Everything is reproduced against a real process.

mod common;

use std::time::{Duration, Instant};

use common::spawn_sh;
use termlens::{Error, Key, Terminal};

fn sh(script: &str) -> Terminal {
    spawn_sh(script, Duration::from_secs(3))
}

fn raw(script: &str) -> Terminal {
    spawn_sh(
        &format!("stty raw -echo; {script}; head -c 1 >/dev/null"),
        Duration::from_secs(3),
    )
}

// =================================================== 1. builder validation

#[test]
fn v1_zero_dimensions_are_rejected_at_spawn_and_resize() -> termlens::Result<()> {
    for (cols, rows) in [(0u16, 24u16), (80, 0), (0, 0)] {
        let err = Terminal::builder()
            .size(cols, rows)
            .args(["-c", "true"])
            .spawn("/bin/sh")
            .expect_err("zero must be rejected");
        assert!(matches!(err, Error::Input(_)), "{err:?}");
        println!("--- v1 --- {cols}x{rows}: {err}");
    }
    let mut t = raw("printf 'ALIVE'");
    t.wait_until(|s| s.contains("ALIVE"))?;
    let err = t.resize(0, 10).expect_err("resize zero rejected");
    assert!(matches!(err, Error::Input(_)), "{err:?}");
    // and the terminal is untouched by the rejected resize
    assert_eq!(t.screen().size(), (80, 24));
    Ok(())
}

#[test]
fn v2_upper_bound_is_unchecked_and_snapshots_cost_area() -> termlens::Result<()> {
    for (cols, rows) in [(80u16, 24u16), (500, 500), (1500, 1500)] {
        let spawn_start = Instant::now();
        let mut t = Terminal::builder()
            .size(cols, rows)
            .env_clear()
            .timeout(Duration::from_secs(10))
            .args(["-c", "stty raw -echo; printf 'BIG'; head -c 1 >/dev/null"])
            .spawn("/bin/sh")?;
        let spawned = spawn_start.elapsed();
        t.wait_until(|s| s.contains("BIG"))?;
        let snap_start = Instant::now();
        let s = t.screen();
        let first_snapshot = snap_start.elapsed();
        let text_start = Instant::now();
        let _ = s.text();
        println!(
            "--- v2 --- {cols}x{rows} ({} cells): spawn {spawned:?}, first screen() {first_snapshot:?}, text() {:?}",
            u64::from(cols) * u64::from(rows),
            text_start.elapsed()
        );
    }
    Ok(())
}

#[test]
fn v3_missing_current_dir_and_empty_program_are_rejected() {
    let err = Terminal::builder()
        .current_dir("/definitely/not/here")
        .args(["-c", "true"])
        .spawn("/bin/sh")
        .expect_err("missing cwd");
    println!("--- v3a --- {err}");
    assert!(matches!(err, Error::Spawn { .. }));

    let err = Terminal::builder().spawn("").expect_err("empty program");
    println!("--- v3b --- {err}");
    assert!(matches!(err, Error::Spawn { .. }));

    // A path that exists but is a file, not a directory.
    let err = Terminal::builder()
        .current_dir("/etc/hostname")
        .args(["-c", "true"])
        .spawn("/bin/sh")
        .expect_err("file as cwd");
    println!("--- v3c --- {err}");
}

// ========================================================= 2. paste changes

#[test]
fn v4_paste_now_sends_cr_and_collapses_crlf() -> termlens::Result<()> {
    let mut t =
        raw(r"printf '\033[?2004hPASTE='; od -An -c -N14 | tr -d '\n'; printf '\r\nDONE\r\n'");
    t.wait_until(|s| s.contains("PASTE="))?;
    t.paste("a\r\nb\nc");
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- v4 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

#[test]
fn v5_embedded_paste_markers_are_stripped() -> termlens::Result<()> {
    let mut t =
        raw(r"printf '\033[?2004hPASTE='; od -An -c -N16 | tr -d '\n'; printf '\r\nDONE\r\n'");
    t.wait_until(|s| s.contains("PASTE="))?;
    t.paste("a\x1b[201~INJECTED");
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- v5 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

#[test]
fn v6_paste_without_bracketed_mode_also_rewrites_newlines() -> termlens::Result<()> {
    let mut t = raw(r"printf 'PASTE='; od -An -c -N3 | tr -d '\n'; printf '\r\nDONE\r\n'");
    t.wait_until(|s| s.contains("PASTE="))?;
    t.paste("a\nb"); // no mode 2004 enabled
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- v6 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

// ==================================================== 3. mouse encodings

#[test]
fn v7_utf8_mouse_encoding_is_used_when_the_app_asks_for_1005() -> termlens::Result<()> {
    let mut t = raw(
        r"printf '\033[?1000h\033[?1005hCLICK='; od -An -tx1 -N6 | tr -d '\n'; printf '\r\nDONE\r\n'",
    );
    t.wait_until(|s| s.contains("CLICK="))?;
    t.click(100, 3)?; // legacy would put a bare 0x85 on the wire
    t.wait_until(|s| s.contains("DONE"))?;
    println!("--- v7 --- {:?}", t.screen().row_text(0).trim_end());
    Ok(())
}

#[test]
fn v8_utf8_mouse_still_refuses_past_222_and_blames_the_wrong_encoding() -> termlens::Result<()> {
    let mut t = raw(r"printf '\033[?1000h\033[?1005hREADY'");
    t.wait_until(|s| s.contains("READY"))?;
    // Mode 1005 exists precisely to carry coordinates past the legacy
    // limit (xterm goes to ~2015), but the guard runs before the encoding
    // is consulted.
    let err = t.click(300, 5).expect_err("refused");
    println!("--- v8 --- {err}");
    assert!(err.to_string().contains("legacy mouse encoding"), "{err}");
    Ok(())
}

// ================================================= 4. query diagnostics

#[test]
fn v9_several_unanswered_queries_are_all_named() {
    let mut t = sh(r#"printf '\033[?u\033[=c'; printf 'ASKED'; read x"#);
    let err = t
        .wait_until(|s| s.contains("NEVER"))
        .expect_err("times out");
    println!("--- v9 --- {}", err.to_string().lines().next().unwrap());
}

#[test]
fn v10_more_than_eight_distinct_queries_overflow_to_a_count() {
    let mut t = sh(
        r#"for n in 1 2 3 4 7 8 9 10 11 12 13 14; do printf '\033[%sn' "$n"; done; printf 'ASKED'; read x"#,
    );
    let err = t
        .wait_until(|s| s.contains("NEVER"))
        .expect_err("times out");
    println!("--- v10 --- {}", err.to_string().lines().next().unwrap());
}

#[test]
fn v11_a_query_the_app_moved_past_is_reported_as_context() {
    let mut t = sh(r#"printf '\033[?u'; sleep 0.2; printf 'PROGRESS'; sleep 0.2; read x"#);
    let err = t
        .wait_until(|s| s.contains("NEVER"))
        .expect_err("times out");
    println!("--- v11 --- {}", err.to_string().lines().next().unwrap());
}

#[test]
fn v12_eof_errors_now_carry_the_query_note() {
    let mut t = sh(r#"printf '\033[?u'; printf 'BYE'"#);
    let err = t
        .wait_until(|s| s.contains("NEVER"))
        .expect_err("eof first");
    assert!(matches!(err, Error::Eof { .. }), "{err:?}");
    println!("--- v12 --- {}", err.to_string().lines().next().unwrap());
}

#[test]
fn v13_wait_frame_timeout_now_shows_the_live_screen_not_the_last_frame() -> termlens::Result<()> {
    let mut t = raw(
        r"printf '\033[?2026h\033[2J\033[HCOMPLETE-FRAME\033[?2026l'; sleep 0.1; printf '\033[2J\033[HLIVE-TORN'",
    );
    t.wait_until(|s| s.contains("LIVE-TORN"))?;
    let err = t
        .wait_frame(|s| s.contains("NEVER"))
        .expect_err("times out");
    let msg = err.to_string();
    println!("--- v13 --- {}", msg.lines().next().unwrap());
    assert!(msg.contains("LIVE-TORN"), "shows the live screen");
    assert!(!msg.contains("COMPLETE-FRAME"), "not the last frame");
    Ok(())
}

// ============================================ 5. the new responder thread

#[test]
fn v14_batched_queries_lose_replies_silently() -> termlens::Result<()> {
    // A legitimate pattern: probe several capabilities, then read all the
    // answers. Here it is exaggerated to make the loss deterministic.
    let script = r#"stty raw -echo
i=0; while [ $i -lt 1500 ]; do printf '\033[6n'; i=$((i+1)); done
printf 'ASKED-1500'
sleep 0.6
stty min 0 time 5
GOT=$(dd bs=1 count=60000 2>/dev/null | tr -cd 'R' | wc -c)
stty sane
printf '\r\nGOT=%s\r\n' "$GOT"
head -c 1 >/dev/null"#;
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(15))
        .args(["-c", script])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("ASKED-1500"))?;
    t.wait_until_for(|s| s.contains("GOT="), Duration::from_secs(12))?;
    let s = t.screen();
    let line = s
        .text()
        .lines()
        .find(|l| l.contains("GOT="))
        .unwrap_or("")
        .to_owned();
    println!("--- v14 --- asked 1500, {line}");
    Ok(())
}

#[test]
fn v15_reply_backlog_note_appears() -> termlens::Result<()> {
    let script = r#"stty raw -echo
i=0; while [ $i -lt 1500 ]; do printf '\033[6n'; i=$((i+1)); done
printf 'ASKED'
head -c 1 >/dev/null"#;
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(3))
        .args(["-c", script])
        .spawn("/bin/sh")?;
    t.wait_until(|s| s.contains("ASKED"))?;
    let err = t
        .wait_until(|s| s.contains("NEVER"))
        .expect_err("times out");
    println!("--- v15 --- {}", err.to_string().lines().next().unwrap());
    Ok(())
}

#[test]
fn v16_reply_and_typed_input_are_written_by_different_threads() -> termlens::Result<()> {
    // The reply goes out on a duplicated fd from the responder thread while
    // send() writes on the original under a lock: nothing orders the two.
    let script = r#"stty raw -echo min 0 time 10
printf 'READY'
printf '\033[6n'
ORDER=$(dd bs=1 count=32 2>/dev/null | tr -d '\033[')
printf '\r\nORDER=[%s]\r\n' "$ORDER"
head -c 1 >/dev/null"#;
    for run in 0..8 {
        let mut t = Terminal::builder()
            .size(80, 24)
            .env_clear()
            .timeout(Duration::from_secs(5))
            .args(["-c", script])
            .spawn("/bin/sh")?;
        t.wait_until(|s| s.contains("READY"))?;
        t.send(Key::Char('Z'));
        t.wait_until(|s| s.contains("ORDER="))?;
        let s = t.screen();
        let line = s
            .text()
            .lines()
            .find(|l| l.contains("ORDER="))
            .unwrap_or("")
            .to_owned();
        println!("--- v16 run {run} --- {}", line.trim());
    }
    Ok(())
}

#[test]
fn v18_where_reply_loss_begins() -> termlens::Result<()> {
    for batch in [50u32, 200, 400, 600, 1000] {
        let script = format!(
            r#"stty raw -echo
i=0; while [ $i -lt {batch} ]; do printf '\033[6n'; i=$((i+1)); done
printf 'ASKED'
sleep 0.5
stty min 0 time 5
GOT=$(dd bs=1 count=60000 2>/dev/null | tr -cd 'R' | wc -c)
stty sane
printf '\r\nGOT=%s\r\n' "$GOT"
head -c 1 >/dev/null"#
        );
        let mut t = Terminal::builder()
            .size(80, 24)
            .env_clear()
            .timeout(Duration::from_secs(15))
            .args(["-c", &script])
            .spawn("/bin/sh")?;
        t.wait_until(|s| s.contains("ASKED"))?;
        t.wait_until_for(|s| s.contains("GOT="), Duration::from_secs(12))?;
        let s = t.screen();
        let got = s
            .text()
            .lines()
            .find(|l| l.contains("GOT="))
            .unwrap_or("")
            .trim()
            .to_owned();
        println!("--- v18 --- asked {batch}, {got}");
    }
    Ok(())
}

// ============================================== 6. still-open from 0.2.0

#[test]
fn v17_decrqm_2026_probe_still_unrecognised() {
    let mut t = sh(r#"printf '\033[?2026$p'; read -r r; printf 'GOT'; read x"#);
    let err = t.wait_until(|s| s.contains("GOT")).expect_err("no reply");
    assert!(
        !err.to_string().contains("queried the terminal"),
        "still not diagnosed: {err}"
    );
    println!("--- v17 --- unchanged in 0.2.1");
}
