# termlens-demo

A deliberately feature-dense TUI, built as a subject for
[termlens](https://crates.io/crates/termlens) integration tests — and a
written-up answer to *what can termlens actually cover?*

## taskboard

`src/` is a ratatui/crossterm task manager with tabs, a kanban board, a
filtered list, a detail pane, a modal confirm dialog, a progress run, a text
input with a live cursor, a help overlay, colour-coded rows, CJK and emoji
titles, a responsive layout, mouse selection and bracketed paste.

```sh
cargo run --bin taskboard
```

| key | |
|---|---|
| `j`/`k`, arrows | move cursor |
| `h`/`l` | move between board lanes |
| `PgUp`/`PgDn`, `Home`/`End` | page / jump |
| `Tab`/`Shift-Tab`, `Ctrl-←`/`Ctrl-→` | switch tab |
| `space` | toggle done |
| `/` | filter (matches titles and tags) |
| `d` | delete, with confirmation |
| `r` | run the selected task |
| `y` | yank the title to the clipboard |
| `T` | toggle the high-contrast palette |
| `?` or `F1` | help |
| `q` / `Ctrl-C` | quit (exit 0 / 130) |
| click / wheel / right-click | select / move / clear filter |

Repaints are bracketed in DEC 2026 synchronized updates, and `SIGTERM`
triggers a graceful shutdown (exit 143).

### The hard surface

taskboard also emits, on purpose, what a screen-grid harness struggles
with: `OSC 8` hyperlinks, `OSC 52` clipboard writes, `OSC 4` palette
overrides, `DECSCUSR` cursor shapes, `BEL`, focus events, strikethrough,
blink, conceal, and a burst of complete frames from a single keystroke.

Two flags make the application probe its terminal the way a careful one
does:

```sh
cargo run --bin taskboard -- --probe-sync   # ask DECRQM about DEC 2026 first
cargo run --bin taskboard -- --probe-caps   # fire six capability probes at startup
```

`--probe-sync` is the sharp one: the application brackets its repaints only
if the terminal says it supports synchronized output, and termlens does not
recognise the query that asks. Under the harness the same binary therefore
becomes completely untestable by frame.

## Tests

```sh
cargo test
```

**135 tests**, 5/5 clean stress runs.

- **`tests/tui.rs`** (40) — what termlens covers: text, per-cell styles,
  cursor, wide glyphs, keys and chords, mouse, paste, resize, terminal
  state, signals, exit codes, snapshots.
- **`tests/hard.rs`** (23) — the hard cases driven against the application:
  the board, the frame burst, the capability probe, and one passing test per
  thing taskboard demonstrably does that no assertion can reach.
- **`tests/limits.rs`** (9) — one passing test per 0.2 limitation, each
  demonstrating the gap against the real binary and encoding the workaround.
- **`tests/survey.rs`** (45) — the same questions asked of plain `/bin/sh`,
  so each finding is isolated from any application.
- **`tests/survey_0_2_1.rs`** (18) — what 0.2.1 changed, and what the new
  code brought with it.

The survey suites print their evidence under `--nocapture`.

## Findings — termlens 0.2.1

**[docs/TERMLENS-COVERAGE.md](docs/TERMLENS-COVERAGE.md)** is the write-up:
the 0.1 → 0.2 study, then a deeper pass against 0.2.1 with this harder
subject.

- **0.2 was the release that made the suite ordinary.** `wait_frame` and
  DEC 2026 deleted all three defensive idioms the 0.1 suite needed, and it
  *retains* the last complete frame, so a frame the app immediately
  overwrites is still catchable — measured 5/5 against `wait_until`'s 2/5.
- **0.2.1 is a good patch**: paste now sends `\r` and strips embedded paste
  markers, mouse mode 1005 is encoded properly, the builder rejects
  configurations that cannot work, and query diagnostics now name several
  probes at once and say when one is *probably not* the cause.
- **The sharpest remaining gap is one unrecognised query.** termlens
  implements DEC 2026 but does not answer `CSI ? 2026 $ p`, so an
  application that asks before using synchronized output never uses it —
  and `wait_frame` then blames the application for not emitting frames.
- **Nothing outside the grid is reachable**: hyperlink targets, clipboard
  writes, the bell, cursor shape, palette overrides, strikethrough, blink
  and conceal all vanish. Conceal is the one to know about — the concealed
  text is in the grid in clear.
- Also open: no frame history, no barrier or return value on `wait_frame`,
  `Esc`+key still reads as Alt, the mouse API is one button, no scrollback,
  and no `wait_frame_for`.
