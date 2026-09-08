//! `TERMLENS_ARTIFACT_DIR` (0.10): a failing wait's screen reaches the CI
//! log, and with the variable set it also reaches a directory a later step
//! can render into the pull request.
//!
//! Alone in its own test binary because the variable is process-wide, and
//! because setting it is `unsafe` under edition 2024 — sound here precisely
//! because this binary runs one test and nothing else races the environment.

use std::time::Duration;

use termlens::Screen;

mod common;

#[test]
fn a_failed_wait_writes_its_screen_where_ci_can_find_it() -> termlens::Result<()> {
    let dir = std::env::temp_dir().join(format!("taskboard-artifacts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);

    // SAFETY: this binary contains exactly one test, so no other thread is
    // reading or writing the environment while this runs.
    unsafe { std::env::set_var("TERMLENS_ARTIFACT_DIR", &dir) };

    let mut t = common::spawn();
    let err = t
        .wait_until_for(|s| s.contains("no such text"), Duration::from_millis(300))
        .expect_err("the wait must time out");
    assert!(matches!(err, termlens::Error::Timeout { .. }));

    let mut files: Vec<_> = std::fs::read_dir(&dir)?
        .map(|entry| entry.expect("a directory entry").path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 1, "one screen, one file: {files:?}");

    let name = files[0].file_name().unwrap().to_string_lossy().into_owned();
    assert!(
        name.starts_with("a_failed_wait_writes_its_screen_where_ci_can_find_it-1.screen."),
        "the file is named for the test that produced it: {name}"
    );

    // With the `serde` feature on, the artifact is JSON; without it, the
    // `with_styles` text that `Screen::parse` reads back. This suite enables
    // `serde`, so it is the structured form — and it is the same screen the
    // error carried, not a re-read of a terminal that has moved on.
    assert!(name.ends_with(".screen.json"), "{name}");
    let body = std::fs::read_to_string(&files[0])?;
    let saved: Screen = serde_json::from_str(&body).expect("the file is a Screen");
    let embedded = err.screen().expect("a timeout carries its screen");
    assert!(saved.diff(embedded).is_empty(), "{}", saved.diff(embedded));
    assert!(
        saved.contains("NORMAL"),
        "and it is taskboard's screen:\n{saved}"
    );

    // SAFETY: as above.
    unsafe { std::env::remove_var("TERMLENS_ARTIFACT_DIR") };
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}
