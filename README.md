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

**150 tests**, against termlens 0.6.0.

- **`tests/tui.rs`** (42) — what termlens covers: text, per-cell styles,
  cursor, wide glyphs, keys and chords, the full mouse API, paste, resize,
  terminal state, signals, exit codes, snapshots.
- **`tests/hard.rs`** (23) — the hard cases driven against the application:
  the board, the frame burst, the capability probe, the clipboard payload,
  the three style attributes — and one passing test per thing taskboard
  demonstrably does that *still* no assertion can reach.
- **`tests/limits.rs`** (13) — one passing test per remaining limitation.
  Mostly **bounds** now rather than absences, and each asserts the mechanism:
  four of the 0.2 pins stayed green against 0.4 while their claims went
  false, and six more did the same on the way to 0.6.
- **`tests/survey.rs`** (44) — the same questions asked of plain `/bin/sh`,
  so each finding is isolated from any application.
- **`tests/survey_0_2_1.rs`** (18) — what 0.2.1 changed, and what the new
  code brought with it.
- **`tests/survey_0_6_0.rs`** (10) — the inline-graphics surface, probed with
  hand-written escapes whose every byte is known: images counted as images
  rather than as escapes, deletes counted apart, placement, and the pixels
  themselves.

The survey suites print their evidence under `--nocapture`.

## Findings — termlens 0.6

**[docs/TERMLENS-COVERAGE.md](docs/TERMLENS-COVERAGE.md)** is the write-up:
the 0.1 → 0.2 study, a deeper pass against 0.2.1 with this harder subject,
§7–9 for 0.4, then §10–12 for 0.6. **Three of the five items §9 ranked have
shipped** — the same score as last time.

- **A finding about the study first, again.** Bumping the dependency and
  changing nothing else left 5 tests failing and **6 passing that should have
  failed** — in three shapes, only one of which the 0.4 pass had seen. A pin
  that asserted a *symptom* (the bell leaves the grid untouched — still true,
  now beside the point). A pin that only **printed** its measurement, so it
  could not fail when the thing it watched for was fixed. And a pin whose
  assertion was **true for opposite reasons**: a `DECRQM` timeout lacking a
  phrase, which held both when the query was unrecognised and once it was
  answered.
- **§9's number-one item shipped, and this suite reported it to a log.**
  Reply loss is gone up to 1000 queries, on Linux and macOS alike, where
  0.2.1 lost 715 of 1000. The two tests watching for it had no assertions.
  They do now — and finding the right ceiling cost a wrong claim first: the
  assertion went in at 1500 from a green run on one machine, and a loaded
  Linux runner delivered 376. That loss is the kernel's `n_tty` discarding,
  not termlens's queue, and it is the third time in this pass that a
  measurement looked like a fact.
- **A branch of the subject that had never run.** taskboard dims its status
  bar when the terminal reports lost focus, and before 0.5 no input could
  enter that branch — not untested, unreachable. `focus_in`/`focus_out` reach
  it, and the test now crosses the boundary in both directions.
- **Inline graphics: the assertion no cell can carry.** An image transmitted,
  chunked, deleted, placed — and, with the `decode` feature, the pixels
  themselves. Nothing on screen changes for any of it, which is exactly why
  it needed an API rather than a needle.
- **`XTGETTCAP` is answered**, so all six of taskboard's startup capability
  probes now come back; and `contains`/`find` fold both sides to **NFC**, so
  the needle a test author types finds text the application normalized the
  other way.
- **Two breaking changes, 113 call sites**: `send`/`send_str`/`paste` return
  `Result` (one `?` each), and `ExitStatus::code` returns `Option<u32>`. Plus
  one silent change that cost more to find than either: `drag` now reports one
  motion per cell crossed, which pushed a fixed-size wire read off the end of
  the gesture it was measuring.
- Still open: the 222 guard firing before the encoding match, styles in
  scrollback and in `rect_text`, `OSC 52` reads, hyperlink targets,
  `DECSCUSR`, palette overrides — and no opt-out from a torn `screen()`.
