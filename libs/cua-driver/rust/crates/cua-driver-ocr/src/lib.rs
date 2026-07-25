//! OCR via Apple's Vision framework (`VNRecognizeTextRequest`).
//!
//! Runs on an encoded image (the same PNG/JPEG bytes a window capture produces)
//! and returns recognized text lines with their centers in the image's pixel
//! space, so recognized text is clickable through the very same coordinate
//! frame as the screenshot the agent is looking at.
//!
//! On-device: no model files to download and no runtime to install. Recognizes
//! Latin and CJK scripts among others.
//!
//! The Vision FFI in this crate is ported from Nova (MIT, © bigduu,
//! <https://github.com/bigduu/Nova>), `src/platform/mac/ocr.rs`.
//!
//! The public API is bytes-in / plain-structs-out on purpose: it keeps every
//! objc2 type inside this crate, so this crate's objc2 0.6 requirement and
//! `platform-macos`'s objc2 0.5 pin coexist without meeting.

/// One recognized line of text.
#[derive(Debug, Clone, PartialEq)]
pub struct OcrLine {
    /// The recognized text (Vision's top candidate for this line).
    pub text: String,
    /// Recognition confidence in `[0, 1]`.
    pub confidence: f32,
    /// Center of the text's bounding box, in the SOURCE IMAGE's pixel space
    /// (origin top-left) — directly usable as a click coordinate against that
    /// same image.
    pub center: (f64, f64),
}

/// Convert a Vision bounding box to its center in image pixels.
///
/// Vision reports normalized `[0, 1]` boxes with a **bottom-left** origin;
/// callers click in a **top-left** origin pixel frame, so the Y axis flips.
///
/// Only the macOS Vision path calls this, but it is plain arithmetic and its
/// tests are worth running on every target, so it stays compiled everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn box_center_to_pixels(
    origin_x: f64,
    origin_y: f64,
    width: f64,
    height: f64,
    img_w: u32,
    img_h: u32,
) -> (f64, f64) {
    let cx = (origin_x + width / 2.0) * img_w as f64;
    let cy = (1.0 - (origin_y + height / 2.0)) * img_h as f64;
    (cx, cy)
}

#[cfg(target_os = "macos")]
mod vision {
    use super::{box_center_to_pixels, OcrLine};
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::AnyObject;
    use objc2::AnyThread;
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSString};
    use objc2_vision::{
        VNImageOption, VNImageRequestHandler, VNRecognizeTextRequest, VNRequest,
        VNRequestTextRecognitionLevel,
    };

    /// Recognize text in `image` (encoded PNG/JPEG of `img_w` × `img_h` pixels)
    /// using the given BCP-47 language hints (e.g. `["zh-Hans", "en-US"]`).
    ///
    /// Synchronous and self-contained (creates and drops all Objective-C objects
    /// internally), so it is safe to call from a `spawn_blocking` thread.
    pub fn recognize(
        image: &[u8],
        img_w: u32,
        img_h: u32,
        languages: &[&str],
    ) -> Result<Vec<OcrLine>, String> {
        autoreleasepool(|_| {
            let request = VNRecognizeTextRequest::new();
            request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
            request.setUsesLanguageCorrection(true);
            if !languages.is_empty() {
                let langs: Vec<Retained<NSString>> =
                    languages.iter().map(|l| NSString::from_str(l)).collect();
                request.setRecognitionLanguages(&NSArray::from_retained_slice(&langs));
            }

            // A request handler over the encoded image bytes — Vision decodes the
            // image itself (JPEG/PNG), so no CGImage construction is needed.
            let data = NSData::with_bytes(image);
            let options = NSDictionary::<VNImageOption, AnyObject>::new();
            let handler = VNImageRequestHandler::initWithData_options(
                VNImageRequestHandler::alloc(),
                &data,
                &options,
            );

            let req_ref: &VNRequest = &request;
            let requests = NSArray::from_slice(&[req_ref]);
            handler
                .performRequests_error(&requests)
                .map_err(|e| format!("Vision performRequests failed: {e:?}"))?;

            let mut lines = Vec::new();
            if let Some(results) = request.results() {
                for obs in results.to_vec() {
                    let candidates = obs.topCandidates(1);
                    let Some(top) = candidates.to_vec().into_iter().next() else {
                        continue;
                    };
                    let text = top.string().to_string();
                    if text.trim().is_empty() {
                        continue;
                    }
                    let bbox = unsafe { obs.boundingBox() };
                    let center = box_center_to_pixels(
                        bbox.origin.x,
                        bbox.origin.y,
                        bbox.size.width,
                        bbox.size.height,
                        img_w,
                        img_h,
                    );
                    lines.push(OcrLine {
                        text,
                        confidence: top.confidence(),
                        center,
                    });
                }
            }
            Ok(lines)
        })
    }
}

#[cfg(target_os = "macos")]
pub use vision::recognize;

/// OCR is macOS-only today; other platforms report it as unsupported rather
/// than silently returning zero lines, which would read as "no text on screen".
#[cfg(not(target_os = "macos"))]
pub fn recognize(
    _image: &[u8],
    _img_w: u32,
    _img_h: u32,
    _languages: &[&str],
) -> Result<Vec<OcrLine>, String> {
    Err("OCR is not supported on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::box_center_to_pixels;

    /// Normalized-box arithmetic lands a fraction of a pixel off exact decimal
    /// values, so compare within a tolerance far tighter than one pixel.
    #[track_caller]
    fn assert_center(actual: (f64, f64), expected: (f64, f64)) {
        let (dx, dy) = (actual.0 - expected.0, actual.1 - expected.1);
        assert!(
            dx.abs() < 1e-6 && dy.abs() < 1e-6,
            "expected {expected:?}, got {actual:?}"
        );
    }

    #[test]
    fn box_center_flips_the_y_axis() {
        // A box hugging Vision's BOTTOM-left origin must land near the BOTTOM
        // of a top-left pixel frame. Getting this backwards is the classic
        // Vision integration bug and puts every click on the wrong line.
        assert_center(
            box_center_to_pixels(0.0, 0.0, 0.5, 0.2, 1000, 500),
            (250.0, 450.0),
        );
    }

    #[test]
    fn box_center_maps_a_centered_box_to_the_image_center() {
        assert_center(
            box_center_to_pixels(0.25, 0.25, 0.5, 0.5, 800, 600),
            (400.0, 300.0),
        );
    }

    #[test]
    fn box_center_scales_to_the_image_dimensions() {
        // Same normalized box, different image: the center scales with it.
        assert_center(
            box_center_to_pixels(0.1, 0.8, 0.2, 0.1, 2000, 1000),
            (400.0, 150.0),
        );
        assert_center(
            box_center_to_pixels(0.1, 0.8, 0.2, 0.1, 1000, 500),
            (200.0, 75.0),
        );
    }
}
