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

> **Correction (issue #1).** `DECRQM` was *not* named. It was neither
> answered nor diagnosed: the `$` intermediate set `csi_invalid`, so the
> tracker never classified it as a query at all, and an application probing
> it hung with no note. §5.1 has the end-to-end consequence. `DECRQM` is
> answered as of 0.3 — see §7.2.

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

---

# termlens 0.2.1 — a deeper pass, and a harder subject

Everything above was written against 0.2.0 with a subject that stayed
inside what a screen grid can express. This section is the result of two
things: upgrading to **0.2.1**, and rebuilding `taskboard` so it
deliberately does the things a screen-grid harness struggles with.

**135 tests, all passing, 5/5 clean stress runs.** `tests/tui.rs` (40) is
the coverage, `tests/limits.rs` (9) pins the 0.2 gaps, `tests/hard.rs` (23)
drives the hard cases against the application, and `tests/survey.rs` (45)
plus `tests/survey_0_2_1.rs` (18) isolate each finding against plain
`/bin/sh`. As before, nothing here is inferred from reading the crate.

## 3. What 0.2.1 changed

| change | effect |
|---|---|
| `paste` rewrites line breaks to `\r`, collapses `\r\n` | closes the one fidelity gap the 0.2.0 pass found in paste |
| `paste` strips embedded `ESC[200~`/`ESC[201~` to a fixed point | paste injection can no longer end the paste early |
| UTF-8 mouse encoding (mode 1005) | `click` emits `c2 85` where the legacy form emits a bare `85` |
| builder validation | zero dimensions, empty program name and a missing `current_dir` are typed errors instead of a wedged terminal |
| a dedicated responder thread | a query reply can no longer block the drain and deadlock the harness |
| richer query diagnostics | several unanswered queries named at once, an overflow count past eight, a note on `Eof` errors too, and a causation heuristic |
| bounded `Drop` reap | a wedged child can no longer hang the test binary forever |

The causation heuristic is the nicest of these. A query the application
asked and then moved past now reads *"…and received no answer, but produced
output afterwards, so that is probably not why this wait failed"*, instead
of being presented as the cause.

`src/screen.rs`, `src/emu/seq.rs`, `src/error.rs` and `src/wait.rs` are
byte-identical to 0.2.0, so every §2 gap that lives in those files survives.

## 4. New in 0.2.1

### 4.1 The UTF-8 mouse encoding cannot reach the coordinates it exists for
`mouse_report` checks `col > 222 || row > 222` *before* it consults the
encoding, so with mode 1005 active a click at column 300 is refused — and
the message names the wrong scheme:

> `(300, 5) is unrepresentable in the legacy mouse encoding the application selected (max 222)`

Mode 1005 exists precisely to carry coordinates past that limit (xterm
reaches ~2015), and 0.2.1 implements the encoding correctly. Only the guard
stands in the way. (`survey_0_2_1::v8`; `v7` shows the encoding itself
works.)

### 4.2 Query replies are dropped past roughly a hundred unread
The responder queue is 64 deep and drops when full. The premise in the
source — *"reached only when the application has stopped reading its input
entirely, in which case it cannot be waiting on these bytes"* — is
falsified by batch-probe-then-read, a legitimate startup pattern. Measured,
with the application reading everything after a pause:

| queries asked | replies received |
|---|---|
| 50 | 50 |
| 200 | **94** |
| 400 | 161 |
| 600 | 231 |
| 1000 | 285 |

A real terminal backpressures; termlens discards. The only signal is a note
that appears *if some later wait fails* — in `v14` the application carried
on with 395 of 1500 answers and nothing failed, so nothing warned. Ordinary
applications issuing a handful of probes are unaffected.

### 4.3 Only the lower size bound is guarded
`check_size` rejects zero at both `spawn` and `resize`, with a clear
message. Nothing caps the top, and snapshot cost is O(area) with a `String`
per cell. First `screen()`: 80×24 → 267µs, 500×500 → 34ms, 1500×1500 →
**331ms**. Since `wait_until` rebuilds a snapshot per state change, a large
grid quietly reduces a timeout budget to a handful of evaluations.
(`survey_0_2_1::v2`)

### 4.4 Smaller
- `paste` is no longer byte-transparent: the newline rewrite applies even
  when bracketed paste is off, so no `paste` call can deliver a literal LF.
  `send_str` is the escape hatch. (`v6`)
- Replies now go out on a duplicated descriptor from the responder thread
  while `send` writes on the original under a lock; nothing orders the two.
  Not reproduced — 8/8 runs kept reply-before-keystroke — so this is a note
  about construction, not a defect. (`v16`)
- `current_dir` pointing at an existing *file* reports "is not an existing
  directory". Accurate, mildly confusing. (`v3c`)

## 5. The hard cases, against a real application

`taskboard` now emits, on purpose, what a grid cannot hold: an `OSC 8`
hyperlink, an `OSC 52` clipboard write, an `OSC 4` palette override,
`DECSCUSR` cursor shapes, `BEL`, focus events, strikethrough, blink,
conceal, a burst of complete frames, and — behind `--probe-sync` — a
`DECRQM` capability probe. `tests/hard.rs` records what survives the trip.

### 5.1 A capability probe turns `wait_frame` off entirely
The headline. With `--probe-sync` the application does what a careful
application does: it asks `CSI ? 2026 $ p` whether the terminal supports
synchronized output, and brackets its repaints only if the answer says yes.

termlens **implements** DEC 2026 — `wait_frame` is built on it — but does
not **recognise the query that advertises it**: the `$` intermediate sets
`csi_invalid`, and `csi_final` returns before classification. So the probe
goes unanswered, the application concludes there is no support, stops
emitting synchronized updates, and `wait_frame` can never succeed against
it. The error then blames the application:

> a complete frame — but the application never emitted a DEC 2026
> synchronized update.

and no note names the query that caused it. The same binary without the
flag is fully frame-testable. One unrecognised query is the whole
difference. (`hard::probing_for_synchronized_output_turns_wait_frame_off_entirely`)

Of the six probes a capability-hungry app fires at startup, five are
answered; XTGETTCAP is the one that is not.
(`hard::five_of_six_startup_capability_probes_are_answered`)

### 5.2 A burst of frames, from a real event loop
`r` runs the selected task: the loop paints 0%, 10%, … 100% and a closing
frame back to back with nothing pacing them. Every one is a complete DEC
2026 frame; only the last is reachable. §2.1 held against a `printf`
fixture, and it holds against an application.
(`hard::only_the_last_frame_of_a_progress_burst_is_observable`)

The transient toast, by contrast, *is* catchable — because it ends the
burst rather than sitting inside it. That is the same property that makes
the `SAVING` frame observable on SIGTERM.

### 5.3 What the application does that no test can see

| the application does | the harness sees |
|---|---|
| strikes through finished titles (`SGR 9`) | only the dim; a struck-through dim title and a plain dim one are the same value |
| blinks the overdue badge (`SGR 5`) | a red cell, no blink attribute anywhere |
| conceals the credentials field (`SGR 8`) | **the secret, in clear** — and no marker that it was concealed |
| links "open ref" to a URL (`OSC 8`) | the label; the target is nowhere |
| copies the title to the clipboard (`OSC 52`) | only the app's own toast |
| rings the bell on a rejected key (`BEL`) | a byte-identical screen |
| switches the cursor to a bar (`DECSCUSR`) | position and visibility, never shape |
| redefines palette slots 1-6 (`OSC 4`) | `Color::Indexed(1)`, unchanged, whatever it now paints |
| dims its chrome when the window loses focus (mode 1004) | the focused branch only — no API delivers a focus event |

The conceal row is the one worth pausing on: a test asserting that a
password field is masked would pass against an application that prints it
in clear, and fail against one that masks it properly only if it happened
to check the styles too — which cannot distinguish them either.

### 5.4 Text fidelity, in an application rather than a fixture
- The credentials task's title carries a decomposed `é`. `contains("café
  credentials")` — the needle a test author would type — misses.
  (`hard::the_nfd_title_does_not_match_an_nfc_needle`)
- The audit task mixes a ZWJ sequence, a regional-indicator flag and a VS16
  emoji. The grid's column accounting for all three differs from what a
  terminal draws, so the rendered row is not the row a user sees.
- A title ending in three real spaces is indistinguishable from one padded
  to the same width by the list widget: the identical assertion passes for
  both. A trailing-whitespace regression inside a padded pane is not
  assertable at all.

## 6. Ranking, revised

1. **Answer `DECRQM` for 2026** (§5.1) — still the highest leverage in the
   crate, and now demonstrated end to end against an application. It is the
   detection path the headline feature depends on.
2. **Give `wait_frame` a barrier and a return value** — count only frames
   completed after the call, and return the matched `Screen`. Closes three
   ways a test can pass while proving nothing.
3. **Move the 222 guard inside the encoding match** (§4.1) — the smallest
   fix here, on code that just shipped.
4. **Backpressure rather than drop** (§4.2) — a discarded answer should not
   be discoverable only through a note in an unrelated timeout.
5. **`strikethrough`, `blink`, `conceal` on `Style`** — vt100 already
   exposes them, and §5.3 shows what their absence costs.

> **Correction.** vt100 does *not* expose them. `Attrs` is
> `{ fgcolor, bgcolor, mode: u8 }` using five of eight bits, and the SGR
> dispatch handles only `0 1 2 3 4 7 22 23 24 27` plus the colour params, so
> `5`/`8`/`9` and their resets are dropped before they reach a cell. Shipped
> in 0.4 anyway, by parsing the stream a second time — see §7.3.

---

# termlens 0.4 — three of five ranked items, and a lesson about pins

Same subject, same suite, two releases later: 0.2.1 → 0.4.0. **140 tests, all
passing, 0 failures in 15 stress runs** (`tui` 42, `survey` 44,
`survey_0_2_1` 18, `hard` 23, `limits` 13).

Against §6's ranking: **items 1, 2 and 5 shipped.** Items 3 (the 222 guard)
and 4 (reply backpressure) are still open, and `v8`/`4.2` still pin them.

## 7. What 0.3 and 0.4 changed

### 7.1 A finding about this study, before any finding about the crate

Bumping the dependency and changing *nothing else* left **9 tests failing and
9 passing that should have failed.** The nine that failed are the real
inversions. The nine that passed are the interesting ones, because every file
here opens with the same promise:

> Every test passes and pins *current* behaviour, so closing the gap will
> fail the test that encodes it.

It did not hold. Each of these asserted a **symptom** that the closed gap
still shares with its replacement:

| pin | what it actually asserted | why that survived |
|---|---|---|
| `limits::only_the_newest_frame_of_a_burst_survives` | an earlier frame is unreachable | also true once the frame has merely been *consumed* |
| `limits::output_scrolled_off_the_top_is_unrecoverable` | the visible grid lacks the text | the visible grid lacks it either way |
| `limits::right_click_and_drag_still_need_hand_rolled_bytes` | hand-rolled SGR bytes work | nothing removed the ability to send raw bytes |
| `limits::only_wait_until_takes_a_per_call_timeout` | `wait_idle` honours the builder timeout | it still does; the *override* is what was added |
| `survey::b2_osc52_clipboard_write_is_unobservable` | base64 stays off the grid | still true, and never the claim |
| `survey::d3_decrqm_probe_..._nor_named` | the timeout names no query | true when answered *or* unanswered — and its cooked-mode `read` could never see a reply anyway |
| `survey::d4_osc10_foreground_is_always_white` | nothing; it printed | a print cannot fail |
| `hard::the_clipboard_write_is_unobservable` | base64 stays off the grid | as above |
| `hard::only_the_last_frame_of_a_progress_burst_is_observable` | 50% is unreachable | it had been consumed by the wait for the end of the burst |

**A pin must assert the mechanism.** Count the frames. Count the history
rows. Name the API. Read the reply. The rewritten versions do, and they are
what this section's numbers are based on.

Two of the nine were worse than uninformative: `d3` and `d4` had *two*
independent reasons to pass, neither of them their claim. And `d3` is the
`DECRQM` finding — the top of §6's ranking — so the study's own headline ask
had a test that could not have observed it being granted.

### 7.2 `DECRQM`, end to end

The highest-leverage item in three studies, and the one whose absence
§5.1 traced to a single unclassified query. 0.3 answers it, so the same
`--probe-sync` binary that was **completely untestable by frame** now works
unmodified:

```
DECRQM ?2026 supported: yes
```

`hard::a_probing_application_is_told_that_synchronized_output_works` drives
the whole loop — the app asks, believes the answer, brackets its repaints,
and `wait_frame` returns a complete frame. That is the difference between a
harness you write subjects for and one you point at real programs.

0.4 also corrected the *mouse* mode answers: with nothing tracking they now
report "reset" instead of "not recognized", which closed a loop where
probe-then-enable applications were told the mouse did not exist and then
blamed by `click` for not enabling it.

### 7.3 The style model, and the trap it closed

All three rows at the top of §5.3 have moved. Against the same unmodified
taskboard:

| §5.3 said the harness sees | it now sees |
|---|---|
| strikethrough: *only the dim* | `dim strikethrough` — two attributes, not one |
| blink: *a red cell, no blink* | `fg=1 blink` — `find_by(\|c\| c.style().blink)` finds the badge |
| conceal: **the secret, in clear**, unmarked | the secret, in clear, **marked concealed** |

The styled snapshot diff is the whole story in two lines:

```
-4: … 5-9 fg=1 bold reverse; 10-31 dim reverse;            …
+4: … 5-9 fg=1 bold reverse; 10-31 dim reverse strikethrough; …
-7: … 5-9 fg=1 bold; 10-11 fg=1;      …
+7: … 5-9 fg=1 bold; 10-11 fg=1 blink; …
```

The conceal row is the one §5.3 said to pause on, and it is worth pausing on
again now that it is closed. A test asserting "the credentials field is
masked" used to pass against an application printing the secret in the clear,
because the two renderings were the same value. `a2` now asserts identical
`text()` and different `conceal` flags — the assertion that was impossible to
write.

Note the text is *still* in the grid, and that is correct: a real terminal
holds it too. What changed is that the harness can tell you it was hidden.

### 7.4 `wait_frame` stopped being able to lie

§6's item 2 asked for "a barrier and a return value". It shipped as a
**consumption cursor** instead, for a reason worth recording: a strict
barrier ("only frames completed after the call") contradicts the frame-history
feature in the same breath, because in a burst the intermediate frames were
all drawn *before* the call that wants them. A per-terminal cursor — each
call scans only frames newer than the one it last returned — gives both.

Three separate ways a test could pass while proving nothing, all closed:

- `e2` — `send(key)` then `wait_frame(old_state)` passed vacuously. Now it
  times out.
- `e1` — a `wait_frame` after `resize` matched the frame drawn at the old
  size. Resize now advances the cursor.
- `e3` — the matched frame was unreachable, so the assertion after the wait
  read a later, possibly torn, instant. `wait_frame` returns the frame.

And against the real event loop, §5.2's progress burst is now assertable step
by step: `a_progress_burst_is_observable_up_to_the_retention_bound` walks 40%
through 100% in the order the gauge drew them, and pins the four frames that
fell off the 8-frame bound.

### 7.5 Scrollback, clipboard, deadlines

- **Scrollback** is retained (1000 rows by default). taskboard is unaffected
  — an alternate-screen application accumulates no history, exactly as on a
  real terminal — which is both why the rest of the suite needed no changes
  and why the feature is aimed at the other kind of program.
- **`OSC 52`** payloads are captured: text *and* target selection, with an
  undecodable payload reported as such rather than as an empty clipboard.
  `hard::the_clipboard_payload_is_observable` asserts the exact title, and
  that it tracks the selection rather than being a constant that happens to
  match.
- **Every wait takes a per-call deadline** now, `wait_exit_for` included.

### 7.6 One fix nobody wrote down

`survey::f7` pinned that ten writes to an exited child were silently
discarded, while the docs promised a panic with the screen attached. As of
0.3 the write path is acknowledged, so the `EIO` reaches the caller and the
documented contract is finally the behaviour. It is not in the crate's
changelog; it fell out of the write-deadline work.

### 7.7 The one migration cost

`hard::clicking_past_column_222_works_under_sgr` broke, and not because a gap
closed: it called `wait_frame(NORMAL)` after `common::spawn_sized` had already
consumed that frame. Under the cursor, a frame satisfies exactly one wait, so
the second call waited for a repaint that never came and burned the full
timeout.

That is the whole upgrade cost, and it is the good kind: a **loud** failure
(a ten-second hang, not a silent pass) in a test that was redundant anyway.
The companion discipline is new and does need learning — with eight frames
retained and scanned oldest first, a predicate true of two of them resolves
on the *older*. Pinned by
`limits::a_predicate_true_of_two_retained_frames_matches_the_older`.

## 8. What is still a limitation

`limits.rs` is thirteen tests now, up from nine, because four absences closed
and six bounds took their place — plus the two ranked items that are still
open.

- **The frame history is eight deep**, and a predicate true of two retained
  frames resolves on the older.
- **`screen()` can be torn** even for a correctly-synchronized application.
  Documented in 0.4 rather than changed, and the reasoning holds: serving the
  newest complete frame would let a `wait_until` predicate match content the
  following `screen()` does not show. `wait_idle` refuses to call an open
  frame idle, and now says so in the timeout.
- **Scrollback is bounded, unreflowed, and text only** — three separate
  bounds, so a style regression above the fold is not assertable.
- **`wait_frame` still needs the application to opt in.**
- **Still unanswerable**: kitty `CSI ? u`, DA3, OSC 12, `XTGETTCAP`, and
  `OSC 52` *reads*, so copy-then-read-back still hangs.
- **Still open from §6**: the 222 guard fires before the encoding match, so
  mode 1005 cannot reach the coordinates it exists for (`v8`); and query
  replies are dropped rather than backpressured past the queue depth (§4.2).
- **Still outside the model**: `OSC 8` targets, `BEL`, `DECSCUSR`, `OSC 4`
  redefinition, focus events (mode 1004), NFD/NFC needle matching, trailing
  whitespace inside a padded pane. Unix only.

## 9. Ranking, revised again

1. **Backpressure rather than drop** (§4.2) — the oldest unaddressed item,
   and the only remaining one where the harness silently loses data an
   application asked for. A discarded answer should not be discoverable only
   through a note in an unrelated timeout.
2. **Move the 222 guard inside the encoding match** (§4.1) — still the
   smallest fix on this list, and still wrong in the direction of refusing
   input the selected encoding can carry.
3. **`send`/`paste`/`click` returning `Result`** — now the last place the
   crate panics where it could report. 0.3 made the panic honest and bounded;
   making it a `Result` would make it composable.
4. **Focus events** (mode 1004) — the largest untouched category: no API
   delivers one, so a whole branch of any focus-aware TUI is unreachable.
5. **Styles in scrollback** (§8) — narrow, but it is exactly the class of
   application scrollback was added for: a log view that colours by severity
   cannot be asserted on above the fold.

Nothing on this list can produce a test that passes while proving nothing.
After §7.1, that is the property worth naming.
