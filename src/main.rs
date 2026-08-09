//! A tiny toy CLI: prints "Ready", echoes each line it's given, and
//! exits when it reads "q". Just enough behavior for the termlens test
//! in `tests/demo.rs` to drive and assert against.

use std::io::{self, BufRead, Write};

fn main() {
    println!("Ready");
    io::stdout().flush().unwrap();

    for line in io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        if line == "q" {
            break;
        }
        println!("got: {line}");
        io::stdout().flush().unwrap();
    }
}
