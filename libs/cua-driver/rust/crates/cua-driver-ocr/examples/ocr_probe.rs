//! Run OCR over an image file and print each line with its center.
//!
//! ```sh
//! cargo run -p cua-driver-ocr --example ocr_probe -- <image.png> [lang ...]
//! ```
//!
//! Passing no languages exercises the automatic-detection path. Passing hints
//! restricts recognition to them, which is the difference the language tests in
//! `tests/e2e_vision.rs` pin down.

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: ocr_probe <image> [lang ...]");
    let langs: Vec<String> = args.collect();
    let refs: Vec<&str> = langs.iter().map(String::as_str).collect();

    let bytes = std::fs::read(&path).expect("read image");
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());

    match cua_driver_ocr::recognize(&bytes, w, h, &refs) {
        Ok(lines) => {
            println!("hints={refs:?} -> {} line(s)", lines.len());
            for l in &lines {
                println!(
                    "  ({:>5.0},{:>5.0}) conf={:.2} {:?}",
                    l.center.0, l.center.1, l.confidence, l.text
                );
            }
        }
        Err(e) => println!("hints={refs:?} -> ERROR: {e}"),
    }
}
