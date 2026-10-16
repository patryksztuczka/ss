//! The system colour picker for the editor's Custom background (replaces
//! `<input type="color">`, which Chromium also backs with NSColorPanel).

use std::cell::RefCell;

use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{NSColor, NSColorPanel, NSColorSpace};
use objc2_foundation::{MainThreadMarker, NSObject};

#[derive(Debug)]
struct TargetIvars {
    colors: UnboundedSender<String>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetIvars]
    struct ColorTarget;

    unsafe impl NSObjectProtocol for ColorTarget {}

    impl ColorTarget {
        #[unsafe(method(colorChanged:))]
        fn color_changed(&self, panel: &NSColorPanel) {
            if let Some(hex) = to_hex(&panel.color()) {
                let _ = self.ivars().colors.unbounded_send(hex);
            }
        }
    }
);

impl ColorTarget {
    fn new(colors: UnboundedSender<String>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars { colors });
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    // The panel holds its target weakly.
    static TARGET: RefCell<Option<Retained<ColorTarget>>> = const { RefCell::new(None) };
}

/// Shows the shared colour panel at `initial` (`#rrggbb`). Continuous
/// changes arrive on the receiver as sRGB `#rrggbb`; opening it again for
/// another editor ends the previous stream.
pub fn open(initial: [u8; 3]) -> Option<UnboundedReceiver<String>> {
    let mtm = MainThreadMarker::new()?;
    let panel = NSColorPanel::sharedColorPanel(mtm);
    panel.setShowsAlpha(false);
    panel.setContinuous(true);
    let [r, g, b] = initial.map(|channel| f64::from(channel) / 255.0);
    // Set before the new target so the initial colour is not echoed back.
    // SAFETY: clearing the target never dangles.
    unsafe { panel.setTarget(None) };
    panel.setColor(&NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0));
    let (sender, receiver) = unbounded();
    let target = ColorTarget::new(sender, mtm);
    unsafe {
        panel.setTarget(Some(&target));
        panel.setAction(Some(sel!(colorChanged:)));
    }
    TARGET.with(|slot| slot.replace(Some(target)));
    panel.orderFront(None);
    Some(receiver)
}

fn to_hex(color: &NSColor) -> Option<String> {
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channel(srgb.redComponent()),
        channel(srgb.greenComponent()),
        channel(srgb.blueComponent())
    ))
}
