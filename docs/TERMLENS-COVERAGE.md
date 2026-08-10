# What termlens 0.1 covers, and what it doesn't

Findings from testing `taskboard` — a TUI with tabs, a filtered list, a
detail pane, a modal dialog, a text input with a live cursor, a help
overlay, styled cells, CJK/emoji glyphs, a responsive layout, mouse
support and bracketed paste.

**39 tests, all passing.** `tests/tui.rs` (28) is the coverage; `tests/limits.rs`
(11) pins each gap below with a test that demonstrates it against the real
binary. Nothing here is inferred from reading the crate — every claim was
reproduced.

---

## 1. What it covers, and covers well

| Capability | How it's asserted | Test |
|---|---|---|
| Rendered text anywhere on screen | `contains` / `row_text` / `find` | most of `tui.rs` |
| Cursor position + visibility | `screen.cursor()` | `filter_mode_shows_a_live_cursor…` |
| Per-cell colour and attributes | `cell.style()` | `priority_and_status_are_colour_coded` |
| Double-width glyph layout | `is_wide` / `is_wide_continuation` | `wide_glyphs_occupy_two_columns` |
| Column arithmetic around wide glyphs | comparing border columns per row | `wide_glyphs_do_not_break_the_box_drawing` |
| Typed input incl. F-keys, Ctrl chords | `Key` | `help_overlay_opens_from_question_mark_and_f1` |
| Resize / SIGWINCH and responsive layout | `resize()` then re-assert | `resizing_narrow_drops_the_detail_pane` |
| Exit code, and signal-vs-code | `wait_exit()` → `ExitStatus` | `ctrl_c_exits_with_130` |
| Alt-screen teardown on exit | content vanishes after `wait_exit` | `quitting_restores_the_main_screen` |
| Whole-screen regression snapshots | `assert_screen_snapshot!` | the four `snapshot_*` tests |

The failure ergonomics are genuinely good: every timeout embeds the screen,
so a CI log shows the frame the app was displaying rather than
`assertion failed: false`. Several bugs in these tests were diagnosed from
the error text alone.

---

## 2. The one that will bite you: there is no frame boundary

**This is the most important limitation, and it is not obvious from the
docs.**

The reader thread feeds each chunk of PTY output into the emulator as it
arrives, and `wait_until` re-evaluates the predicate on every chunk. Nothing
marks where one repaint ends and the next begins. A predicate can therefore
fire on a **half-painted frame — including half a row**.

This was not theoretical. An early version of the boot test did:

```rust
t.wait_until(|s| s.contains("NORMAL"))?;      // status bar, last row
assert!(t.screen().contains("Tasks 1/10"));   // …also the status bar
```

It failed roughly 2 runs in 15 under parallel load, with this screen:

```
 NORMAL
```

`NORMAL` had landed; ` Tasks 1/10 ? help  q quit` — the rest of the *same
row* — was still in flight.

Three rules follow, and all of them are in use in `tests/tui.rs`:

1. **Put everything you assert into one predicate.** A `Screen` is a
   consistent instant, so a single predicate that checks all the conditions
   is race-free. Splitting into `wait_until(…)` then `assert!(screen…)` on a
   *different* region is a race.
2. **Wait on the last thing painted.** `common::spawn` waits for `"q quit"`,
   the rightmost text of the bottom row, not for `"NORMAL"`. Which text is
   "last" is a property of your app's render order — termlens can't tell you.
3. **Settle before whole-screen snapshots.** A snapshot asserts on cells the
   test never named, so a targeted predicate isn't enough; `common::settle`
   calls `wait_idle(100ms)` first. This is a heuristic, and the crate is
   honest about that — but it's the only tool available.

Rule 1 also breaks down after a **resize**, because the old frame is still
on the grid (merely clipped) until the app handles SIGWINCH:

```rust
t.resize(50, 20)?;
t.wait_until(|s| s.cols() == 50 && s.contains("tasks (10)"))?;  // ← both true of the STALE frame
```

The fix is to wait for something only the new geometry can produce — here,
a complete status bar on the last row of a 20-row screen.

> `wait_frame`, built on DEC mode 2026 (synchronized output), is on the
> crate's roadmap and would close this cleanly.

---

## 3. Input you cannot express

### 3.1 Mouse events
`Key` has no mouse variants. Any mouse-driven code path — click-to-select,
drag, scroll wheel — is reachable only by hand-encoding a report:

```rust
t.send_str("\x1b[<0;10;7M");   // SGR press, button 0, 1-based col;row
t.send_str("\x1b[<0;10;7m");   // release
```

That works (`mouse_clicks_require_hand_rolled_escape_bytes`), but the test
now owns a protocol detail the typed API exists to hide, and nothing checks
it against the tracking mode the app actually enabled (1000 / 1002 / 1006).

### 3.2 Modifier + special-key chords
`Key::Ctrl` takes a `char` and encodes a C0 byte, so **Ctrl-Right,
Shift-Up, Alt-PageDown and every other modifier+special-key combination
have no representation.** These are common TUI bindings. Workaround is the
raw CSI-modifier form, `"\x1b[1;5C"` for Ctrl-Right
(`ctrl_arrow_chords_are_not_expressible_as_a_key`).

### 3.3 Bracketed paste
A paste is one `Paste` event, not a burst of key presses, and `Key` cannot
produce one. You must write the wrapper literally:
`"\x1b[200~text\x1b[201~"` (`bracketed_paste_has_no_typed_api`). Focus
in/out events (`CSI I` / `CSI O`) are in the same position.

### 3.4 `Esc` is ambiguous on the wire, and there is no inter-key delay
`Key::Esc` sends a bare `0x1B`. Followed immediately by another key, the
bytes are *identical* to an Alt chord, and every input parser resolves it
the same way — as one Alt chord. Real keyboards are saved by the human
delay between presses; `send` writes back-to-back with none, and there is
no `send_after(delay)`.

```rust
t.send(Key::Esc);
t.send(Key::Char('?'));   // app sees Alt('?'), and the Esc is lost
```

Verified both ways in `esc_immediately_followed_by_a_key_is_read_as_alt`.
The fix is to make the first key's effect observable and wait for it before
sending the second — which only works if it *has* an observable effect.

### 3.5 Cursor-key mode is not tracked
`Key::Up` always sends `ESC [ A`, never the `ESC O A` that DECCKM
application-cursor mode implies. Mainstream parsers accept both, so this is
latent rather than active — but an app with a strict hand-rolled parser
would see the wrong key. Documented in the crate's `keys.rs`.

---

## 4. State you cannot observe

### 4.1 No scrollback — at all
The emulator is built with **zero** scrollback rows. Anything scrolled off
the top is unrecoverable (`output_scrolled_off_the_top_is_unrecoverable`),
and resizing does not reflow. Fine for a full-screen TUI; a hard wall for a
log-spewing CLI where the interesting line is 200 rows back.

### 4.2 Styles are captured but never snapshotted
`Cell::style()` exposes fg, bg, bold, dim, italic, underline and reverse —
and assertions on them work well. But the `Display`/snapshot format is text
only, so **a regression that changes only styling is invisible to
`assert_screen_snapshot!`**. Moving the selection highlight from row 1 to
row 2 leaves the snapshot text byte-identical
(`moving_the_highlight_does_not_change_the_snapshot_text`). Style coverage
has to be written cell by cell, by hand. (`with_styles()` is slated for v0.2.)

### 4.3 Terminal state that isn't a cell
`Screen` is a grid plus a cursor position and visibility flag. There is no
accessor for:

- the **window title** the app set via OSC 0 (vt100 tracks it; termlens
  doesn't surface it)
- whether the **alternate screen** is active — only inferable, e.g. from the
  frame vanishing on exit
- **cursor shape or blink** (`DECSCUSR`)
- **OSC 8 hyperlink** targets
- **OSC 52 clipboard** writes
- **sixel / kitty graphics** — not modelled by the vt100 backend

Pinned in `out_of_band_terminal_state_is_only_observable_by_inference`.

### 4.4 stdout and stderr are one stream
A PTY has a single output stream, so "assert this went to stderr" is not a
question termlens can answer (`stdout_and_stderr_are_indistinguishable`).
Inherent to PTY testing, not a termlens defect — but it does mean stderr
diagnostics can't be tested separately from UI output.

---

## 5. The terminal never answers back

termlens renders what the app writes; it never writes back. An app that
*asks* the terminal a question gets silence:

- DSR cursor-position report (`CSI 6 n`)
- DA1/DA2 device attributes
- OSC 11 background-colour query (used for light/dark detection)
- kitty keyboard-protocol capability probes
- `XTGETTCAP`

In `terminal_queries_are_never_answered`, a shell issues `CSI 6 n` and
blocks on `read`; the wait can only end in a timeout. **A TUI that probes
capabilities at startup will hang under test rather than fail informatively**
— and the timeout error will point at your predicate, not at the unanswered
query, which makes it an unpleasant thing to debug.

Worth knowing before adopting: crossterm and ratatui don't probe by default,
which is why `taskboard` works. Libraries that *do* — or your own
light/dark detection — will not.

---

## 6. Query API gaps

- **`find` is single-row only.** `contains` joins rows with `\n` and matches
  across them; `find` scans row by row and returns `None` for a multi-row
  needle (`find_cannot_locate_text_that_spans_rows`). Locating a box-drawn
  widget means finding one row and doing the arithmetic yourself.
- **No region queries.** No "text within this rect", no "find the bold
  text", no "which cells are reversed". `tests/common/mod.rs` grew a
  `pane_text(screen, row, cols)` helper within the first hour; something
  like it belongs in the crate.
- **No style-aware search.** Finding *the highlighted row* means scanning
  every cell yourself.

---

## 7. Process and platform

- **No working directory on the builder.** `TerminalBuilder` has `size`,
  `timeout`, `arg`/`args`, `env`/`env_clear` — but no `current_dir`. Testing
  a CLI that behaves differently per directory means `cd … && …` through a
  shell.
- **No signal delivery.** The child's pid isn't exposed and there's no
  `kill`/`signal` method, so graceful-shutdown-on-SIGTERM and
  signal-death paths can't be tested. `Drop` kills, and that's the only
  lever.
- **One timeout per `Terminal`.** The builder's `timeout` applies to every
  `wait_*`; there's no per-call override. A suite that wants one slow wait
  and many fast ones has to pick the slow value for all of them — which is
  what makes a genuinely hung app take the full timeout on the *first*
  failing predicate.
- **`send` panics rather than returning `Result`** if the child is gone. A
  deliberate, defensible choice — but it means "app died mid-input" can't be
  handled gracefully in a test.
- **Unix only.** `portable-pty` speaks ConPTY, the harness doesn't yet.
- **Instant-exit output loss.** A child that writes and exits within its
  first milliseconds can lose output to PTY teardown, macOS especially. The
  crate's advice is to end such scripts with a `read`; every `spawn_sh`
  helper in `tests/limits.rs` does exactly that.
- **Spawn/teardown serialize process-wide.** A global lifecycle mutex
  (correctly — it fixes a real macOS `revoke()` race) means a large parallel
  suite queues on PTY setup and teardown.

---

## 8. Determinism notes

Nothing here is termlens's fault, but it constrains what you can test:

- **Animations and clocks make snapshots flaky.** `taskboard` deliberately
  has neither. A spinner would need to be frozen behind a test flag.
- **`wait_idle` is evidence, not proof.** "No output for N ms" can resolve
  early on a slow machine mid-render, and costs a real N ms every call.
- **`env_clear()` is essential and easy to forget.** Without it a
  developer's `LS_COLORS`, `COLORTERM` or `NO_COLOR` can change a snapshot.

---

## Summary

| # | Gap | Workaround | Pinned by |
|---|---|---|---|
| 1 | No frame boundary; torn frames | one predicate; wait on last-painted text; `wait_idle` before snapshots | `a_frame_is_only_complete_once_its_last_cell_arrives` |
| 2 | No mouse in `Key` | hand-rolled SGR bytes | `mouse_clicks_require_hand_rolled_escape_bytes` |
| 3 | No modifier+special-key chords | raw CSI-modifier bytes | `ctrl_arrow_chords_are_not_expressible_as_a_key` |
| 4 | No bracketed paste / focus events | literal `ESC[200~…ESC[201~` | `bracketed_paste_has_no_typed_api` |
| 5 | `Esc`+key reads as Alt; no send delay | wait on the Esc's effect first | `esc_immediately_followed_by_a_key_is_read_as_alt` |
| 6 | Terminal never answers queries | none — app hangs, test times out | `terminal_queries_are_never_answered` |
| 7 | Zero scrollback, no reflow | size the screen to fit | `output_scrolled_off_the_top_is_unrecoverable` |
| 8 | Styles absent from snapshots | per-cell assertions by hand | `moving_the_highlight_does_not_change_the_snapshot_text` |
| 9 | No title / alt-screen / cursor-shape / OSC 8 / OSC 52 | infer from the grid | `out_of_band_terminal_state_is_only_observable_by_inference` |
| 10 | stdout and stderr merged | none (inherent to PTYs) | `stdout_and_stderr_are_indistinguishable` |
| 11 | `find` is single-row | find a row, compute offsets | `find_cannot_locate_text_that_spans_rows` |
| 12 | No `current_dir`, no signals, one timeout | shell wrappers | — |

**Verdict.** For a keyboard-driven, full-screen TUI — the case it targets —
termlens covers essentially everything that matters, and the screen-carrying
errors make failures cheap to diagnose. The gaps worth knowing before you
adopt it are **#1** (it will produce flaky tests until you learn the
idioms), **#6** (a capability-probing app hangs), and **#8** (snapshots
silently ignore styling).
