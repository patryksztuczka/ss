//! The editor's compositor (port of `renderer/src/lib/compose.ts`), on
//! tiny-skia instead of Canvas 2D. Pure and blocking: run it on the
//! background executor. Pixels are sRGB; sources are colour-managed from
//! their embedded ICC profile first, as Chromium does when decoding.

use std::io::Cursor;

use color_eyre::eyre::{Result, eyre};
use image::codecs::png::PngDecoder;
use image::{DynamicImage, ImageDecoder, RgbaImage};
use tiny_skia::{
    Color, FillRule, FilterQuality, IntSize, Mask, Paint, PathBuilder, Pixmap, PixmapPaint,
    PremultipliedColorU8, Rect, Shader, Transform,
};

use crate::backgrounds;
use crate::settings::{AspectRatio, EditorStyle};

/// An axis-aligned box in output pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Box {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

/// Everything in output pixels at 1:1 with the source screenshot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub width: i64,
    pub height: i64,
    /// The rounded "card": screenshot plus inset.
    pub card: Box,
    /// Where the screenshot itself lands.
    pub image: Box,
    pub radius: f64,
    pub inset: i64,
}

/// JavaScript's `Math.round`: halves round up, towards +∞.
fn js_round(value: f64) -> f64 {
    (value + 0.5).floor()
}

fn ratio(ratio: AspectRatio) -> Option<f64> {
    match ratio {
        AspectRatio::Auto => None,
        AspectRatio::R16x9 => Some(16.0 / 9.0),
        AspectRatio::R4x3 => Some(4.0 / 3.0),
        AspectRatio::R3x2 => Some(3.0 / 2.0),
        AspectRatio::R1x1 => Some(1.0),
    }
}

/// `computeLayout`: sizes scale with the screenshot (geometric mean of its
/// sides), so a style looks the same on a tiny selection and a 5K capture.
pub fn compute_layout(style: &EditorStyle, image_width: u32, image_height: u32) -> Layout {
    let (image_width, image_height) = (f64::from(image_width), f64::from(image_height));
    let base = (image_width * image_height).sqrt();
    let padding = js_round(style.padding / 100.0 * 0.24 * base);
    let inset = js_round(style.inset / 100.0 * 0.1 * base);
    let radius = style.radius / 100.0 * 0.045 * base;

    let card_width = image_width + inset * 2.0;
    let card_height = image_height + inset * 2.0;
    let mut width = card_width + padding * 2.0;
    let mut height = card_height + padding * 2.0;
    if let Some(ratio) = ratio(style.ratio) {
        if width / height < ratio {
            width = js_round(height * ratio);
        } else {
            height = js_round(width / ratio);
        }
    }
    let card = Box {
        x: js_round((width - card_width) / 2.0) as i64,
        y: js_round((height - card_height) / 2.0) as i64,
        width: card_width as i64,
        height: card_height as i64,
    };
    Layout {
        width: width as i64,
        height: height as i64,
        card,
        image: Box {
            x: card.x + inset as i64,
            y: card.y + inset as i64,
            width: image_width as i64,
            height: image_height as i64,
        },
        radius,
        inset: inset as i64,
    }
}

/// A decoded screenshot: sRGB pixels plus a mip chain for quality downscaling.
pub struct Source {
    rgba: RgbaImage,
    /// Premultiplied; level 0 is full size, each next level half the last.
    levels: Vec<Pixmap>,
}

impl Source {
    /// Decodes a PNG and converts it to sRGB using its ICC profile.
    pub fn from_png(png: &[u8]) -> Result<Self> {
        let mut decoder = PngDecoder::new(Cursor::new(png))?;
        let icc = decoder.icc_profile().ok().flatten();
        let mut rgba = DynamicImage::from_decoder(decoder)?.into_rgba8();
        if let Some(icc) = icc {
            to_srgb(&mut rgba, &icc);
        }
        Ok(Self::from_rgba(rgba))
    }

    pub fn from_rgba(rgba: RgbaImage) -> Self {
        let mut levels = vec![premultiplied(&rgba)];
        while let Some(last) = levels.last()
            && last.width() > 1
            && last.height() > 1
            && last.width().max(last.height()) > 64
        {
            let next = half_size(last);
            levels.push(next);
        }
        Self { rgba, levels }
    }

    pub fn width(&self) -> u32 {
        self.rgba.width()
    }

    pub fn height(&self) -> u32 {
        self.rgba.height()
    }

    /// The sRGB pixels, unpremultiplied.
    pub fn rgba(&self) -> &RgbaImage {
        &self.rgba
    }
}

fn to_srgb(rgba: &mut RgbaImage, icc: &[u8]) {
    let Some(input) = qcms::Profile::new_from_slice(icc, false) else {
        tracing::debug!("unreadable ICC profile; treating the image as sRGB");
        return;
    };
    let output = qcms::Profile::new_sRGB();
    match qcms::Transform::new(
        &input,
        &output,
        qcms::DataType::RGBA8,
        qcms::Intent::default(),
    ) {
        Some(transform) => transform.apply(rgba),
        None => tracing::debug!("unsupported ICC profile; treating the image as sRGB"),
    }
}

fn premultiplied(rgba: &RgbaImage) -> Pixmap {
    let mut data = rgba.as_raw().clone();
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        for channel in &mut pixel[..3] {
            *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
        }
    }
    let size = IntSize::from_wh(rgba.width().max(1), rgba.height().max(1)).expect("non-empty");
    Pixmap::from_vec(data, size).expect("sized buffer")
}

/// 2×2 box downsample of a premultiplied pixmap.
fn half_size(pixmap: &Pixmap) -> Pixmap {
    let (width, height) = ((pixmap.width() / 2).max(1), (pixmap.height() / 2).max(1));
    let mut half = Pixmap::new(width, height).expect("non-empty");
    let source = pixmap.pixels();
    let stride = pixmap.width() as usize;
    let pixels = half.pixels_mut();
    for y in 0..height as usize {
        for x in 0..width as usize {
            let mut sum = [0u32; 4];
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let sx = (x * 2 + dx).min(stride - 1);
                let sy = (y * 2 + dy).min(pixmap.height() as usize - 1);
                let pixel = source[sy * stride + sx];
                sum[0] += u32::from(pixel.red());
                sum[1] += u32::from(pixel.green());
                sum[2] += u32::from(pixel.blue());
                sum[3] += u32::from(pixel.alpha());
            }
            let [r, g, b, a] = sum.map(|channel| ((channel + 2) / 4) as u8);
            pixels[y * width as usize + x] =
                PremultipliedColorU8::from_rgba(r.min(a), g.min(a), b.min(a), a)
                    .expect("premultiplied");
        }
    }
    half
}

/// `roundRect` with `min(radius, w/2, h/2)`, as cubic quarter circles.
fn rounded_rect(width: f32, height: f32, radius: f32) -> Option<tiny_skia::Path> {
    let radius = radius.min(width / 2.0).min(height / 2.0).max(0.0);
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(Rect::from_xywh(
            0.0, 0.0, width, height,
        )?));
    }
    let k = radius * 0.552_284_8;
    let (w, h, r) = (width, height, radius);
    let mut path = PathBuilder::new();
    path.move_to(r, 0.0);
    path.line_to(w - r, 0.0);
    path.cubic_to(w - r + k, 0.0, w, r - k, w, r);
    path.line_to(w, h - r);
    path.cubic_to(w, h - r + k, w - r + k, h, w - r, h);
    path.line_to(r, h);
    path.cubic_to(r - k, h, 0.0, h - r + k, 0.0, h - r);
    path.line_to(0.0, r);
    path.cubic_to(0.0, r - k, r - k, 0.0, r, 0.0);
    path.close();
    path.finish()
}

/// `renderComposition` at `scale` (1 = full resolution export; smaller for
/// the on-screen preview). Returns a premultiplied pixmap.
pub fn render(source: &Source, style: &EditorStyle, inset_color: [u8; 3], scale: f64) -> Pixmap {
    let layout = compute_layout(style, source.width(), source.height());
    let width = js_round(layout.width as f64 * scale).max(1.0) as u32;
    let height = js_round(layout.height as f64 * scale).max(1.0) as u32;
    let mut canvas = Pixmap::new(width, height).expect("non-empty");
    backgrounds::paint(
        &mut canvas,
        backgrounds::by_id(style.background),
        &style.custom_color,
    );

    // The card is built off-screen so the shadow follows its exact silhouette.
    let card_width = (layout.card.width as f64 * scale).ceil().max(1.0) as u32;
    let card_height = (layout.card.height as f64 * scale).ceil().max(1.0) as u32;
    let mut card = Pixmap::new(card_width, card_height).expect("non-empty");
    let scale32 = scale as f32;
    let mut clip = Mask::new(card_width, card_height).expect("non-empty");
    if let Some(path) = rounded_rect(
        layout.card.width as f32,
        layout.card.height as f32,
        layout.radius as f32,
    ) {
        clip.fill_path(
            &path,
            FillRule::Winding,
            true,
            Transform::from_scale(scale32, scale32),
        );
    }
    if layout.inset > 0 {
        let [r, g, b] = inset_color;
        let paint = Paint {
            shader: Shader::SolidColor(Color::from_rgba8(r, g, b, 255)),
            ..Paint::default()
        };
        if let Some(area) = Rect::from_xywh(0.0, 0.0, card_width as f32, card_height as f32) {
            card.fill_rect(area, &paint, Transform::identity(), Some(&clip));
        }
    }
    draw_source(&mut card, source, layout.inset as f64 * scale, scale, &clip);

    let card_x = layout.card.x as f64 * scale;
    let card_y = layout.card.y as f64 * scale;
    let strength = style.shadow / 100.0;
    if strength > 0.0 {
        let base = (layout.image.width as f64 * layout.image.height as f64).sqrt();
        let blur = strength * 0.07 * base * scale;
        let offset_y = strength * 0.022 * base * scale;
        let alpha = 0.12 + strength * 0.33;
        draw_shadow(
            &mut canvas,
            &card,
            card_x,
            card_y + offset_y,
            blur / 2.0,
            alpha,
        );
    }
    draw_at(&mut canvas, &card, card_x, card_y);
    canvas
}

/// `drawImage(source, inset, inset, w, h)` with high-quality filtering.
fn draw_source(card: &mut Pixmap, source: &Source, offset: f64, scale: f64, clip: &Mask) {
    if scale >= 1.0 {
        card.draw_pixmap(
            offset.round() as i32,
            offset.round() as i32,
            source.levels[0].as_ref(),
            &PixmapPaint::default(),
            Transform::identity(),
            Some(clip),
        );
        return;
    }
    // Mipmapped: start from the smallest level still at least the target size.
    let mut level = 0;
    while level + 1 < source.levels.len() && scale * f64::powi(2.0, level as i32 + 1) <= 1.0 {
        level += 1;
    }
    let factor = (scale * f64::powi(2.0, level as i32)) as f32;
    let paint = PixmapPaint {
        quality: FilterQuality::Bicubic,
        ..PixmapPaint::default()
    };
    let transform =
        Transform::from_scale(factor, factor).post_translate(offset as f32, offset as f32);
    card.draw_pixmap(
        0,
        0,
        source.levels[level].as_ref(),
        &paint,
        transform,
        Some(clip),
    );
}

fn draw_at(canvas: &mut Pixmap, pixmap: &Pixmap, x: f64, y: f64) {
    let integral = x.fract() == 0.0 && y.fract() == 0.0;
    let paint = PixmapPaint {
        quality: if integral {
            FilterQuality::Nearest
        } else {
            FilterQuality::Bilinear
        },
        ..PixmapPaint::default()
    };
    canvas.draw_pixmap(
        0,
        0,
        pixmap.as_ref(),
        &paint,
        Transform::from_translate(x as f32, y as f32),
        None,
    );
}

/// Canvas `shadowColor`/`shadowBlur`: the card's alpha, blurred with a
/// Gaussian of `sigma` (= shadowBlur / 2), tinted `rgba(15, 20, 40, alpha)`.
fn draw_shadow(canvas: &mut Pixmap, card: &Pixmap, x: f64, y: f64, sigma: f64, alpha: f64) {
    let pad = (sigma * 3.0).ceil() as usize + 2;
    let (width, height) = (
        card.width() as usize + pad * 2,
        card.height() as usize + pad * 2,
    );
    let mut mask = vec![0f32; width * height];
    for (index, pixel) in card.pixels().iter().enumerate() {
        let (cx, cy) = (index % card.width() as usize, index / card.width() as usize);
        mask[(cy + pad) * width + cx + pad] = f32::from(pixel.alpha()) / 255.0;
    }
    gaussian_blur(&mut mask, width, height, sigma);
    let mut data = Vec::with_capacity(width * height * 4);
    for value in mask {
        let a = (value as f64 * alpha).clamp(0.0, 1.0);
        let premultiply = |channel: f64| (channel * a).round() as u8;
        data.extend([
            premultiply(15.0),
            premultiply(20.0),
            premultiply(40.0),
            (a * 255.0).round() as u8,
        ]);
    }
    let size = IntSize::from_wh(width as u32, height as u32).expect("non-empty");
    let shadow = Pixmap::from_vec(data, size).expect("sized buffer");
    draw_at(canvas, &shadow, x - pad as f64, y - pad as f64);
}

/// Three box blurs approximating a Gaussian (as Skia does for large sigma).
fn gaussian_blur(data: &mut [f32], width: usize, height: usize, sigma: f64) {
    if sigma < 0.5 {
        return;
    }
    let mut scratch = vec![0f32; data.len()];
    for size in box_sizes(sigma, 3) {
        let radius = (size - 1) / 2;
        box_blur(data, &mut scratch, width, height, radius, true);
        box_blur(&scratch, data, width, height, radius, false);
    }
}

/// Box widths whose successive passes approximate a Gaussian of `sigma`.
fn box_sizes(sigma: f64, passes: usize) -> Vec<usize> {
    let n = passes as f64;
    let ideal = (12.0 * sigma * sigma / n + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1) as f64;
    let upper = lower + 2.0;
    let m = ((12.0 * sigma * sigma - n * lower * lower - 4.0 * n * lower - 3.0 * n)
        / (-4.0 * lower - 4.0))
        .round();
    (0..passes)
        .map(|index| if (index as f64) < m { lower } else { upper } as usize)
        .collect()
}

/// One box pass over rows (`horizontal`) or columns; outside counts as 0.
fn box_blur(
    source: &[f32],
    target: &mut [f32],
    width: usize,
    height: usize,
    radius: usize,
    horizontal: bool,
) {
    let (lines, length, step, line_step) = if horizontal {
        (height, width, 1, width)
    } else {
        (width, height, width, 1)
    };
    let scale = 1.0 / (radius * 2 + 1) as f32;
    for line in 0..lines {
        let start = line * line_step;
        let at = |index: usize| source[start + index * step];
        let mut sum: f32 = (0..=radius.min(length - 1)).map(at).sum();
        for index in 0..length {
            target[start + index * step] = sum * scale;
            if index + radius + 1 < length {
                sum += at(index + radius + 1);
            }
            if index >= radius {
                sum -= at(index - radius);
            }
        }
    }
}

/// `detectEdgeColor`: the most common colour along the border of a 64×64
/// downsample, quantised to 4 bits per channel.
pub fn detect_edge_color(source: &Source) -> [u8; 3] {
    const SIZE: usize = 64;
    let sample = bilinear(source.rgba(), SIZE as u32, SIZE as u32);
    // Insertion-ordered counts: ties go to the colour seen first, like the JS Map.
    let mut counts: Vec<(u16, usize)> = Vec::new();
    for i in 0..SIZE {
        for (x, y) in [(i, 0), (i, SIZE - 1), (0, i), (SIZE - 1, i)] {
            let [r, g, b, _] = sample.get_pixel(x as u32, y as u32).0;
            let key = (u16::from(r >> 4) << 8) | (u16::from(g >> 4) << 4) | u16::from(b >> 4);
            match counts.iter_mut().find(|(candidate, _)| *candidate == key) {
                Some((_, count)) => *count += 1,
                None => counts.push((key, 1)),
            }
        }
    }
    let mut best = (0xfff, 0);
    for &(key, count) in &counts {
        if count > best.1 {
            best = (key, count);
        }
    }
    let channel = |shift: u16| (((best.0 >> shift) & 15) * 17) as u8;
    [channel(8), channel(4), channel(0)]
}

pub fn hex_color([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Canvas `drawImage` at default ("low") smoothing: plain bilinear sampling
/// at pixel centres, no mipmaps.
fn bilinear(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (source_width, source_height) = (image.width() as f64, image.height() as f64);
    RgbaImage::from_fn(width, height, |x, y| {
        let sx =
            ((x as f64 + 0.5) * source_width / width as f64 - 0.5).clamp(0.0, source_width - 1.0);
        let sy = ((y as f64 + 0.5) * source_height / height as f64 - 0.5)
            .clamp(0.0, source_height - 1.0);
        let (x0, y0) = (sx.floor() as u32, sy.floor() as u32);
        let (x1, y1) = (
            (x0 + 1).min(image.width() - 1),
            (y0 + 1).min(image.height() - 1),
        );
        let (fx, fy) = (sx - x0 as f64, sy - y0 as f64);
        let mut out = [0u8; 4];
        for (channel, value) in out.iter_mut().enumerate() {
            let p = |px: u32, py: u32| f64::from(image.get_pixel(px, py).0[channel]);
            let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
            let bottom = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
            *value = (top * (1.0 - fy) + bottom * fy).round() as u8;
        }
        image::Rgba(out)
    })
}

/// `exportPng`: the full-resolution composition.
pub fn export_png(source: &Source, style: &EditorStyle, inset_color: [u8; 3]) -> Result<Vec<u8>> {
    render(source, style, inset_color, 1.0)
        .encode_png()
        .map_err(|error| eyre!("cannot encode PNG: {error}"))
}

/// The on-screen raster, unpremultiplied for GPUI.
pub fn render_preview(
    source: &Source,
    style: &EditorStyle,
    inset_color: [u8; 3],
    scale: f64,
) -> RgbaImage {
    let pixmap = render(source, style, inset_color, scale.min(1.0));
    let (width, height) = (pixmap.width(), pixmap.height());
    let mut data = pixmap.take();
    for pixel in data.as_chunks_mut::<4>().0 {
        let alpha = u16::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    RgbaImage::from_raw(width, height, data).expect("sized buffer")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde::Deserialize;

    use super::*;
    use crate::settings::BackgroundId;

    fn style() -> EditorStyle {
        EditorStyle::default()
    }

    #[test]
    fn layout_matches_compute_layout() {
        // 1601×948: base = sqrt(1517748) = 1231.97…
        let layout = compute_layout(&style(), 1601, 948);
        // padding = round(0.36 · 0.24 · base) = round(106.44) = 106
        assert_eq!((layout.width, layout.height), (1601 + 212, 948 + 212));
        assert_eq!(
            layout.card,
            Box {
                x: 106,
                y: 106,
                width: 1601,
                height: 948
            }
        );
        assert!((layout.radius - 0.3 * 0.045 * 1_517_748f64.sqrt()).abs() < 1e-9);

        let wide = EditorStyle {
            padding: 42.0,
            inset: 10.0,
            ratio: AspectRatio::R16x9,
            ..style()
        };
        let layout = compute_layout(&wide, 823, 621);
        // base = 714.9…, padding = round(72.06) = 72, inset = round(7.149) = 7
        assert_eq!(layout.inset, 7);
        assert_eq!(layout.card.width, 823 + 14);
        let natural_height = 621 + 14 + 144;
        assert_eq!(layout.height, natural_height);
        assert_eq!(
            layout.width,
            js_round(natural_height as f64 * 16.0 / 9.0) as i64
        );
        assert_eq!(
            layout.card.x,
            js_round((layout.width - 837) as f64 / 2.0) as i64
        );
        assert_eq!(layout.image.x, layout.card.x + 7);

        let square = EditorStyle {
            ratio: AspectRatio::R1x1,
            ..style()
        };
        let layout = compute_layout(&square, 300, 100);
        assert_eq!(layout.width, layout.height);
        assert!(layout.height > layout.card.height);
    }

    #[test]
    fn js_round_rounds_halves_up() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(0.49), 0.0);
    }

    #[test]
    fn box_sizes_cover_the_gaussian_variance() {
        for sigma in [1.0, 4.0, 13.7, 60.0] {
            let sizes = box_sizes(sigma, 3);
            let variance: f64 = sizes.iter().map(|&w| ((w * w - 1) as f64) / 12.0).sum();
            assert!((variance.sqrt() - sigma).abs() < 0.6, "{sigma}: {sizes:?}");
        }
    }

    #[test]
    fn edge_color_votes_on_the_border() {
        let mut image = RgbaImage::from_pixel(200, 120, image::Rgba([0x30, 0x60, 0x90, 255]));
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            if x > 20 && x < 180 && y > 20 && y < 100 {
                *pixel = image::Rgba([255, 0, 0, 255]);
            }
        }
        let color = detect_edge_color(&Source::from_rgba(image));
        assert_eq!(hex_color(color), "#336699");
    }

    #[test]
    fn transparent_backgrounds_keep_the_shadow_and_card() {
        let source = Source::from_rgba(RgbaImage::from_pixel(
            100,
            60,
            image::Rgba([10, 20, 30, 255]),
        ));
        let none = EditorStyle {
            background: BackgroundId::None,
            ..style()
        };
        let pixmap = render(&source, &none, [255, 255, 255], 1.0);
        assert_eq!(pixmap.pixel(0, 0).unwrap().alpha(), 0);
        let layout = compute_layout(&none, 100, 60);
        let below = pixmap
            .pixel(
                (layout.card.x + 50) as u32,
                (layout.card.y + layout.card.height + 2) as u32,
            )
            .unwrap();
        assert!(below.alpha() > 0, "the shadow falls below the card");
        let inside = pixmap
            .pixel((layout.card.x + 50) as u32, (layout.card.y + 30) as u32)
            .unwrap();
        assert_eq!(
            (inside.red(), inside.green(), inside.blue(), inside.alpha()),
            (10, 20, 30, 255)
        );
    }

    #[test]
    fn preview_scale_shrinks_the_raster() {
        let source = Source::from_rgba(RgbaImage::from_pixel(
            1000,
            600,
            image::Rgba([200, 200, 200, 255]),
        ));
        let layout = compute_layout(&style(), 1000, 600);
        let preview = render_preview(&source, &style(), [0, 0, 0], 0.25);
        assert_eq!(
            preview.dimensions(),
            (
                js_round(layout.width as f64 * 0.25) as u32,
                js_round(layout.height as f64 * 0.25) as u32
            )
        );
    }

    #[derive(Deserialize)]
    struct Manifest {
        #[serde(rename = "insetColor")]
        inset_color: String,
        cases: Vec<Case>,
    }

    #[derive(Deserialize)]
    struct Case {
        file: String,
        style: serde_json::Value,
        layout: ReferenceLayout,
    }

    #[derive(Deserialize)]
    struct ReferenceLayout {
        width: i64,
        height: i64,
        radius: f64,
        inset: i64,
        card: ReferenceBox,
        image: ReferenceBox,
    }

    #[derive(Deserialize)]
    struct ReferenceBox {
        x: i64,
        y: i64,
        width: i64,
        height: i64,
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/compose")
                .join(name),
        )
        .unwrap()
    }

    fn manifest() -> Manifest {
        serde_json::from_slice(&fixture("manifest.json")).unwrap()
    }

    /// Per-channel mean and maximum absolute error between two RGBA images,
    /// on premultiplied values: colour channels of nearly transparent pixels
    /// are quantisation noise (alpha 6 can store 0 or 43 for the same look).
    pub(super) fn difference(a: &RgbaImage, b: &RgbaImage) -> (f64, u8) {
        assert_eq!(a.dimensions(), b.dimensions());
        let premultiply = |pixel: &[u8; 4]| {
            let alpha = u16::from(pixel[3]);
            [0, 1, 2]
                .map(|channel| ((u16::from(pixel[channel]) * alpha + 127) / 255) as u8)
                .into_iter()
                .chain([pixel[3]])
        };
        let mut total = 0u64;
        let mut worst = 0u8;
        for (x, y) in a
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .zip(b.as_raw().as_chunks::<4>().0)
        {
            for (x, y) in premultiply(x).zip(premultiply(y)) {
                let delta = x.abs_diff(y);
                total += u64::from(delta);
                worst = worst.max(delta);
            }
        }
        (total as f64 / a.as_raw().len() as f64, worst)
    }

    #[test]
    fn layouts_match_the_electron_reference() {
        let source = Source::from_png(&fixture("source.png")).unwrap();
        for case in manifest().cases {
            let style =
                crate::settings::Settings::from_value(&serde_json::json!({ "editor": case.style }))
                    .editor;
            let layout = compute_layout(&style, source.width(), source.height());
            let reference = &case.layout;
            assert_eq!(
                (layout.width, layout.height),
                (reference.width, reference.height),
                "{}",
                case.file
            );
            assert_eq!(layout.inset, reference.inset);
            assert!((layout.radius - reference.radius).abs() < 1e-9);
            for (ours, theirs) in [
                (layout.card, &reference.card),
                (layout.image, &reference.image),
            ] {
                assert_eq!(
                    (ours.x, ours.y, ours.width, ours.height),
                    (theirs.x, theirs.y, theirs.width, theirs.height),
                    "{}",
                    case.file
                );
            }
        }
    }

    #[test]
    fn colour_management_matches_chromium() {
        let source = Source::from_png(&fixture("source.png")).unwrap();
        let reference = image::load_from_memory(&fixture("source-srgb.png"))
            .unwrap()
            .into_rgba8();
        let (mean, worst) = difference(source.rgba(), &reference);
        assert!(mean < 0.5 && worst <= 3, "mean {mean:.3}, max {worst}");
    }

    #[test]
    fn edge_colour_matches_the_electron_reference() {
        let source = Source::from_png(&fixture("source.png")).unwrap();
        assert_eq!(
            hex_color(detect_edge_color(&source)),
            manifest().inset_color
        );
    }

    /// Thresholds: measured means are 0.00–0.21 levels with maxima 1–31,
    /// which sit on antialiased corner edges and in the shadow (a box
    /// approximation of Skia's blur). A mean under 0.5 with no channel off by
    /// more than 40 still fails on any structural mistake (wrong layout,
    /// radius, colour, gradient direction, missing or offset shadow).
    #[test]
    fn exports_match_the_electron_reference() {
        let png = fixture("source.png");
        let source = Source::from_png(&png).unwrap();
        let manifest = manifest();
        let inset = backgrounds::parse_color(&manifest.inset_color);
        for case in &manifest.cases {
            let style =
                crate::settings::Settings::from_value(&serde_json::json!({ "editor": case.style }))
                    .editor;
            let ours = image::load_from_memory(&export_png(&source, &style, inset).unwrap())
                .unwrap()
                .into_rgba8();
            let reference = image::load_from_memory(&fixture(&case.file))
                .unwrap()
                .into_rgba8();
            assert_eq!(ours.dimensions(), reference.dimensions(), "{}", case.file);
            let (mean, worst) = difference(&ours, &reference);
            eprintln!("{}: mean {mean:.3}, max {worst}", case.file);
            assert!(
                mean < 0.5 && worst <= 40,
                "{}: mean {mean:.3}, max {worst}",
                case.file
            );
        }
    }
}
