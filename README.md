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

## Tests

```sh
cargo test
```

- **`tests/tui.rs`** (28) — what termlens covers: text, per-cell styles,
  cursor, wide-glyph layout, keys, resize, exit codes, screen snapshots.
- **`tests/limits.rs`** (11) — one passing test per limitation, each
  demonstrating the gap against the real binary and encoding the workaround.

## Findings

**[docs/TERMLENS-COVERAGE.md](docs/TERMLENS-COVERAGE.md)** is the write-up.
The short version:

- It covers keyboard-driven full-screen TUIs very well, and the
  screen-carrying timeout errors make failures cheap to diagnose.
- The gap that will bite first is that **there is no frame boundary** —
  `wait_until` can fire on a half-painted frame, including half a row. Three
  idioms avoid it; all three are used in `tests/tui.rs`.
- **A TUI that probes the terminal** (DSR, DA1, OSC 11 background query)
  hangs under test — nothing ever replies.
- **Snapshots are text-only**, so a styling-only regression passes silently.
- No mouse, modifier+special-key chords, or bracketed paste in the typed
  `Key` API; all three need hand-rolled escape bytes.
