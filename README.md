# termlens-demo

A deliberately feature-dense TUI, built as a subject for
[termlens](https://crates.io/crates/termlens) integration tests — and a
written-up answer to *what can termlens actually cover?*

## taskboard

`src/` is a ratatui/crossterm task manager with tabs, a filtered list, a
detail pane, a modal confirm dialog, a text input with a live cursor, a help
overlay, colour-coded rows, CJK and emoji titles, a responsive layout, mouse
selection and bracketed paste.

```sh
cargo run --bin taskboard
```

| key | |
|---|---|
| `j`/`k`, arrows | move cursor |
| `PgUp`/`PgDn`, `Home`/`End` | page / jump |
| `Tab`/`Shift-Tab`, `Ctrl-←`/`Ctrl-→` | switch tab |
| `space` | toggle done |
| `/` | filter (matches titles and tags) |
| `d` | delete, with confirmation |
| `?` or `F1` | help |
| `q` / `Ctrl-C` | quit (exit 0 / 130) |
| click / wheel / right-click | select / move / clear filter |

Repaints are bracketed in DEC 2026 synchronized updates, and `SIGTERM`
triggers a graceful shutdown (exit 143).

## Tests

```sh
cargo test
```

- **`tests/tui.rs`** (40) — what termlens covers: text, per-cell styles,
  cursor, wide glyphs, keys and chords, mouse, paste, resize, terminal state,
  signals, exit codes, snapshots.
- **`tests/limits.rs`** (9) — one passing test per remaining limitation, each
  demonstrating the gap against the real binary and encoding the workaround.

## Findings — termlens 0.2

**[docs/TERMLENS-COVERAGE.md](docs/TERMLENS-COVERAGE.md)** is the write-up.
It re-runs the 0.1 study against the same app: **11 of the 12 gaps found
then are closed.**

- **`wait_frame` is the release.** DEC 2026 synchronized updates mean
  predicates only ever see complete frames, which deleted all three
  defensive idioms the 0.1 suite needed. It also *retains* the last complete
  frame, so a frame the app immediately overwrites is still catchable —
  measured 5/5 against `wait_until`'s 2/5.
- **Input reads the app's real modes**: `paste` wraps only if mode 2004 is
  on, `click` encodes for the tracking mode the app enabled and errors if it
  enabled none, `send` picks the DECCKM cursor form.
- **Terminal queries are answered** — and answered truthfully, not with
  canned strings. Unanswerable ones are named in the timeout message.
- **`with_styles()`** puts styling into snapshots, so a highlight moving is
  finally a diff.
- Still open: no frame *history* (§2.1), `wait_frame` needs the app to opt
  in (§2.2), `Esc`+key still reads as Alt (§2.3), the mouse API is one
  button (§2.4), no scrollback, and no `wait_frame_for`.
