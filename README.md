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

`--probe-sync` was the sharp one: the application brackets its repaints only
if the terminal says it supports synchronized output, and under 0.2 termlens
did not recognise the query that asks — so the same binary became completely
untestable by frame. 0.3 answers it, and the probe now comes back `yes`.

## Tests

```sh
cargo test
```

**140 tests**, 15/15 clean stress runs.

- **`tests/tui.rs`** (42) — what termlens covers: text, per-cell styles,
  cursor, wide glyphs, keys and chords, the full mouse API, paste, resize,
  terminal state, signals, exit codes, snapshots.
- **`tests/hard.rs`** (23) — the hard cases driven against the application:
  the board, the frame burst, the capability probe, the clipboard payload,
  the three style attributes — and one passing test per thing taskboard
  demonstrably does that *still* no assertion can reach.
- **`tests/limits.rs`** (13) — one passing test per remaining limitation.
  Mostly **bounds** now rather than absences, and each asserts the mechanism:
  four of the 0.2 pins stayed green against 0.4 while their claims went false.
- **`tests/survey.rs`** (44) — the same questions asked of plain `/bin/sh`,
  so each finding is isolated from any application.
- **`tests/survey_0_2_1.rs`** (18) — what 0.2.1 changed, and what the new
  code brought with it.

The survey suites print their evidence under `--nocapture`.

## Findings — termlens 0.4

**[docs/TERMLENS-COVERAGE.md](docs/TERMLENS-COVERAGE.md)** is the write-up:
the 0.1 → 0.2 study, a deeper pass against 0.2.1 with this harder subject,
then §7–9 for 0.4. **Three of the five items §6 ranked have shipped.**

- **A finding about the study first.** Bumping the dependency and changing
  nothing else left 9 tests failing and **9 passing that should have failed**.
  Each of those nine asserted a *symptom* the closed gap still shares with its
  replacement — "no scrollback" pinned on the visible grid, which lacks the
  text either way. A pin has to assert the mechanism: count the frames, count
  the history rows, name the API, read the reply. Two of them had *two*
  independent reasons to pass, neither the claim.
- **The sharpest gap was one unrecognised query, and it is answered.** With
  `--probe-sync` the same unmodified binary goes from completely
  untestable-by-frame to fully frame-testable. Highest-leverage change across
  three studies.
- **The style model closed a trap.** A test asserting the credentials field is
  masked used to pass against an application printing the secret in the clear;
  `conceal`, `blink` and `strikethrough` now make the two different values.
  The styled snapshot picks up `dim strikethrough` and `fg=1 blink` against a
  taskboard nobody changed.
- **`wait_frame` stopped being able to lie**: a superseded frame, a
  pre-resize frame, and an unreachable matched frame were three ways a test
  could pass while proving nothing. All three closed by one consumption
  cursor, plus a returned `Screen`.
- **Also new**: scrollback, `OSC 52` payloads, a per-call deadline on every
  wait — and a fix nobody wrote down, where a write to a dead child now
  panics as documented instead of being silently discarded.
- Still open: reply backpressure, the 222 guard firing before the encoding
  match, `send` panicking rather than returning `Result`, focus events,
  hyperlink targets, `BEL`, cursor shape, palette overrides, and styles in
  scrollback. Plus the new bounds: eight retained frames, torn `screen()`
  reads, and bounded unreflowed text-only history.
