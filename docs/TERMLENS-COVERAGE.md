# termlens 0.4 — what improved, what's left

Third run of the study against the same subject: `taskboard`, a TUI with
tabs, a filtered list, a detail pane, a modal dialog, a text input with a
live cursor, a help overlay, styled cells, CJK/emoji glyphs, a responsive
layout, mouse support, bracketed paste, DEC 2026 synchronized repaints, a
SIGTERM shutdown path — and, added for this run, struck-through done titles,
a blinking priority badge, and a `y` key that copies the selected title to
the system clipboard with `OSC 52`.

**62 tests, all passing, 0 failures in 20 stress runs.** `tests/tui.rs` (49)
is the coverage; `tests/limits.rs` (13) pins what remains. As before,
nothing here is inferred from reading the crate — every claim was
reproduced.

The demo jumped 0.2.0 → 0.4.0, so this covers three releases at once.

Headline: **all three items this study asked for in its 0.2 ranking
shipped**, in the order it ranked them. So did the two gaps it had listed as
merely "outside the model".

---

## 0. A methodological finding first

The 0.2 study said this about its own pinning tests:

> Every test here passes and pins *current* behaviour, so closing one of
> these gaps will fail the test that encodes its workaround.

**That did not happen.** Bumping the dependency from 0.2.0 to 0.4.0 and
changing nothing else left all 49 tests passing — including **four pins
whose stated claims had become false**:

| pin | why it still passed | claim status |
|---|---|---|
| `only_the_newest_frame_of_a_burst_survives` | the frame it looked for had been *consumed* by the earlier wait, not never retained | false |
| `right_click_and_drag_still_need_hand_rolled_bytes` | hand-rolled bytes still work; nothing removed them | false |
| `output_scrolled_off_the_top_is_unrecoverable` | it only asserted on the *visible* grid, which is still true | false |
| `only_wait_until_takes_a_per_call_timeout` | it demonstrated the short deadline via `wait_idle`, which still honours it | false |

A green pinning test that asserts something false is worse than no pin,
because it reads as evidence. The lesson generalises past this repo: **a pin
must assert the mechanism, not a symptom the mechanism happens to share with
its own replacement.** The rewritten `tests/limits.rs` counts frames, counts
scrollback rows, and asserts on the API's existence rather than on a
workaround still functioning.

---

## 1. What 0.3 and 0.4 fixed

| 0.2 finding | now |
|---|---|
| **`wait_frame` retains one frame** (§2.1, ranked #3) | 8 frames, observable in emission order, each call returning the frame it matched |
| **The mouse API is one button** (§2.4, ranked #2) | `click_with`, `drag`, modifier chords, horizontal wheel |
| **Per-call timeouts only on `wait_until`** (§2.8, ranked #1) | `wait_frame_for`, `wait_idle_for`, `wait_exit_for` |
| `DECRQM` unanswerable (§2.5) | answered — probe-then-enable applications work unmodified |
| No scrollback (§2.6) | retained, 1000 rows by default |
| `OSC 52` clipboard writes outside the model (§2.9) | `Screen::clipboard()` — payload *and* target selection |
| Strikethrough / blink / conceal absent from `Style` | all three in `Style` and in `with_styles()` |

Three defects also surfaced in 0.4 that this study had not found, because
they needed adversarial probing of the release rather than use of it: a stray
`?2026l` published a phantom frame and *suppressed* the "never emitted a
synchronized update" diagnosis; `wait_frame` matched frames the application
had already moved past; and `screen()` could hand back a torn grid even for a
correctly-synchronized application.

### 1.1 The style model is complete enough to catch a masked field

This is the change with the sharpest edge, and it is not about
strikethrough. Before 0.4, `SGR 8` (conceal) reached nothing, so a test
asserting that a password field is masked **passed against an application
that printed the secret in the clear** — the two renderings are identical
text, and `with_styles()` could not break the tie either. That is the one
failure mode where a green test certifies the bug it was written to catch.
`a_masked_field_is_distinguishable_from_clear_text` now asserts identical
`text()` and *different* conceal flags.

For strikethrough and blink the effect is visible as a snapshot diff.
taskboard was changed for this run to strike through done titles and blink
the HIGH badge — the same rendering a real terminal shows — and the styled
snapshot went from

```
4: 1-4 fg=2 bold reverse; 5-9 fg=1 bold reverse; 10-31 dim reverse; …
```

to

```
4: 1-4 fg=2 bold reverse; 5-9 fg=1 bold blink reverse; 10-31 dim reverse strikethrough; …
```

Under 0.2 those two extra attributes were simply absent from the model, so a
regression that dropped either was invisible. Now `find_by(|c|
c.style().blink)` locates the badge, and toggling a task with `space` moves
the strikethrough with it.

### 1.2 Clipboard payloads are assertable

taskboard's `y` copies the selected title and paints a "copied" toast. The
toast proves the code path ran; it says nothing about *what* was copied, and
the base64 never reaches the grid. That was unanswerable in 0.2.

```rust
t.send(Key::Char('y'));
let frame = t.wait_frame(|s| s.contains("copied"))?;
let clip = frame.clipboard().expect("captured");
assert_eq!(clip.text(),    Some("Wire up the PTY reader"));
assert_eq!(clip.targets(), "c");   // clipboard, not primary
```

Two details make it trustworthy rather than merely present. The target
selection is reported as the application named it, so writing to `p` when it
meant `c` is catchable. And an undecodable payload reports `None` rather than
`Some("")`, so a test asserting an empty clipboard cannot pass on something
the harness failed to read.

### 1.3 Frames are a history, and the order is enforced

The 0.2 study asked for "even two or three retained frames". It got eight,
plus something it had not asked for: each call scans only frames *newer than
the one it last returned*. So three keypresses whose repaints arrive
coalesced are observable one at a time —

```rust
t.send(Key::Down); t.send(Key::Down); t.send(Key::Down);
t.wait_until(|s| s.contains("Tasks 4/10"))?;          // settle, consuming nothing
for expected in ["Tasks 2/10", "Tasks 3/10", "Tasks 4/10"] {
    let frame = t.wait_frame(|s| s.contains(expected))?;
    assert!(frame.contains(expected));
}
```

— and asking backwards now *fails*, which is what makes the sequence a
guarantee rather than a coincidence. The same cursor closes a hazard the 0.2
study never noticed: `send(key)` followed by `wait_frame(old_state)` used to
pass on the retained frame while the assertion after it read the old screen.

`wait_frame` also returns the frame it matched, which removes the last reason
to reach for `screen()` after a frame wait. The 0.2 suite smuggled matched
screens out of predicates with a captured `Option`; that idiom is gone.

### 1.4 `DECRQM`, and a correction to this study

**§2.5 of the 0.2 study was wrong.** It listed `DECRQM` among queries that
"the timeout now names", and it did not: it was neither answered nor named,
so an application probing it hung silently with no diagnosis. Filed as
issue #1 against this repo rather than quietly edited.

It is now moot in the best way — `DECRQM` is *answered*, and that is the
single highest-leverage query in the set, because it is how an application
asks "do you support synchronized output?" and "is mouse tracking on?".
Answering it means a probe-then-enable application enables features against
termlens without being modified for the harness — which is the difference
between a harness you write subjects for and one you point at real programs.

0.4 also corrected the answer for the mouse tracking modes. With nothing
tracking, they now report *reset* rather than "not recognized", which closed
a loop that blamed the application for a decision the harness caused.

### 1.5 Scrollback

`TerminalBuilder::scrollback(rows)`, defaulting to 1000, plus
`scrollback_rows()`, `scrollback_text()` and `full_text()` — history followed
by the visible screen, which is the assertion an author actually writes.

Worth stating because it surprised us: taskboard's own tests are unaffected,
because an application on the **alternate screen accumulates no history at
all**, exactly as on a real terminal. So the feature costs a full-screen TUI
nothing, and it is aimed squarely at the class of program that hands finished
output back to the terminal — a pager, a log view, a TUI that commits
completed blocks into native scrollback.

### 1.6 Every wait takes a deadline

The 0.2 study's #1 ask, and the smallest change of the three:

```rust
let frame = t.wait_frame_for(|s| s.contains("late frame"), Duration::from_secs(5))?;
t.wait_idle_for(Duration::from_millis(50), Duration::from_secs(5))?;
let status = t.wait_exit_for(Duration::from_secs(5))?;
```

`wait_exit_for` was not asked for; "every wait" is not true without it.

---

## 2. What is still a limitation

Eleven, up from nine — because four of the 0.2 items closed and six new
**bounds and disciplines** took their place. That is the healthier trade: a
missing feature blocks a test, while a bound is something a test author can
work with once it is written down.

### 2.1 The frame history is eight frames deep
A longer burst drops its oldest. Pinned by
`a_burst_longer_than_eight_frames_drops_its_oldest`, which counts frames
rather than asserting that any particular one is unreachable.

### 2.2 An ambiguous frame predicate resolves on the *older* frame
New, and the one that cost us a test while writing this run. Each call scans
retained frames oldest first, so a predicate true of more than one of them
returns the earlier — which may not be the state you meant. The remedy is
the discipline `wait_until` already demands: name something only the frame
you want can show. (`a_predicate_true_of_two_retained_frames_matches_the_older`)

### 2.3 `wait_frame` still requires the application to opt in
Unchanged. It works only for apps that bracket repaints in DEC 2026; for a
plain CLI it can never succeed, and the error says so. taskboard had to be
modified to emit synchronized updates before any of this suite could use
`wait_frame` — the headline feature is still conditional on the subject's
cooperation.

### 2.4 A snapshot can be torn even for a synchronized application
`screen()` returns the live grid, which can be half-painted. Documented in
0.4 rather than changed, and the reasoning holds up: substituting the newest
complete frame would let a `wait_until` predicate match content the following
`screen()` does not show, and a torn read is what diagnoses an application
hung mid-repaint. `wait_idle` refuses to call an open frame idle — and now
says so in the timeout — which is what makes the "settle before a
whole-screen snapshot" recipe work.
(`a_snapshot_can_be_torn_even_for_a_synchronized_app`)

### 2.5 `Esc` followed by a key is still read as Alt
Byte-identical to an Alt chord, and there is still no `send_after(delay)`.
Documented on `Key::Esc` with the wait-for-the-effect remedy, which only
works when the `Esc` *has* an observable effect.

### 2.6 Some queries remain unanswerable
The kitty keyboard probe (`CSI ? u`), DA3, OSC 12 (cursor colour),
`XTGETTCAP`, and `OSC 52` clipboard **reads** — so an application that
copies and then reads back to verify still hangs. `DECRQM` has left this
list. The timeout names whatever is left, which turns a strace-level mystery
into a one-line diagnosis.

### 2.7 Scrollback is bounded, unreflowed, and text only
Three separate bounds, each pinned. Past the configured length the oldest
rows are dropped; a resize does not rewrap history; and a scrolled-off row
has no `Style` and no cell addressing, so a style regression above the fold
is not assertable.

### 2.8 stdout and stderr are one stream
Inherent to PTYs, not a defect — but "assert this went to stderr" remains
unanswerable.

### 2.9 Still outside the model
Cursor shape and blink (`DECSCUSR`), OSC 8 hyperlink targets, sixel/kitty
graphics. Unix only. `send` still panics rather than returning `Result` when
the child is gone — though as of 0.3 it panics at a deadline instead of
hanging, with the screen attached.

---

## Verdict

0.2 was the release that made the suite stop being flaky. 0.3 and 0.4 are
the releases that made it stop *lying* — which is a harder and more valuable
property, and one this study is now on the wrong side of once: four of its
own pins went green against a version that had invalidated them.

Every item in the 0.2 ranking shipped, and the two features listed as
"outside the model" (clipboard writes, the missing style attributes) shipped
too. What replaced them is a list of bounds rather than a list of absences,
which is what a maturing harness should look like.

If you want a ranking of what to do next, from this study:

1. **`send`/`paste`/`click` returning `Result`** (§2.9) — the last place the
   crate panics where it could report. A test that types into a dead child
   gets a panic where every other failure gets a screen-carrying error.
2. **`send_after(delay)`** (§2.5) — the `Esc`/Alt ambiguity is the oldest
   unfixed item in this study, now three releases old, and the only one whose
   workaround requires the application to cooperate.
3. **Styles in scrollback** (§2.7) — the narrowest gap, and the one most
   likely to be hit by the class of application scrollback was added for: a
   log view that colours by severity cannot be asserted on above the fold.

Nothing in the remaining list threatens correctness or produces flaky tests.
That was true of 0.2 as well; what is new is that nothing in it can produce a
test that passes while proving nothing.
