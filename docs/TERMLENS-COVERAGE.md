# termlens 0.2 — what improved, what's left

Re-run of the 0.1 study against the same subject: `taskboard`, a TUI with
tabs, a filtered list, a detail pane, a modal dialog, a text input with a
live cursor, a help overlay, styled cells, CJK/emoji glyphs, a responsive
layout, mouse support, bracketed paste — now also DEC 2026 synchronized
repaints and a SIGTERM shutdown path.

**49 tests, all passing, 0 failures in 20 stress runs.** `tests/tui.rs` (40)
is the coverage; `tests/limits.rs` (9) pins what remains. As before, nothing
here is inferred from reading the crate — every claim was reproduced.

Headline: **11 of the 12 gaps from the 0.1 study are closed.** The one that
mattered most — no frame boundary — is not merely closed but inverted into
an advantage.

---

## 1. What 0.2 fixed

| 0.1 finding | 0.2 |
|---|---|
| **No frame boundary; torn frames** | `wait_frame` — DEC 2026 synchronized updates |
| No mouse in `Key` | `click`, `scroll`, `MouseMode`, mode-aware encoding |
| No modifier + special-key chords | `Chord`: `Key::Right.ctrl()` |
| No bracketed paste | `paste()`, wraps only if the app enabled 2004 |
| Terminal never answers queries | answers DSR / DA1 / DA2 / `18t` / OSC 10–11 |
| Styles absent from snapshots | `Screen::with_styles()` |
| No title / alt-screen / input modes | `title()`, `alternate_screen()`, `bracketed_paste()`, `application_cursor()`, `mouse_mode()` |
| `find` was single-row | `find` takes multi-row needles; plus `find_by`, `rect_text` |
| No `current_dir` | `TerminalBuilder::current_dir` |
| No signals, no pid | `Terminal::signal(Signal::Term)`, `pid()` |
| One timeout for every wait | `wait_until_for(pred, timeout)` |
| Cursor-key mode not tracked | `send` picks the DECCKM form automatically |

### 1.1 `wait_frame` is the release

The 0.1 study's main finding was that `wait_until` re-evaluates on every
chunk of PTY output, so a predicate can fire on a half-painted frame —
including half a row. That produced a genuinely flaky test at ~2 runs in 15,
and forced three idioms on the whole suite: one combined predicate per
assertion, anchored on whatever the app painted last, plus a `wait_idle`
settle before every snapshot.

With `wait_frame` all of that disappears. Compare the same test:

```rust
// 0.1 — everything asserted has to be in one predicate, and you have to
// know that the status bar's *tail* is the last thing painted.
t.send(Key::Esc);
t.wait_until(|s| s.contains("tasks (10)") && !s.contains("filter:"))?;

// 0.2 — a frame is published only when it is complete.
t.send(Key::Esc);
t.wait_frame(|s| s.contains("tasks (10)"))?;
assert!(!t.screen().contains("filter:"));
```

`common::spawn` shows it most plainly: it now waits for `"NORMAL"`, the
exact predicate that was racy in 0.1 and had to be replaced with `"q quit"`.
Snapshots no longer need settling either — the frame `wait_frame` returns on
is complete by construction, so the `settle()` helper is gone.

**It also does something the 0.1 study didn't ask for.** A completed frame is
*retained*, so a frame the app immediately overwrites is still observable.
taskboard draws a `SAVING` frame on SIGTERM and then wipes it leaving the
alt screen. Measured over 5 runs each:

| | caught `SAVING` |
|---|---|
| `wait_frame` | **5 / 5** |
| `wait_until` | 2 / 5 |

`wait_until` is racing the teardown; `wait_frame` is reading a frame that
already happened. That turns "assert on a transient state" from unreliable
into routine.

The error when an app emits no synchronized updates is a model of its kind —
it names the cause and the remedy rather than just reporting a timeout:

> a complete frame — but the application never emitted a DEC 2026
> synchronized update. wait_frame needs repaints bracketed in
> BeginSynchronizedUpdate/EndSynchronizedUpdate; for other apps use
> wait_until

### 1.2 Input is mode-aware, not just typed

The new input APIs read the modes the application actually set, rather than
assuming:

- `paste("core")` wraps in `ESC[200~ … ESC[201~` **only because** the app
  enabled mode 2004; without it the bytes go through plain, like a real
  terminal.
- `click(col, row)` encodes for the tracking mode the app enabled (SGR vs
  legacy), sends a release only if that mode reports one, and **refuses with
  a clear error** if the app enabled no tracking at all:
  `the application has not enabled mouse tracking (no CSI ?9/?1000/?1002/?1003 h was seen)`.
  Feeding mouse bytes to an app that isn't listening — which is what 0.1's
  hand-rolled workaround did silently — is now impossible by accident.
- `send(Key::Up)` emits `ESC O A` instead of `ESC [ A` while the app has
  DECCKM set.

Reading `mouse_mode()` back is genuinely useful: it revealed that
crossterm's `EnableMouseCapture` turns on **1003 (any-motion)**, not 1000.

### 1.3 Query answering

0.1's sharpest failure mode was that a capability-probing app hung forever
and the timeout blamed your predicate. 0.2 answers the common probes, and
the answers are *real* rather than canned — asking for the cursor position
from row 5, column 10 replies `ESC[5;10R`:

| query | reply |
|---|---|
| `CSI 6 n` (DSR cursor) | `ESC[<row>;<col>R`, the true cursor |
| `CSI 5 n` (status) | `ESC[0n` |
| `CSI c` (DA1) | `ESC[?62;22c` — VT220 + ANSI colour |
| `CSI > c` (DA2) | `ESC[>1;10;0c` |
| `CSI 18 t` (text area) | `ESC[8;<rows>;<cols>t` |
| `OSC 10/11` (fg/bg colour) | `rgb:rrrr/gggg/bbbb`, background settable via `background_rgb` |

DA1 deliberately claims only what the emulator can render — no sixel, no
kitty graphics. That's the honest choice: an app that trusts the reply won't
emit output the grid would mangle.

For the probes it still can't answer, the timeout now names them:

> — note: the application queried the terminal (`^[[?u`) and received no
> answer; if it is blocked waiting for that reply, this is the cause

`answer_queries(false)` reproduces 0.1's behaviour deliberately, and keeps
the diagnostic.

### 1.4 Styles in snapshots

`with_styles()` appends a per-row span block to the snapshot:

```
4: 1-4 fg=2 bold reverse; 5-9 fg=1 bold reverse; 10-31 dim reverse; 42-50 fg=6
5: 1-4 fg=2; 5-9 fg=1 bold; 10-33 dim; 42-50 fg=6
25: 0-7 fg=15 bg=4 bold; 20-33 fg=8
```

Moving the selection highlight one row now changes the diff — the exact
regression that 0.1's text-only snapshots could not see. Compact enough to
read in a review, and plain snapshots stay text-only, so it's opt-in per
assertion rather than a format change.

### 1.5 Smaller things that removed real friction

- **`find_by(|c| c.style().reverse)`** answers "where did the highlight go"
  in one call. 0.1 needed a hand-written cell scan.
- **`rect_text(0..40, 4..8)`** isolates a pane. The 0.1 suite grew exactly
  this helper by hand within the first hour.
- **`signal(Signal::Term)` + `pid()`** made the graceful-shutdown path
  testable at all — and `signal` refuses once the child is reaped, rather
  than letting you signal a recycled pid.
- **`Error::screen()`** gives uniform access to the screen from any error.
- **`wait_until_for`** takes the one known-slow wait off the builder timeout.

---

## 2. What is still a limitation

Nine, down from twelve — and none of them is the kind of thing that made
0.1's suite flaky.

### 2.1 `wait_frame` retains one frame, not a history
Only the most recently completed frame is kept, so when several complete
inside one read burst the earlier ones are unreachable. Measured: three
frames emitted in a single write, and catching the first is a coin flip
(1/3); with pauses between them, 3/3. A progress counter ticking 1→2→3 in
one burst is only ever observable at 3. Pinned by
`only_the_newest_frame_of_a_burst_survives`, which makes it deterministic by
waiting for the last frame first and then showing the first is gone.

### 2.2 `wait_frame` requires the application to opt in
It works only for apps that bracket repaints in DEC 2026. For a plain CLI,
or a TUI that hasn't opted in, it can never succeed and you are back to
`wait_until` and the full 0.1 discipline. taskboard had to be modified to
emit synchronized updates before any of this suite could use it — worth
knowing that the headline feature is conditional on the subject's
cooperation. (`wait_frame_is_useless_without_synchronized_updates`)

### 2.3 `Esc` followed by a key is still read as Alt
Byte-identical to an Alt chord, and there is still no `send_after(delay)` —
so whether the app sees one Alt chord or two key presses depends on whether
its input loop happens to read the two writes together. Now clearly
documented on `Key::Esc`, with the wait-for-the-effect remedy. The remedy
only works when the `Esc` *has* an observable effect.

Worth stressing because it caught me twice: a test that asserted the merged
outcome was itself flaky at ~1 run in 5 once the app switched from a
blocking read to a poll loop. `esc_immediately_followed_by_a_key_is_still_read_as_alt`
now sends `"\x1b?"` as one write to make the hazard deterministic.

### 2.4 The mouse API is one button
`click` sends button 0 and `Scroll` has only `Up`/`Down`. No right- or
middle-click, no drag, no modifier+click (Ctrl-click to multi-select is a
common TUI idiom), no horizontal wheel. These still need hand-encoded SGR
bytes — exactly what *all* mouse input needed in 0.1. Given how well
`click` reads the app's mode, the asymmetry stands out.
(`right_click_and_drag_still_need_hand_rolled_bytes`)

### 2.5 Some queries remain unanswerable
The kitty keyboard probe (`CSI ? u`), DA3, OSC 12, `DECRQM`, `XTGETTCAP`. An
app that blocks on one still hangs — but the timeout names it, which turns
a strace-level mystery into a one-line diagnosis.

### 2.6 No scrollback
Zero scrollback rows, and resizing does not reflow. Anything scrolled off
the top is unrecoverable. Fine for a full-screen TUI, still a hard wall for
a log-spewing CLI. Listed in the crate's own known limitations.

### 2.7 stdout and stderr are one stream
Inherent to PTYs, not a defect — but "assert this went to stderr" remains
unanswerable.

### 2.8 Per-call timeouts only on `wait_until`
There is no `wait_frame_for` or `wait_idle_for`. A frame-driven suite that
needs one long wait must raise the builder timeout for every wait it makes.
Since `wait_frame` is now the recommended primitive, this is the gap most
likely to be felt next. (`only_wait_until_takes_a_per_call_timeout`)

### 2.9 Still outside the model
Cursor shape and blink (`DECSCUSR`), OSC 8 hyperlink targets, OSC 52
clipboard writes, sixel/kitty graphics. Unix only. `send` still panics
rather than returning `Result` when the child is gone.

---

## Verdict

0.2 closes the gaps that made 0.1 awkward, and closes them properly rather
than superficially: the input APIs read the application's real modes, the
query answers are computed rather than canned, and the error messages name
causes. The suite that needed three defensive idioms in 0.1 now reads like
ordinary integration tests, and got *shorter* while covering more.

If you want a ranking of what to do next, from this study:

1. **`wait_frame_for`** (§2.8) — smallest change, and `wait_frame` being the
   recommended primitive makes the missing override conspicuous.
2. **A fuller mouse API** (§2.4) — button, drag, modifiers. The one place
   0.2's own standard isn't met.
3. **A short frame history** (§2.1) — even two or three retained frames
   would make rapid intermediate states assertable.

Nothing in the remaining list threatens correctness or produces flaky tests.
That was not true of 0.1.
