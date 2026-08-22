//! Probe suite for the inline-graphics surface termlens **0.6** added, and
//! for the two things about it that a consumer gets wrong first: that a
//! transmission is counted as an *image* rather than as an escape, and that
//! a picture's placement is a fact about the grid rather than about the
//! payload.
//!
//! Everything here is reproduced against a real process. The images are
//! emitted by `printf` rather than by taskboard, deliberately: the point is
//! what termlens reports for a known payload, and a hand-written escape is
//! the only way to know the payload exactly.

mod common;

use std::time::Duration;

use common::spawn_sh;
use termlens::{GraphicsAction, GraphicsFormat, GraphicsProtocol, Screen, Terminal};

/// A 2x2 image: three pixels of GitHub Primer's brightest contribution
/// green and one transparent, in that order.
const GREEN: [u8; 4] = [0x39, 0xd3, 0x53, 0xff];
const RGBA_B64: &str = "OdNT/znTU/8501P/AAAAAA==";
const ZLIB_B64: &str = "eJyzvBz83xKKGRgYGABH5gcb";

/// Emit `escapes`, then a marker the test can wait for. The marker is
/// printed last, so a screen carrying it carries the payload too.
fn emit(escapes: &str) -> Terminal {
    spawn_sh(
        &format!("printf '{escapes}'; printf DONE"),
        Duration::from_secs(5),
    )
}

fn settled(t: &mut Terminal) -> termlens::Result<Screen> {
    t.wait_until(|s| s.contains("DONE"))?;
    Ok(t.screen())
}

/// The baseline claim: one kitty transmission is one image, described by
/// what its control block declared.
#[test]
fn g1_a_kitty_transmission_is_one_described_image() -> termlens::Result<()> {
    let mut t = emit(&format!("\\033_Ga=T,f=32,s=2,v=2,i=7;{RGBA_B64}\\033\\\\"));
    let screen = settled(&mut t)?;
    let seen = screen.graphics();

    assert_eq!(seen.kitty(), 1, "{seen:?}");
    assert_eq!(seen.sixel(), 0);
    assert_eq!(seen.deletes(), 0);
    assert_eq!(seen.total(), 1);
    assert!(!seen.is_empty());

    let image = seen.last().expect("a payload");
    assert_eq!(image.protocol(), GraphicsProtocol::Kitty);
    assert_eq!(image.action(), GraphicsAction::TransmitAndPlace);
    assert_eq!(image.format(), GraphicsFormat::Rgba);
    assert_eq!(image.id(), Some(7));
    assert_eq!(image.size(), Some((2, 2)));
    assert!(!image.compressed(), "this one is plain");
    assert!(image.bytes() > 0);
    assert_eq!(image.chunks(), 1);
    Ok(())
}

/// The chunking claim, and the one that made the pre-0.6 count wrong: the
/// protocol caps a payload at 4096 bytes and continues with `m=1`, so any
/// image of consequence arrives in several escapes. It is still one image,
/// and the joined data is the same data.
#[test]
fn g2_a_chunked_transmission_is_one_image() -> termlens::Result<()> {
    let (head, tail) = RGBA_B64.split_at(12);
    let mut t = emit(&format!(
        "\\033_Ga=T,f=32,s=2,v=2,i=7,m=1;{head}\\033\\\\\\033_Gm=0;{tail}\\033\\\\"
    ));
    let chunked = settled(&mut t)?;

    let mut t = emit(&format!("\\033_Ga=T,f=32,s=2,v=2,i=7;{RGBA_B64}\\033\\\\"));
    let whole = settled(&mut t)?;

    let (a, b) = (chunked.graphics(), whole.graphics());
    assert_eq!(a.kitty(), 1, "two escapes, one image: {a:?}");
    assert_eq!(b.kitty(), 1);

    let split = a.last().expect("a payload");
    assert!(split.chunks() > 1, "the fixture split it: {split:?}");
    assert_eq!(
        split.data(),
        b.last().expect("a payload").data(),
        "same image, however it was cut up on the way"
    );
    Ok(())
}

/// A delete is traffic, not a picture. Folding it into the image count made
/// an application that tears down what it drew look like one that drew
/// twice as much.
#[test]
fn g3_a_delete_is_counted_apart_from_a_transmission() -> termlens::Result<()> {
    let mut t = emit(&format!(
        "\\033_Ga=T,f=32,s=2,v=2,i=7;{RGBA_B64}\\033\\\\\\033_Ga=d,i=7;\\033\\\\"
    ));
    let screen = settled(&mut t)?;
    let seen = screen.graphics();

    assert_eq!(seen.kitty(), 1, "one image: {seen:?}");
    assert_eq!(seen.deletes(), 1, "and one teardown: {seen:?}");
    assert_eq!(seen.total(), 1);

    let payloads = seen.payloads();
    assert_eq!(payloads.len(), 2, "both are on the wire: {payloads:?}");
    assert_eq!(payloads[1].action(), GraphicsAction::Delete);
    assert!(!payloads[1].action().carries_image());
    assert!(seen.bytes() > 0, "a delete is still counted as traffic");
    Ok(())
}

/// Placement: the one fact that lives in the grid rather than in the
/// payload, and the one an application gets wrong when a picture slides out
/// from under its own labels — a failure no cell shows.
#[test]
fn g4_an_image_is_stamped_with_the_cell_it_landed_on() -> termlens::Result<()> {
    // Row 3, column 6 (1-based on the wire) => (2, 5) 0-based.
    let mut t = emit(&format!(
        "\\033[3;6H\\033_Ga=T,f=32,s=2,v=2,i=7;{RGBA_B64}\\033\\\\"
    ));
    let screen = settled(&mut t)?;
    let image = screen.graphics().last().expect("a payload").at();
    assert_eq!(image, (2, 5), "landed at {image:?}");
    Ok(())
}

/// Nothing about a payload touches the grid. That is exactly why the
/// counters exist, and it is worth one assertion that the emulator has not
/// quietly started rendering pictures into cells.
#[test]
fn g5_the_payload_leaves_the_grid_alone() -> termlens::Result<()> {
    let mut t = emit(&format!(
        "above\\033[2;1H\\033_Ga=T,f=32,s=2,v=2,i=7;{RGBA_B64}\\033\\\\\\033[3;1H"
    ));
    let screen = settled(&mut t)?;
    assert!(screen.contains("above"), "{screen}");
    assert_eq!(
        screen.row_text(1).trim(),
        "",
        "the payload's row:\n{screen}"
    );
    Ok(())
}

/// The negative assertion, which is as often as not the one a suite wants:
/// "this must render as text everywhere, and never go out as an image".
#[test]
fn g6_a_program_that_draws_no_image_reports_none() -> termlens::Result<()> {
    let mut t = emit("just text");
    let screen = settled(&mut t)?;
    let seen = screen.graphics();

    assert!(seen.is_empty(), "{seen:?}");
    assert_eq!(seen.bytes(), 0);
    assert!(seen.payloads().is_empty());
    assert!(seen.last().is_none());
    Ok(())
}

/// The retention budget is a memory bound, not an observation bound: every
/// count and every declared fact survives it, and only the bytes go.
#[test]
fn g7_a_bounded_capture_keeps_the_counts_and_drops_the_bytes() -> termlens::Result<()> {
    let mut t = Terminal::builder()
        .size(40, 6)
        .env_clear()
        .timeout(Duration::from_secs(5))
        .capture_graphics(0)
        .args([
            "-c",
            &format!("printf '\\033_Ga=T,f=32,s=2,v=2,i=7;{RGBA_B64}\\033\\\\'; printf DONE"),
        ])
        .spawn("/bin/sh")?;
    let screen = settled(&mut t)?;
    let seen = screen.graphics();

    assert_eq!(seen.kitty(), 1, "still counted: {seen:?}");
    let image = seen.last().expect("a payload");
    assert_eq!(image.size(), Some((2, 2)), "still described");
    assert!(image.bytes() > 0, "still measured");
    assert_eq!(image.data(), None, "and not kept");
    Ok(())
}

/// Sixel arrives by a different route and is counted apart from kitty.
#[test]
fn g8_a_sixel_is_recognised_and_counted_apart() -> termlens::Result<()> {
    let mut t = emit("\\033Pq\"1;1;2;2#0;2;0;100;0#0!2~-!2~\\033\\\\");
    let screen = settled(&mut t)?;
    let seen = screen.graphics();

    assert_eq!(seen.sixel(), 1, "{seen:?}");
    assert_eq!(seen.kitty(), 0);
    let image = seen.last().expect("a payload");
    assert_eq!(image.protocol(), GraphicsProtocol::Sixel);
    assert_eq!(image.format(), GraphicsFormat::Sixel);
    Ok(())
}

/// What the pictures actually were, which is the claim the `decode` feature
/// exists for: not "an image of about the right size went out", but *this*
/// image went out. Compressed and plain decode to the same pixels.
#[test]
fn g9_the_pixels_are_the_pixels_that_were_drawn() -> termlens::Result<()> {
    for (label, payload, compressed) in [
        ("plain", format!("f=32,s=2,v=2,i=7;{RGBA_B64}"), false),
        ("zlib", format!("f=32,s=2,v=2,i=7,o=z;{ZLIB_B64}"), true),
    ] {
        let mut t = emit(&format!("\\033_Ga=T,{payload}\\033\\\\"));
        let screen = settled(&mut t)?;
        let image = screen.graphics().last().expect("a payload").clone();
        assert_eq!(image.compressed(), compressed, "{label}");

        let bitmap = image
            .decode()
            .unwrap_or_else(|error| panic!("{label}: {error}"));
        assert_eq!((bitmap.width(), bitmap.height()), (2, 2), "{label}");
        assert_eq!(bitmap.pixel(0, 0), Some(GREEN), "{label}");
        assert_eq!(bitmap.pixel(1, 0), Some(GREEN), "{label}");
        assert_eq!(bitmap.pixel(0, 1), Some(GREEN), "{label}");
        assert_eq!(
            bitmap.pixel(1, 1),
            Some([0, 0, 0, 0]),
            "{label}: kitty has the alpha channel to say 'transparent'"
        );
        assert_eq!(bitmap.pixel(2, 0), None, "{label}: out of bounds is None");
        assert_eq!(bitmap.colours()[0], (GREEN, 3), "{label}");
    }
    Ok(())
}

/// A refusal names its reason rather than decoding a prefix of itself into
/// a plausible wrong picture.
#[test]
fn g10_a_refusal_to_decode_says_why() -> termlens::Result<()> {
    use termlens::DecodeError;

    let mut t = emit(&format!(
        "\\033_Ga=T,f=32,s=2,v=2,i=7;{RGBA_B64}\\033\\\\\\033_Ga=d,i=7;\\033\\\\"
    ));
    let screen = settled(&mut t)?;
    let seen = screen.graphics();
    assert_eq!(
        seen.payloads()[1].decode().unwrap_err(),
        DecodeError::NoImage(GraphicsAction::Delete),
        "a delete carries no image"
    );
    Ok(())
}
