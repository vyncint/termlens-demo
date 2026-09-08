//! `termlens-cli` — the published binary, against this repository's own
//! saved screens and its own application.
//!
//! The library half of the tier asks "does the crate still do what the study
//! says?". This asks the question a user meets first: **does
//! `cargo install termlens-cli` produce a tool that reads the snapshots a
//! real suite committed?** Nothing else in the tier checks that, and the
//! snapshots under `tests/snapshots/` are the honest input — insta wrote
//! them, from taskboard, and they have been committed since 0.2.
//!
//! The binary comes from crates.io like everything else here: `$TERMLENS_CLI`
//! if CI installed it, otherwise installed on demand at the same version as
//! the `termlens` the lockfile names, so the tool and the library under test
//! are one release.

use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::Duration;

use termlens::{Color, Terminal};

mod common;

/// The `termlens` version this suite is measured against, read from the
/// lockfile so the CLI can never drift from the library.
fn version_under_test() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let lock = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.lock"))
            .expect("Cargo.lock is committed");
        let mut lines = lock.lines();
        while let Some(line) = lines.next() {
            if line.trim() == "name = \"termlens\"" {
                for next in lines.by_ref() {
                    if let Some(rest) = next.trim().strip_prefix("version = \"") {
                        return rest.trim_end_matches('"').to_owned();
                    }
                }
            }
        }
        panic!("no termlens version in Cargo.lock");
    })
}

/// Path to the `termlens` binary, installed once per test process if it is
/// not already there. `cargo install` is a no-op when the version already
/// matches, so only the first run in a fresh checkout pays for it.
fn cli() -> &'static PathBuf {
    static BIN: OnceLock<PathBuf> = OnceLock::new();
    BIN.get_or_init(|| {
        if let Some(given) = std::env::var_os("TERMLENS_CLI") {
            return PathBuf::from(given);
        }
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("termlens-cli");
        let bin = root
            .join("bin")
            .join(format!("termlens{}", std::env::consts::EXE_SUFFIX));
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let status = Command::new(cargo)
            .args(["install", "termlens-cli", "--version", version_under_test()])
            .args(["--locked", "--root"])
            .arg(&root)
            .status()
            .expect("cargo install termlens-cli");
        assert!(
            status.success(),
            "cargo install termlens-cli --version {} failed. The CLI is \
             published alongside the library; if this version of termlens \
             exists on crates.io and termlens-cli does not, that is the \
             finding — the two releases went out of lockstep.",
            version_under_test()
        );
        assert!(bin.exists(), "installed but missing at {}", bin.display());
        bin
    })
}

fn run(args: &[&str]) -> Output {
    Command::new(cli())
        .args(args)
        .output()
        .expect("run the termlens binary")
}

/// A snapshot this repository committed, as a path the CLI can be handed.
fn snap(name: &str) -> String {
    format!("{}/tests/snapshots/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// The tool and the library are one release. If this fails, everything below
/// is measuring two different versions.
#[test]
fn the_installed_binary_is_the_version_under_test() {
    let out = run(&["--version"]);
    assert!(out.status.success());
    let printed = String::from_utf8_lossy(&out.stdout);
    assert_eq!(printed.trim(), format!("termlens {}", version_under_test()));
}

/// The saved-screen reader, against screens this suite actually saved: an
/// insta `.snap` carries a `---` metadata header above the content, and the
/// CLI is expected to see through it.
#[test]
fn render_reads_the_insta_snapshots_this_suite_committed() {
    let out = run(&["render", "--text", &snap("tui__snapshot_initial_view.snap")]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("size: 90x26"), "the header survives:\n{text}");
    assert!(
        text.contains("Wire up the PTY reader"),
        "and the grid:\n{text}"
    );
    assert!(text.contains("styles:"), "with a styles block appended");

    // The styled snapshot round-trips with its spans intact.
    let styled = run(&[
        "render",
        "--text",
        &snap("tui__snapshot_initial_view_with_styles.snap"),
    ]);
    assert!(styled.status.success());
    assert!(String::from_utf8_lossy(&styled.stdout).contains("reverse"));

    // SVG and HTML carry the application's text into a report.
    for (flag, needle) in [("--svg", "<svg"), ("--html", "<pre")] {
        let out = run(&["render", flag, &snap("tui__snapshot_initial_view.snap")]);
        assert!(out.status.success(), "{flag}");
        let body = String::from_utf8_lossy(&out.stdout);
        assert!(
            body.contains(needle),
            "{flag}: {}",
            &body[..60.min(body.len())]
        );
        assert!(body.contains("taskboard"), "{flag} carries the text");
    }
}

/// The exit code is the API a script uses: 0 when two saved screens are the
/// same picture, 1 when they differ, 2 when the tool could not run.
#[test]
fn diff_exits_zero_one_and_two() {
    let initial = snap("tui__snapshot_initial_view.snap");
    let help = snap("tui__snapshot_help_overlay.snap");

    let same = run(&["diff", "--color", "never", &initial, &initial]);
    assert_eq!(same.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&same.stdout).contains("no difference"));

    let differ = run(&["diff", "--color", "never", &initial, &help]);
    assert_eq!(differ.status.code(), Some(1), "they differ");
    let rendered = String::from_utf8_lossy(&differ.stdout);
    assert!(
        rendered.contains("help"),
        "the overlay is named:\n{rendered}"
    );
    assert!(rendered.contains("rows unchanged"), "and the rest counted");

    let missing = run(&["diff", "--color", "never", &initial, "/no/such/file.snap"]);
    assert_eq!(missing.status.code(), Some(2), "the tool could not run");
    assert!(String::from_utf8_lossy(&missing.stderr).starts_with("termlens:"));

    let unknown = run(&["frobnicate"]);
    assert_eq!(unknown.status.code(), Some(2));
}

/// `inspect` runs a program in a PTY and prints what it drew — pointed at
/// this repository's own binary, which is the use the tool exists for.
#[test]
fn inspect_drives_taskboard_and_reports_what_it_exited_as() {
    let out = Command::new(cli())
        .args(["inspect", "--size", "90x26", "--idle", "400"])
        .arg(env!("CARGO_BIN_EXE_taskboard"))
        .output()
        .expect("run inspect");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let screen = String::from_utf8_lossy(&out.stdout);
    assert!(screen.contains("size: 90x26"), "{screen}");
    assert!(
        screen.contains("Wire up the PTY reader"),
        "the real grid:\n{screen}"
    );
    assert!(screen.contains("NORMAL"), "the status bar");
    // taskboard never exits on its own, so inspect reports the deadline
    // rather than an exit status — and says which.
    assert!(
        screen.contains("still running at the deadline"),
        "the trailer says what the program did:\n{screen}"
    );
}

/// The one thing only a PTY harness can check about a terminal tool: that it
/// **colours its output on a terminal and not in a pipe**. `Command` above
/// only ever sees the pipe; this drives the same binary through termlens and
/// reads the colour off the rendered cells.
#[test]
fn diff_colours_on_a_terminal_and_stays_plain_in_a_pipe() -> termlens::Result<()> {
    let initial = snap("tui__snapshot_initial_view.snap");
    let help = snap("tui__snapshot_help_overlay.snap");

    // In a pipe: no SGR at all.
    let piped = run(&["diff", &initial, &help]);
    assert_eq!(piped.status.code(), Some(1));
    assert!(
        !String::from_utf8_lossy(&piped.stdout).contains('\u{1b}'),
        "a pipe gets the plain rendering"
    );

    // On a terminal: the same diff, painted. termlens gives the tool a real
    // PTY, so `IsTerminal` is true and the colour decision is the live one.
    let mut t = Terminal::builder()
        .size(200, 40)
        .env_clear()
        .timeout(Duration::from_secs(20))
        .args(["diff", &initial, &help])
        .spawn(cli())?;
    t.wait_until(|s| s.contains("rows unchanged"))?;
    let screen = t.screen();
    assert!(
        screen
            .find_by(|cell| cell.style().fg == Color::Indexed(1))
            .is_some(),
        "the before side is painted red:\n{}",
        screen.with_styles()
    );
    assert!(
        screen
            .find_by(|cell| cell.style().fg == Color::Indexed(2))
            .is_some(),
        "and the after side green:\n{}",
        screen.with_styles()
    );
    Ok(())
}
