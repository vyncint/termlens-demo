use std::time::Duration;

use termlens::{Key, Terminal};

#[test]
fn echoes_input_and_quits_on_q() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(80, 24)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .spawn(env!("CARGO_BIN_EXE_myapp"))?;

    t.wait_until(|screen| screen.contains("Ready"))?;

    t.send_str("hello");
    t.send(Key::Enter);
    t.wait_until(|screen| screen.contains("got: hello"))?;

    t.send_str("q");
    t.send(Key::Enter);
    assert!(t.wait_exit()?.success());

    Ok(())
}
