//! Preview thumbnails. Blocking; run on the background executor.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use color_eyre::eyre::{Result, eyre};
use gpui::RenderImage;
use image::imageops::FilterType;
use image::{Frame, ImageFormat, RgbaImage};

/// The preview card is 248 pt wide; thumbnails are 2× for Retina.
pub const PREVIEW_THUMBNAIL_WIDTH: u32 = 496;
/// Electron's drag icon width.
pub const DRAG_ICON_WIDTH: u32 = 160;

/// A decoded thumbnail ready for GPUI, plus a small PNG for the drag image.
#[derive(Clone)]
pub struct Thumbnail {
    pub image: Arc<RenderImage>,
    pub drag_png: Arc<Vec<u8>>,
}

/// Aspect-correct resize of a PNG, `width` px wide (Electron's `thumbnail`).
pub fn from_png(png: &[u8], width: u32) -> Result<RgbaImage> {
    let image = image::load_from_memory_with_format(png, ImageFormat::Png)?.into_rgba8();
    Ok(resize(&image, width))
}

fn resize(image: &RgbaImage, width: u32) -> RgbaImage {
    let height = scaled_height(image.width(), image.height(), width);
    image::imageops::resize(image, width, height, FilterType::CatmullRom)
}

/// `max(1, round(width · h / w))`.
pub fn scaled_height(source_width: u32, source_height: u32, width: u32) -> u32 {
    let height = (f64::from(width) * f64::from(source_height) / f64::from(source_width.max(1))
        + 0.5)
        .floor();
    (height as u32).max(1)
}

/// A poster frame for a recording via QuickLook (`QLThumbnailGenerator`),
/// `width` px wide. Blocking; run on the background executor.
pub fn video_poster(path: &Path, width: u32) -> Result<RgbaImage> {
    use std::sync::mpsc;
    use std::time::Duration;

    use block2::RcBlock;
    use objc2::AllocAnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep};
    use objc2_core_foundation::CGSize;
    use objc2_foundation::{NSDictionary, NSError, NSString, NSURL};
    use objc2_quick_look_thumbnailing::{
        QLThumbnailGenerationRequest, QLThumbnailGenerationRequestRepresentationTypes,
        QLThumbnailGenerator, QLThumbnailRepresentation,
    };

    let png = objc2::rc::autoreleasepool(|_| {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        // The longest side fits the box, so leave room for portrait movies.
        let size = CGSize::new(f64::from(width), f64::from(width) * 2.0);
        // SAFETY: a file URL, a positive size and a valid type mask.
        let request = unsafe {
            QLThumbnailGenerationRequest::initWithFileAtURL_size_scale_representationTypes(
                QLThumbnailGenerationRequest::alloc(),
                &url,
                size,
                1.0,
                QLThumbnailGenerationRequestRepresentationTypes::Thumbnail,
            )
        };
        let (sender, receiver) = mpsc::channel::<std::result::Result<Vec<u8>, String>>();
        let done = RcBlock::new(
            move |representation: *mut QLThumbnailRepresentation, error: *mut NSError| {
                // SAFETY: QuickLook passes valid (possibly null) objects.
                let result = match unsafe { representation.as_ref() } {
                    Some(representation) => {
                        let image = unsafe { representation.CGImage() };
                        let bitmap =
                            NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
                        // SAFETY: an empty property dictionary is valid.
                        unsafe {
                            bitmap.representationUsingType_properties(
                                NSBitmapImageFileType::PNG,
                                &NSDictionary::new(),
                            )
                        }
                        .map(|data| data.to_vec())
                        .ok_or_else(|| "cannot encode the poster".to_string())
                    }
                    None => Err(unsafe { error.as_ref() }.map_or_else(
                        || "no thumbnail".into(),
                        |error| error.localizedDescription().to_string(),
                    )),
                };
                let _ = sender.send(result);
            },
        );
        // SAFETY: the request and handler outlive the call; the handler may run on any queue.
        unsafe {
            QLThumbnailGenerator::sharedGenerator()
                .generateBestRepresentationForRequest_completionHandler(&request, &done);
        }
        receiver
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| eyre!("QuickLook did not answer"))?
            .map_err(|error| eyre!("QuickLook: {error}"))
    })?;
    from_png(&png, width)
}

/// The preview thumbnail when there is no poster: a neutral card.
pub fn empty() -> RgbaImage {
    RgbaImage::from_pixel(2, 2, image::Rgba([0xef, 0xec, 0xe8, 255]))
}

/// The preview thumbnail and the 160 px drag image for a capture.
pub fn for_preview(source: &RgbaImage) -> Result<Thumbnail> {
    let drag = resize(source, DRAG_ICON_WIDTH);
    let mut drag_png = Vec::new();
    drag.write_to(&mut Cursor::new(&mut drag_png), ImageFormat::Png)?;
    Ok(Thumbnail {
        image: render_image(source.clone()),
        drag_png: Arc::new(drag_png),
    })
}

/// GPUI wants BGRA frames.
pub fn render_image(mut image: RgbaImage) -> Arc<RenderImage> {
    for pixel in image.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    Arc::new(RenderImage::new(vec![Frame::new(image)]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_aspect_ratio() {
        assert_eq!(scaled_height(1601, 948, 496), 294);
        assert_eq!(scaled_height(823, 621, 496), 374);
        assert_eq!(scaled_height(4000, 10, 160), 1);
        assert_eq!(scaled_height(0, 100, 160), 16000);
    }

    #[test]
    fn resizes_png_bytes() {
        let source = RgbaImage::from_pixel(1000, 500, image::Rgba([255, 0, 0, 255]));
        let mut png = Vec::new();
        source
            .write_to(&mut Cursor::new(&mut png), ImageFormat::Png)
            .unwrap();
        let thumbnail = from_png(&png, PREVIEW_THUMBNAIL_WIDTH).unwrap();
        assert_eq!(thumbnail.dimensions(), (496, 248));
        let preview = for_preview(&thumbnail).unwrap();
        assert_eq!(
            crate::capture_store::png_size(&preview.drag_png),
            Some((160, 80))
        );
    }
}
