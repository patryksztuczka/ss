//! On-device text recognition with Apple Vision (port of the `ss-helper ocr`
//! subcommand and `Ocr.ts`). Boxes are normalised 0..1 with a top-left
//! origin; the JSON shape matches `shared/ocr.ts` and the Electron cache.
//! Everything here blocks; run it on the background executor.

use std::cell::RefCell;
use std::fs;
use std::path::Path;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use color_eyre::eyre::{Result, WrapErr, eyre};
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2::{AllocAnyThread, Message, msg_send};
use objc2_core_foundation::CGRect;
use objc2_foundation::{
    NSArray, NSDictionary, NSError, NSRange, NSString, NSStringEnumerationOptions, NSURL,
};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedText, VNRectangleObservation,
    VNRequest, VNRequestTextRecognitionLevel,
};
use serde::{Deserialize, Serialize};

use crate::settings::serialize_number;

/// A normalised box with a top-left origin.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextBox {
    pub text: String,
    #[serde(serialize_with = "serialize_number")]
    pub x: f64,
    #[serde(serialize_with = "serialize_number")]
    pub y: f64,
    #[serde(serialize_with = "serialize_number")]
    pub w: f64,
    #[serde(serialize_with = "serialize_number")]
    pub h: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLine {
    pub text: String,
    #[serde(serialize_with = "serialize_number")]
    pub x: f64,
    #[serde(serialize_with = "serialize_number")]
    pub y: f64,
    #[serde(serialize_with = "serialize_number")]
    pub w: f64,
    #[serde(serialize_with = "serialize_number")]
    pub h: f64,
    pub words: Vec<TextBox>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextLayout {
    #[serde(serialize_with = "serialize_number")]
    pub width: f64,
    #[serde(serialize_with = "serialize_number")]
    pub height: f64,
    pub lines: Vec<TextLine>,
}

impl TextLayout {
    /// The text "Copy text" puts on the clipboard.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Reading order: top-to-bottom rows, then left-to-right within a row.
/// Uses V8's sort, because this comparator is not a consistent total order.
pub fn sort_lines(layout: &mut TextLayout) {
    crate::js_sort::sort_by(&mut layout.lines, |a, b| {
        let same_row = ((a.y + a.h / 2.0) - (b.y + b.h / 2.0)).abs() < a.h.min(b.h) / 2.0;
        if same_row { a.x - b.x } else { a.y - b.y }
    });
}

/// The cached layout at `cache`, or Vision's (sorted) result for `image`,
/// which is then written to `cache`. A failure is never cached.
pub fn load_or_recognize(image: &Path, cache: &Path) -> Result<TextLayout> {
    if let Some(layout) = fs::read(cache)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
    {
        return Ok(layout);
    }
    let mut layout = recognize(image)?;
    sort_lines(&mut layout);
    if let Err(error) = fs::write(cache, serde_json::to_vec(&layout)?) {
        tracing::warn!(%error, path = %cache.display(), "could not cache OCR");
    }
    Ok(layout)
}

/// Runs `VNRecognizeTextRequest` (accurate, language correction, automatic
/// language) on an image file. Lines come back in Vision's order.
pub fn recognize(image: &Path) -> Result<TextLayout> {
    let bytes = fs::read(image).wrap_err_with(|| format!("cannot read {}", image.display()))?;
    let (width, height) =
        crate::capture_store::png_size(&bytes).ok_or_else(|| eyre!("cannot read image"))?;
    objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&image.to_string_lossy()));
        // SAFETY: an empty options dictionary is valid for every key type.
        let handler = unsafe {
            VNImageRequestHandler::initWithURL_options(
                VNImageRequestHandler::alloc(),
                &url,
                &NSDictionary::new(),
            )
        };
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setAutomaticallyDetectsLanguage(true);
        let base: &VNRequest = &request;
        handler
            .performRequests_error(&NSArray::from_slice(&[base]))
            .map_err(|error| eyre!("recognition failed: {}", error.localizedDescription()))?;

        let mut lines = Vec::new();
        for observation in request.results().unwrap_or_default().iter() {
            let Some(candidate) = observation.topCandidates(1).firstObject() else {
                continue;
            };
            // SAFETY: Vision observations always carry a bounding box.
            let (x, y, w, h) = flip(unsafe { observation.boundingBox() });
            lines.push(TextLine {
                text: candidate.string().to_string(),
                x,
                y,
                w,
                h,
                words: words(&candidate),
            });
        }
        Ok(TextLayout {
            width: f64::from(width),
            height: f64::from(height),
            lines,
        })
    })
}

/// Splits a line into words like `String.enumerateSubstrings(.byWords)` and
/// boxes each with `boundingBoxForRange:error:`.
fn words(candidate: &VNRecognizedText) -> Vec<TextBox> {
    let text = candidate.string();
    let candidate = candidate.retain();
    let words = Rc::new(RefCell::new(Vec::new()));
    let collected = words.clone();
    let block = RcBlock::new(
        move |word: *mut NSString, range: NSRange, _: NSRange, _: NonNull<Bool>| {
            // SAFETY: the word, when present, is valid for the block's duration.
            let Some(word) = (unsafe { word.as_ref() }) else {
                return;
            };
            // objc2-vision has no binding for this selector, so send it directly.
            // SAFETY: `boundingBoxForRange:error:` exists on VNRecognizedText (macOS 10.15+).
            let observation: Result<Retained<VNRectangleObservation>, Retained<NSError>> =
                unsafe { msg_send![&*candidate, boundingBoxForRange: range, error: _] };
            if let Ok(observation) = observation {
                // SAFETY: rectangle observations always carry a bounding box.
                let (x, y, w, h) = flip(unsafe { observation.boundingBox() });
                collected.borrow_mut().push(TextBox {
                    text: word.to_string(),
                    x,
                    y,
                    w,
                    h,
                });
            }
        },
    );
    text.enumerateSubstringsInRange_options_usingBlock(
        NSRange::new(0, text.length()),
        NSStringEnumerationOptions::ByWords,
        &block,
    );
    drop(block);
    Rc::try_unwrap(words)
        .map(RefCell::into_inner)
        .unwrap_or_default()
}

/// Vision uses a bottom-left origin; the layout wants top-left.
fn flip(rect: CGRect) -> (f64, f64, f64, f64) {
    (
        rect.origin.x,
        1.0 - (rect.origin.y + rect.size.height),
        rect.size.width,
        rect.size.height,
    )
}

/// Largest coordinate difference between two layouts with the same lines and
/// words, or an error describing the first structural mismatch.
pub fn compare(ours: &TextLayout, reference: &TextLayout) -> std::result::Result<f64, String> {
    if (ours.width, ours.height) != (reference.width, reference.height) {
        return Err(format!(
            "size {}×{} vs {}×{}",
            ours.width, ours.height, reference.width, reference.height
        ));
    }
    if ours.lines.len() != reference.lines.len() {
        return Err(format!(
            "{} lines vs {}",
            ours.lines.len(),
            reference.lines.len()
        ));
    }
    let mut worst: f64 = 0.0;
    let delta = |a: [f64; 4], b: [f64; 4]| {
        a.iter()
            .zip(b)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max)
    };
    for (index, (a, b)) in ours.lines.iter().zip(&reference.lines).enumerate() {
        if a.text != b.text {
            return Err(format!("line {index}: {:?} vs {:?}", a.text, b.text));
        }
        if a.words.len() != b.words.len() {
            return Err(format!(
                "line {index}: {} words vs {}",
                a.words.len(),
                b.words.len()
            ));
        }
        worst = worst.max(delta([a.x, a.y, a.w, a.h], [b.x, b.y, b.w, b.h]));
        for (word_a, word_b) in a.words.iter().zip(&b.words) {
            if word_a.text != word_b.text {
                return Err(format!(
                    "line {index}: word {:?} vs {:?}",
                    word_a.text, word_b.text
                ));
            }
            worst = worst.max(delta(
                [word_a.x, word_a.y, word_a.w, word_a.h],
                [word_b.x, word_b.y, word_b.w, word_b.h],
            ));
        }
    }
    Ok(worst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(x: f64, y: f64, h: f64, index: usize) -> TextLine {
        TextLine {
            text: index.to_string(),
            x,
            y,
            w: 0.1,
            h,
            words: Vec::new(),
        }
    }

    #[test]
    fn reading_order_matches_v8() {
        #[derive(Deserialize)]
        struct Fixture {
            cases: Vec<Case>,
        }
        #[derive(Deserialize)]
        struct Case {
            lines: Vec<[f64; 3]>,
            order: Vec<usize>,
        }
        let fixture: Fixture =
            serde_json::from_str(include_str!("../tests/fixtures/v8-sort-lines.json")).unwrap();
        assert!(fixture.cases.len() >= 60);
        for (number, case) in fixture.cases.iter().enumerate() {
            let mut layout = TextLayout {
                width: 1.0,
                height: 1.0,
                lines: case
                    .lines
                    .iter()
                    .enumerate()
                    .map(|(index, [x, y, h])| line(*x, *y, *h, index))
                    .collect(),
            };
            sort_lines(&mut layout);
            let order: Vec<usize> = layout
                .lines
                .iter()
                .map(|line| line.text.parse().unwrap())
                .collect();
            assert_eq!(
                order,
                case.order,
                "case {number} ({} lines)",
                case.lines.len()
            );
        }
    }

    #[test]
    fn rows_read_left_to_right() {
        let mut layout = TextLayout {
            width: 100.0,
            height: 100.0,
            lines: vec![
                line(0.6, 0.101, 0.02, 0),
                line(0.1, 0.5, 0.02, 1),
                line(0.1, 0.1, 0.02, 2),
            ],
        };
        sort_lines(&mut layout);
        let texts: Vec<_> = layout.lines.iter().map(|line| line.text.as_str()).collect();
        assert_eq!(texts, ["2", "0", "1"]);
        assert_eq!(layout.text(), "2\n0\n1");
    }

    #[test]
    fn cache_json_matches_the_electron_shape() {
        let layout = TextLayout {
            width: 823.0,
            height: 621.0,
            lines: vec![TextLine {
                text: "Hi \"there\"".into(),
                x: 0.0,
                y: 0.25,
                w: 0.5,
                h: 0.0125,
                words: vec![TextBox {
                    text: "Hi".into(),
                    x: 0.0,
                    y: 0.25,
                    w: 0.1,
                    h: 0.0125,
                }],
            }],
        };
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(
            json,
            r#"{"width":823,"height":621,"lines":[{"text":"Hi \"there\"","x":0,"y":0.25,"w":0.5,"h":0.0125,"words":[{"text":"Hi","x":0,"y":0.25,"w":0.1,"h":0.0125}]}]}"#
        );
        assert_eq!(serde_json::from_str::<TextLayout>(&json).unwrap(), layout);
    }

    /// Rust Vision output against the Electron Swift helper's, on a capture of
    /// ss's own onboarding screen (no personal text). The reference is
    /// `ss-helper ocr` built from `apps/desktop/native/helper.swift` (removed
    /// after d2a487e), then Electron's `sortLines` + `JSON.stringify`.
    #[test]
    fn matches_the_swift_helper_on_the_fixture() {
        let image = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ocr-welcome.png");
        let reference: TextLayout =
            serde_json::from_str(include_str!("../tests/fixtures/ocr-welcome.ocr.json")).unwrap();
        let mut ours = recognize(&image).unwrap();
        sort_lines(&mut ours);
        let worst = compare(&ours, &reference).unwrap();
        assert!(worst < 1e-6, "boxes differ by {worst}");
    }

    #[test]
    fn cached_layouts_are_read_before_running_vision() {
        let directory = tempfile::tempdir().unwrap();
        let cache = directory.path().join("x.ocr.json");
        let layout = TextLayout {
            width: 1.0,
            height: 1.0,
            lines: vec![line(0.1, 0.1, 0.1, 7)],
        };
        fs::write(&cache, serde_json::to_vec(&layout).unwrap()).unwrap();
        // The image does not exist, so only the cache can answer.
        let loaded = load_or_recognize(&directory.path().join("missing.png"), &cache).unwrap();
        assert_eq!(loaded, layout);
        fs::write(&cache, b"{broken").unwrap();
        assert!(load_or_recognize(&directory.path().join("missing.png"), &cache).is_err());
    }
}
