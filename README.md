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

- **`tests/tui.rs`** (49) — what termlens covers: text, per-cell styles
  (including strikethrough, blink and conceal), cursor, wide glyphs, keys and
  chords, the full mouse API, paste, clipboard payloads, frame history,
  per-call deadlines, resize, terminal state, signals, exit codes, snapshots.
- **`tests/limits.rs`** (13) — one passing test per remaining limitation. Each
  asserts the *mechanism* rather than a symptom, because four of the 0.2 pins
  stayed green against 0.4 while their claims went false.

## Findings — termlens 0.4

**[docs/TERMLENS-COVERAGE.md](docs/TERMLENS-COVERAGE.md)** is the write-up.
It re-runs the study against the same app across three releases (0.2.0 →
0.4.0): **all three items the 0.2 study ranked shipped, in that order**, and
so did the two it had written off as outside the model.

- **A methodological finding first.** Bumping the dependency and changing
  nothing else left every test green — including four pinning tests whose
  claims had become false. A green pin asserting something false is worse
  than no pin, so they now assert the mechanism rather than a symptom the
  mechanism shares with its replacement.
- **The style model catches a masked field.** `conceal` reaching nothing meant
  a test asserting a password field is masked passed against an app printing
  the secret in the clear — identical text, no marker on either cell. Also
  strikethrough and blink, both now visible as snapshot diffs.
- **Clipboard payloads are assertable.** `y` copies a title with `OSC 52`;
  the toast proved only that the code path ran. `Screen::clipboard()` reports
  the decoded text *and* the target selection.
- **Frames are a history with an enforced order.** Eight retained, each
  observable once, in emission order — and a superseded frame can no longer
  satisfy a wait made after your input.
- **Scrollback, and `DECRQM` answered** — the latter is why a probe-then-enable
  application now turns its own mouse on against termlens unmodified.
- Still open: bounds rather than absences — the 8-frame retention limit, an
  ambiguous frame predicate resolving on the older frame, `wait_frame` needing
  the app to opt in, torn `screen()` reads, `Esc`+key reading as Alt, bounded
  and unstyled scrollback, and `send` panicking rather than returning `Result`.
