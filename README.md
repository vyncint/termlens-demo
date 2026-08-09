# termlens-demo

A minimal example of testing a CLI with [termlens](https://crates.io/crates/termlens):
spawn the program in a real PTY, drive it with keystrokes, and assert on the
rendered screen instead of scraping raw output.

- `src/main.rs` — a toy app: prints `Ready`, echoes each line back as
  `got: <line>`, and exits when it reads `q`.
- `tests/demo.rs` — drives the app with termlens and asserts on what's
  actually shown on screen.

```sh
cargo test
```
