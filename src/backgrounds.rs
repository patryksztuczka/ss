//! Editor backgrounds (port of `renderer/src/lib/backgrounds.ts`): one spec
//! paints both the export and the sidebar swatches, so they always match.

use tiny_skia::{
    Color, GradientStop, LinearGradient, Paint, Pixmap, Point, RadialGradient, Rect, Shader,
    SpreadMode, Transform,
};

use crate::settings::BackgroundId;

/// A soft radial glow; position and radius are relative to the canvas.
#[derive(Clone, Copy, Debug)]
pub struct Blob {
    pub x: f64,
    pub y: f64,
    pub r: f64,
    pub color: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum Spec {
    Gradient {
        angle: f64,
        stops: &'static [u32],
        blobs: &'static [Blob],
    },
    Transparent,
    Custom,
}

#[derive(Clone, Copy, Debug)]
pub struct Background {
    pub id: BackgroundId,
    pub label: &'static str,
    pub spec: Spec,
}

const fn blob(x: f64, y: f64, r: f64, color: u32) -> Blob {
    Blob { x, y, r, color }
}

pub const BACKGROUNDS: [Background; 10] = [
    Background {
        id: BackgroundId::Desktop,
        label: "Desktop",
        spec: Spec::Gradient {
            angle: 160.0,
            stops: &[0xffb347, 0xff7a2f, 0xe5482c],
            blobs: &[
                blob(0.25, 0.2, 0.55, 0xffe08a),
                blob(0.85, 0.85, 0.6, 0xc92f2f),
                blob(0.65, 0.35, 0.35, 0xffc06b),
            ],
        },
    },
    Background {
        id: BackgroundId::Cool,
        label: "Cool",
        spec: Spec::Gradient {
            angle: 160.0,
            stops: &[0x5aa7f5, 0x3d7fe0, 0x4c9ae8],
            blobs: &[blob(0.15, 0.1, 0.6, 0x7fd0ff)],
        },
    },
    Background {
        id: BackgroundId::Nice,
        label: "Nice",
        spec: Spec::Gradient {
            angle: 140.0,
            stops: &[0xc44f80, 0xd64a74, 0xb04a8a],
            blobs: &[blob(0.85, 0.15, 0.5, 0xe66a8c)],
        },
    },
    Background {
        id: BackgroundId::Morning,
        label: "Morning",
        spec: Spec::Gradient {
            angle: 140.0,
            stops: &[0xf5a983, 0xee8f73, 0xf4b78d],
            blobs: &[blob(0.2, 0.85, 0.5, 0xffc9a3)],
        },
    },
    Background {
        id: BackgroundId::Bright,
        label: "Bright",
        spec: Spec::Gradient {
            angle: 135.0,
            stops: &[0x9a72f2, 0x6f7ff4, 0xb479ef],
            blobs: &[blob(0.9, 0.1, 0.5, 0x8fb2ff)],
        },
    },
    Background {
        id: BackgroundId::Love,
        label: "Love",
        spec: Spec::Gradient {
            angle: 150.0,
            stops: &[0x3b1d8f, 0x6a2bd1, 0x9a3fe0],
            blobs: &[blob(0.85, 0.9, 0.6, 0xc54ad6)],
        },
    },
    Background {
        id: BackgroundId::Rain,
        label: "Rain",
        spec: Spec::Gradient {
            angle: 135.0,
            stops: &[0xf7a8c8, 0xf39ab8],
            blobs: &[
                blob(0.1, 0.15, 0.6, 0x6cb8ff),
                blob(0.9, 0.2, 0.55, 0xff6f96),
                blob(0.55, 0.95, 0.6, 0xffa36e),
                blob(0.15, 0.85, 0.45, 0xa08dff),
            ],
        },
    },
    Background {
        id: BackgroundId::Sky,
        label: "Sky",
        spec: Spec::Gradient {
            angle: 135.0,
            stops: &[0xb8d3ff, 0xc3c5ff],
            blobs: &[
                blob(0.05, 0.05, 0.65, 0x8fe6f8),
                blob(0.95, 0.1, 0.55, 0xc8b8ff),
                blob(0.05, 0.95, 0.6, 0x9a7cf2),
                blob(0.95, 0.95, 0.6, 0x7fcaf6),
                blob(0.5, 0.5, 0.45, 0xd6e4ff),
            ],
        },
    },
    Background {
        id: BackgroundId::None,
        label: "None",
        spec: Spec::Transparent,
    },
    Background {
        id: BackgroundId::Custom,
        label: "Custom",
        spec: Spec::Custom,
    },
];

pub fn by_id(id: BackgroundId) -> &'static Background {
    BACKGROUNDS
        .iter()
        .find(|background| background.id == id)
        .unwrap_or(&BACKGROUNDS[0])
}

/// A CSS colour as canvas `fillStyle` would take it: `#rgb` or `#rrggbb`.
/// Anything else is ignored by canvas, which leaves the default black.
pub fn parse_color(text: &str) -> [u8; 3] {
    let hex = text.trim().strip_prefix('#').unwrap_or("");
    let digits: Option<Vec<u8>> = hex
        .chars()
        .map(|char| char.to_digit(16).map(|digit| digit as u8))
        .collect();
    match digits.as_deref() {
        Some([r, g, b]) => [r * 17, g * 17, b * 17],
        Some([r1, r0, g1, g0, b1, b0]) => [r1 * 16 + r0, g1 * 16 + g0, b1 * 16 + b0],
        _ => [0, 0, 0],
    }
}

fn color(hex: u32, alpha: f32) -> Color {
    Color::from_rgba8(
        ((hex >> 16) & 255) as u8,
        ((hex >> 8) & 255) as u8,
        (hex & 255) as u8,
        (alpha * 255.0).round() as u8,
    )
}

/// `paintBackground`: fills the whole pixmap.
pub fn paint(pixmap: &mut Pixmap, background: &Background, custom_color: &str) {
    pixmap.fill(Color::TRANSPARENT);
    let (width, height) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
    let Some(area) = Rect::from_xywh(0.0, 0.0, width as f32, height as f32) else {
        return;
    };
    let fill = |pixmap: &mut Pixmap, shader: Shader| {
        let paint = Paint {
            shader,
            anti_alias: false,
            ..Paint::default()
        };
        pixmap.fill_rect(area, &paint, Transform::identity(), None);
    };
    let (angle, stops, blobs) = match background.spec {
        Spec::Transparent => return,
        Spec::Custom => {
            let [r, g, b] = parse_color(custom_color);
            return fill(pixmap, Shader::SolidColor(Color::from_rgba8(r, g, b, 255)));
        }
        Spec::Gradient {
            angle,
            stops,
            blobs,
        } => (angle, stops, blobs),
    };

    // CSS angle semantics: 0deg points up, increasing clockwise.
    let radians = (angle - 90.0).to_radians();
    let half = ((width * radians.cos()).abs() + (height * radians.sin()).abs()) / 2.0;
    let (cx, cy) = (width / 2.0, height / 2.0);
    let point = |x: f64, y: f64| Point::from_xy(x as f32, y as f32);
    let gradient_stops = stops
        .iter()
        .enumerate()
        .map(|(index, &hex)| {
            let position = index as f32 / (stops.len().max(2) - 1) as f32;
            GradientStop::new(position, color(hex, 1.0))
        })
        .collect();
    if let Some(shader) = LinearGradient::new(
        point(cx - radians.cos() * half, cy - radians.sin() * half),
        point(cx + radians.cos() * half, cy + radians.sin() * half),
        gradient_stops,
        SpreadMode::Pad,
        Transform::identity(),
    ) {
        fill(pixmap, shader);
    }

    // CSS `circle` percentages resolve against the diagonal / √2.
    let reference = width.hypot(height) / std::f64::consts::SQRT_2;
    for blob in blobs {
        let center = point(blob.x * width, blob.y * height);
        if let Some(shader) = RadialGradient::new(
            center,
            center,
            (blob.r * reference) as f32,
            vec![
                GradientStop::new(0.0, color(blob.color, 1.0)),
                GradientStop::new(1.0, color(blob.color, 0.0)),
            ],
            SpreadMode::Pad,
            Transform::identity(),
        ) {
            fill(pixmap, shader);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_has_a_background_in_typescript_order() {
        let labels: Vec<_> = BACKGROUNDS
            .iter()
            .map(|background| background.label)
            .collect();
        assert_eq!(
            labels,
            [
                "Desktop", "Cool", "Nice", "Morning", "Bright", "Love", "Rain", "Sky", "None",
                "Custom"
            ]
        );
        assert_eq!(by_id(BackgroundId::Rain).label, "Rain");
    }

    #[test]
    fn parses_css_hex_colours_like_canvas() {
        assert_eq!(parse_color("#da8216"), [0xda, 0x82, 0x16]);
        assert_eq!(parse_color("#fff"), [255, 255, 255]);
        assert_eq!(parse_color("red"), [0, 0, 0]);
        assert_eq!(parse_color("#12345"), [0, 0, 0]);
    }

    #[test]
    fn none_is_transparent_and_custom_is_flat() {
        let mut pixmap = Pixmap::new(8, 8).unwrap();
        paint(&mut pixmap, by_id(BackgroundId::None), "#000");
        assert!(pixmap.pixels().iter().all(|pixel| pixel.alpha() == 0));
        paint(&mut pixmap, by_id(BackgroundId::Custom), "#da8216");
        let pixel = pixmap.pixel(3, 3).unwrap();
        assert_eq!(
            (pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()),
            (0xda, 0x82, 0x16, 255)
        );
    }

    #[test]
    fn gradients_follow_the_css_angle() {
        // 135° runs from the top-left (first stop) to the bottom-right (last stop).
        let mut pixmap = Pixmap::new(100, 100).unwrap();
        let background = Background {
            id: BackgroundId::Sky,
            label: "",
            spec: Spec::Gradient {
                angle: 135.0,
                stops: &[0xff0000, 0x0000ff],
                blobs: &[],
            },
        };
        paint(&mut pixmap, &background, "");
        let top_left = pixmap.pixel(0, 0).unwrap();
        let bottom_right = pixmap.pixel(99, 99).unwrap();
        assert!(top_left.red() > 245 && top_left.blue() < 10, "{top_left:?}");
        assert!(
            bottom_right.blue() > 245 && bottom_right.red() < 10,
            "{bottom_right:?}"
        );
    }
}
