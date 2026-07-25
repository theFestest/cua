//! End-to-end check of the Apple Vision path against ground truth.
//!
//! `#[ignore]`d because it shells out to `qlmanage` to render the fixture. It
//! needs no TCC permission and posts no input, but Quick Look thumbnailing is
//! host state, so it stays opt-in like the rest of the driver's e2e suite:
//!
//! ```sh
//! cargo test -p cua-driver-ocr -- --ignored
//! ```

#![cfg(target_os = "macos")]

use std::path::PathBuf;
use std::process::Command;

/// Render a text file to a PNG with Quick Look and return (png_bytes, w, h).
fn render_fixture(body: &str, tag: &str) -> (Vec<u8>, u32, u32) {
    let dir = std::env::temp_dir().join(format!("cua-ocr-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create fixture dir");
    let txt = dir.join("fixture.txt");
    std::fs::write(&txt, body).expect("write fixture text");

    let status = Command::new("qlmanage")
        .args(["-t", "-s", "1200", "-o"])
        .arg(&dir)
        .arg(&txt)
        .output()
        .expect("qlmanage must be present on macOS");
    assert!(
        status.status.success(),
        "qlmanage failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );

    let png: PathBuf = dir.join("fixture.txt.png");
    let bytes = std::fs::read(&png)
        .unwrap_or_else(|e| panic!("Quick Look produced no thumbnail at {}: {e}", png.display()));
    // PNG IHDR: width and height are big-endian u32 at byte offsets 16 and 20.
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    (bytes, w, h)
}

fn find<'a>(lines: &'a [cua_driver_ocr::OcrLine], needle: &str) -> &'a cua_driver_ocr::OcrLine {
    lines
        .iter()
        .find(|l| l.text.contains(needle))
        .unwrap_or_else(|| {
            panic!(
                "Vision did not recognize {needle:?}; it read: {:?}",
                lines.iter().map(|l| &l.text).collect::<Vec<_>>()
            )
        })
}

#[test]
#[ignore]
fn recognizes_text_and_orders_centers_top_to_bottom() {
    // Two markers separated by enough blank lines that their vertical order is
    // unambiguous. This is the ground truth for the bottom-left → top-left
    // Y-flip: a flipped conversion puts TOPLINE below BOTTOMLINE.
    let body = format!("TOPLINE ALPHA\n{}BOTTOMLINE OMEGA\n", "\n".repeat(24));
    let (png, w, h) = render_fixture(&body, "order");

    let lines = cua_driver_ocr::recognize(&png, w, h, &["en-US"]).expect("recognize");
    let top = find(&lines, "TOPLINE");
    let bottom = find(&lines, "BOTTOMLINE");

    assert!(
        top.center.1 < bottom.center.1,
        "Y axis is flipped: TOPLINE at y={:.1} should be above BOTTOMLINE at y={:.1}",
        top.center.1,
        bottom.center.1
    );
    assert!(
        top.center.1 < h as f64 / 2.0,
        "TOPLINE should sit in the upper half of a {h}px image, got y={:.1}",
        top.center.1
    );
    for line in [top, bottom] {
        assert!(
            line.center.0 >= 0.0
                && line.center.0 <= w as f64
                && line.center.1 >= 0.0
                && line.center.1 <= h as f64,
            "center {:?} escapes the {w}×{h} image",
            line.center
        );
        assert!(
            line.confidence > 0.0 && line.confidence <= 1.0,
            "confidence {} outside [0,1]",
            line.confidence
        );
    }
}

#[test]
#[ignore]
fn recognizes_left_to_right_order_on_one_line() {
    // X is not flipped. Pinning it alongside Y keeps a future "fix" to the
    // flip from silently mirroring the other axis.
    let (png, w, h) = render_fixture("LEFTMARK                    RIGHTMARK\n", "lr");

    let lines = cua_driver_ocr::recognize(&png, w, h, &["en-US"]).expect("recognize");
    let left = find(&lines, "LEFTMARK");
    let right = find(&lines, "RIGHTMARK");
    assert!(
        left.center.0 < right.center.0,
        "X axis is mirrored: LEFTMARK at x={:.1} should precede RIGHTMARK at x={:.1}",
        left.center.0,
        right.center.0
    );
}

#[test]
#[ignore]
fn empty_hints_detect_non_latin_script() {
    // With no hints, Vision's own default is English-only: it returns success
    // and silently omits every other script, which reads to a caller as "no
    // text here" rather than as a failure. `recognize` therefore turns on
    // automatic language detection when the caller passes no hints.
    let (png, w, h) = render_fixture("設定を開く\nHello World\n한국어 테스트\n", "cjk");

    let lines = cua_driver_ocr::recognize(&png, w, h, &[]).expect("recognize");
    find(&lines, "設定を開く");
    find(&lines, "한국어");
    find(&lines, "Hello World");
}

#[test]
#[ignore]
fn explicit_hints_restrict_recognition() {
    // The flip side, pinned so the trade-off stays visible: an explicit hint
    // list is a restriction, not a preference. Latin text still reads, the
    // unhinted script does not.
    let (png, w, h) = render_fixture("設定を開く\nHello World\n", "restrict");

    let lines = cua_driver_ocr::recognize(&png, w, h, &["en-US"]).expect("recognize");
    find(&lines, "Hello World");
    assert!(
        !lines.iter().any(|l| l.text.contains("設定")),
        "en-US-only recognition unexpectedly returned Japanese: {:?}",
        lines.iter().map(|l| &l.text).collect::<Vec<_>>()
    );
}
