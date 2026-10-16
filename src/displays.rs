//! Displays in Electron's coordinate space: top-left origin at the primary
//! display, y growing downward, in points. `screencapture -R` and Electron's
//! `display.bounds` / `workArea` use this space; Cocoa uses bottom-left.

use color_eyre::eyre::{Result, eyre};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.x + self.width && y >= self.y && y < self.y + self.height
    }

    fn distance_squared(&self, x: f64, y: f64) -> f64 {
        let dx = (self.x - x).max(0.0).max(x - (self.x + self.width));
        let dy = (self.y - y).max(0.0).max(y - (self.y + self.height));
        dx * dx + dy * dy
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Display {
    pub bounds: Rect,
    /// Bounds minus the menu bar and Dock.
    pub work_area: Rect,
    /// The CGDirectDisplayID, which is also GPUI's `DisplayId`.
    pub number: u32,
}

/// A screen as Cocoa reports it: `frame` and `visibleFrame` in bottom-left
/// global coordinates, where the primary screen's origin is (0, 0).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CocoaScreen {
    pub frame: Rect,
    pub visible_frame: Rect,
    pub number: u32,
}

/// Flips a Cocoa rect into top-left global points, and back (it is its own inverse).
pub fn flip(rect: Rect, primary_height: f64) -> Rect {
    Rect {
        y: primary_height - (rect.y + rect.height),
        ..rect
    }
}

/// Converts Cocoa screens (the first is the primary) to [`Display`]s.
pub fn from_cocoa(screens: &[CocoaScreen]) -> Vec<Display> {
    let Some(primary) = screens.first() else {
        return Vec::new();
    };
    let height = primary.frame.height;
    screens
        .iter()
        .map(|screen| Display {
            bounds: flip(screen.frame, height),
            work_area: flip(screen.visible_frame, height),
            number: screen.number,
        })
        .collect()
}

/// `screen.getDisplayNearestPoint`: the display containing the point, else
/// the closest one. The point is in top-left global points.
pub fn nearest(displays: &[Display], x: f64, y: f64) -> Option<Display> {
    displays
        .iter()
        .find(|display| display.bounds.contains(x, y))
        .or_else(|| {
            displays.iter().min_by(|a, b| {
                a.bounds
                    .distance_squared(x, y)
                    .total_cmp(&b.bounds.distance_squared(x, y))
            })
        })
        .copied()
}

/// All displays, primary first. Must run on the main thread.
#[cfg(target_os = "macos")]
pub fn all() -> Result<Vec<Display>> {
    Ok(from_cocoa(&cocoa_screens()?))
}

/// The display under the mouse pointer. Must run on the main thread.
#[cfg(target_os = "macos")]
pub fn under_cursor() -> Result<Display> {
    let screens = cocoa_screens()?;
    let displays = from_cocoa(&screens);
    let primary_height = screens[0].frame.height;
    let mouse = objc2_app_kit::NSEvent::mouseLocation();
    nearest(&displays, mouse.x, primary_height - mouse.y)
        .ok_or_else(|| eyre!("no displays are connected"))
}

/// Height of the primary display, the axis Cocoa and top-left coordinates
/// flip around. Must run on the main thread.
#[cfg(target_os = "macos")]
pub fn primary_height() -> Result<f64> {
    Ok(cocoa_screens()?[0].frame.height)
}

/// `deviceDescription[NSScreenNumber]`, the screen's CGDirectDisplayID.
#[cfg(target_os = "macos")]
fn screen_number(screen: &objc2_app_kit::NSScreen) -> u32 {
    use objc2_foundation::{NSNumber, NSString};
    screen
        .deviceDescription()
        .objectForKey(&NSString::from_str("NSScreenNumber"))
        .and_then(|value| value.downcast::<NSNumber>().ok())
        .map_or(0, |number| number.unsignedIntValue())
}

#[cfg(target_os = "macos")]
fn cocoa_screens() -> Result<Vec<CocoaScreen>> {
    use objc2_app_kit::NSScreen;
    use objc2_foundation::{MainThreadMarker, NSRect};

    let marker = MainThreadMarker::new().ok_or_else(|| eyre!("displays need the main thread"))?;
    let rect = |frame: NSRect| {
        Rect::new(
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        )
    };
    let screens: Vec<CocoaScreen> = NSScreen::screens(marker)
        .iter()
        .map(|screen| CocoaScreen {
            frame: rect(screen.frame()),
            visible_frame: rect(screen.visibleFrame()),
            number: screen_number(&screen),
        })
        .collect();
    if screens.is_empty() {
        return Err(eyre!("no displays are connected"));
    }
    Ok(screens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(frame: Rect, visible: Rect) -> CocoaScreen {
        CocoaScreen {
            frame,
            visible_frame: visible,
            number: 1,
        }
    }

    // A 1512×982 laptop with a 24 pt menu bar and a 70 pt Dock at the bottom.
    fn laptop() -> CocoaScreen {
        screen(
            Rect::new(0.0, 0.0, 1512.0, 982.0),
            Rect::new(0.0, 70.0, 1512.0, 888.0),
        )
    }

    #[test]
    fn primary_display_keeps_its_origin() {
        let displays = from_cocoa(&[laptop()]);
        assert_eq!(displays[0].bounds, Rect::new(0.0, 0.0, 1512.0, 982.0));
        // The menu bar is at the top, the Dock at the bottom.
        assert_eq!(displays[0].work_area, Rect::new(0.0, 24.0, 1512.0, 888.0));
    }

    #[test]
    fn display_above_the_primary_has_a_negative_y() {
        let above = screen(
            Rect::new(-200.0, 982.0, 2560.0, 1440.0),
            Rect::new(-200.0, 982.0, 2560.0, 1415.0),
        );
        let displays = from_cocoa(&[laptop(), above]);
        assert_eq!(
            displays[1].bounds,
            Rect::new(-200.0, -1440.0, 2560.0, 1440.0)
        );
        assert_eq!(
            displays[1].work_area,
            Rect::new(-200.0, -1415.0, 2560.0, 1415.0)
        );
    }

    #[test]
    fn display_left_of_the_primary_has_a_negative_x() {
        // Bottom-aligned 1920×1080 to the left: Cocoa y = 0, top-left y = 982 - 1080.
        let left = screen(
            Rect::new(-1920.0, 0.0, 1920.0, 1080.0),
            Rect::new(-1920.0, 0.0, 1920.0, 1055.0),
        );
        let displays = from_cocoa(&[laptop(), left]);
        assert_eq!(
            displays[1].bounds,
            Rect::new(-1920.0, -98.0, 1920.0, 1080.0)
        );
        assert_eq!(
            displays[1].work_area,
            Rect::new(-1920.0, -73.0, 1920.0, 1055.0)
        );
    }

    #[test]
    fn display_below_the_primary_starts_after_it() {
        let below = screen(
            Rect::new(100.0, -1080.0, 1920.0, 1080.0),
            Rect::new(100.0, -1080.0, 1920.0, 1080.0),
        );
        let displays = from_cocoa(&[laptop(), below]);
        assert_eq!(displays[1].bounds, Rect::new(100.0, 982.0, 1920.0, 1080.0));
    }

    #[test]
    fn nearest_prefers_the_containing_display() {
        let left = screen(
            Rect::new(-1920.0, 0.0, 1920.0, 1080.0),
            Rect::new(-1920.0, 0.0, 1920.0, 1080.0),
        );
        let displays = from_cocoa(&[laptop(), left]);
        assert_eq!(nearest(&displays, 10.0, 10.0), Some(displays[0]));
        assert_eq!(nearest(&displays, -10.0, 10.0), Some(displays[1]));
        // The top-left corner belongs to the display it starts.
        assert_eq!(nearest(&displays, 0.0, 0.0), Some(displays[0]));
        // In the gap below the laptop and right of the external display.
        assert_eq!(nearest(&displays, 5.0, 1000.0), Some(displays[0]));
        assert_eq!(nearest(&displays, -5.0, 1000.0), Some(displays[1]));
        assert_eq!(nearest(&[], 0.0, 0.0), None);
    }
}
