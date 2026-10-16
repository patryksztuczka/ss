//! Dragging a capture file out to other apps (the preview's `startDrag`),
//! after HEX's `HexDragView`. GPUI has no external drag source, so this
//! drives AppKit directly.
//!
//! The session is started from the next run-loop turn rather than inside
//! GPUI's mouse handler: AppKit sends window and view callbacks while a drag
//! begins, and GPUI must not be mid-dispatch when they arrive. The original
//! mouse-down `NSEvent` is retained for `beginDraggingSession`.

use std::cell::RefCell;
use std::path::Path;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession, NSDraggingSource,
    NSEvent, NSImage, NSPasteboardWriting, NSView,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSData, NSObject, NSPoint, NSRect, NSSize, NSString, NSURL,
};

struct PendingDrag {
    view: Retained<NSView>,
    event: Retained<NSEvent>,
    file: Retained<NSURL>,
    image: Retained<NSImage>,
}

#[derive(Default)]
struct SourceIvars {
    pending: RefCell<Option<PendingDrag>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = SourceIvars]
    struct FileDragSource;

    unsafe impl NSObjectProtocol for FileDragSource {}

    unsafe impl NSDraggingSource for FileDragSource {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }
    }

    impl FileDragSource {
        #[unsafe(method(startPendingDrag:))]
        fn start_pending_drag(&self, _sender: Option<&AnyObject>) {
            if let Some(pending) = self.ivars().pending.borrow_mut().take() {
                self.begin(pending);
            }
        }
    }
);

impl FileDragSource {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(SourceIvars::default());
        unsafe { msg_send![super(this), init] }
    }

    fn begin(&self, pending: PendingDrag) {
        let PendingDrag {
            view,
            event,
            file,
            image,
        } = pending;
        let writer = ProtocolObject::<dyn NSPasteboardWriting>::from_ref(&*file);
        let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);
        // Centre the drag image under the pointer, in the view's coordinates.
        let location = view.convertPoint_fromView(event.locationInWindow(), None);
        let size = image.size();
        let frame = NSRect::new(
            NSPoint::new(
                location.x - size.width / 2.0,
                location.y - size.height / 2.0,
            ),
            size,
        );
        // SAFETY: the contents are an NSImage, as the API expects.
        unsafe { item.setDraggingFrame_contents(frame, Some(&image)) };
        let session = view.beginDraggingSessionWithItems_event_source(
            &NSArray::from_retained_slice(&[item]),
            &event,
            ProtocolObject::from_ref(self),
        );
        session.setAnimatesToStartingPositionsOnCancelOrFail(true);
        tracing::debug!("started a file drag");
    }
}

thread_local! {
    static SOURCE: RefCell<Option<Retained<FileDragSource>>> = const { RefCell::new(None) };
}

/// Starts dragging `file` from `view` on the next run-loop turn. `event` is
/// the mouse-down that began the gesture; `png` becomes the drag image at
/// its pixel size in points, like Electron's `startDrag` icon.
pub fn start(view: Retained<NSView>, event: Retained<NSEvent>, file: &Path, png: &[u8]) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(png)) else {
        tracing::warn!("drag image is not a valid PNG");
        return;
    };
    if let Some((width, height)) = crate::capture_store::png_size(png) {
        image.setSize(NSSize::new(f64::from(width), f64::from(height)));
    }
    let file = NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
    let source = SOURCE.with(|source| {
        source
            .borrow_mut()
            .get_or_insert_with(|| FileDragSource::new(mtm))
            .clone()
    });
    source.ivars().pending.replace(Some(PendingDrag {
        view,
        event,
        file,
        image,
    }));
    let none: Option<&AnyObject> = None;
    // SAFETY: `startPendingDrag:` takes one object argument.
    let _: () = unsafe {
        msg_send![&*source, performSelector: sel!(startPendingDrag:), withObject: none, afterDelay: 0.0f64]
    };
}
