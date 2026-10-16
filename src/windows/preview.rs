//! The floating capture preview (port of `Preview.tsx` and the preview half
//! of `Windows.ts`): a card in the bottom-right corner that slides in, hides
//! itself after 6 s unless hovered, and lets the capture be edited, copied
//! as text, revealed or dragged out.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use color_eyre::eyre::{Result, eyre};
use gpui::{
    Animation, AnimationExt, AnyElement, App, AppContext, AsyncApp, Bounds, BoxShadow, Context,
    Div, FontWeight, Global, ImageSource, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    ObjectFit, Pixels, Point, Render, SharedString, Task, WeakEntity, Window,
    WindowBackgroundAppearance, WindowBounds, WindowHandle, WindowKind, WindowOptions, div, img,
    point, prelude::*, px, rgb, size,
};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSFloatingWindowLevel, NSView, NSWindow, NSWindowCollectionBehavior,
};
use objc2_foundation::{MainThreadMarker, NSPoint, NSRect, NSSize};

use crate::capture_store::CaptureKind;
use crate::desktop_ui::{
    FADE_IN, INK, INK_SOFT, RADIUS_LG, RADIUS_PREVIEW, SLIDE_IN, SLIDE_OUT, TEXT_2XS, TEXT_HINT,
    WHITE, alpha, ease_out_soft, ease_spring,
};
use crate::displays::{self, Rect};
use crate::thumbnail::Thumbnail;

pub const CARD_WIDTH: f64 = 248.0;
/// Transparent gutter for the drop shadow and the slide animation.
const MARGIN: f64 = 18.0;
const SCREEN_INSET: f64 = 6.0;
const VISIBLE_FOR: Duration = Duration::from_millis(6000);
const DRAG_THRESHOLD: f32 = 3.0;
const HOVER_TRANSITION: Duration = Duration::from_millis(300);
const SCALE_TRANSITION: Duration = Duration::from_millis(500);

#[derive(Clone)]
pub struct PreviewPayload {
    pub id: String,
    pub kind: CaptureKind,
    pub path: PathBuf,
    /// Size of the capture: pixels for images, points for recordings.
    pub width: u32,
    pub height: u32,
    pub thumbnail: Thumbnail,
}

/// `round(clamp(248 · h / w, 120, 220))`.
pub fn thumbnail_height(width: u32, height: u32) -> f64 {
    let aspect = f64::from(height) / f64::from(width.max(1));
    (CARD_WIDTH * aspect).clamp(120.0, 220.0).round()
}

/// The window: the card plus its gutter, 6 pt from the bottom-right corner
/// of the work area. Top-left global points.
pub fn window_frame(work_area: Rect, width: u32, height: u32) -> Rect {
    let window_width = CARD_WIDTH + MARGIN * 2.0;
    let window_height = thumbnail_height(width, height) + MARGIN * 2.0;
    Rect::new(
        work_area.x + work_area.width - window_width - SCREEN_INSET,
        work_area.y + work_area.height - window_height - SCREEN_INSET,
        window_width,
        window_height,
    )
}

/// What the preview's buttons do; tests and `ss preview` substitute it.
pub trait PreviewHost {
    fn open_editor(&self, id: &str, cx: &mut App);
    fn reveal(&self, path: &Path);
    /// Copies the recognised text; resolves to the number of lines.
    fn copy_text(&self, id: &str, cx: &mut App) -> Task<Result<usize, SharedString>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    In,
    Out,
    Hidden,
}

/// A press on the thumbnail that may turn into a drag.
struct Press {
    position: Point<Pixels>,
    event: Option<Retained<objc2_app_kit::NSEvent>>,
}

pub struct PreviewView {
    host: Rc<dyn PreviewHost>,
    payload: Option<PreviewPayload>,
    phase: Phase,
    note: SharedString,
    hovered: bool,
    /// Bumped on every hover change so hover transitions restart.
    hover_epoch: usize,
    /// Bumped on every new payload so animations and timers restart.
    generation: usize,
    auto_hide: Option<Task<()>>,
    hide: Option<Task<()>>,
    press: Option<Press>,
}

impl PreviewView {
    pub fn new(host: Rc<dyn PreviewHost>) -> Self {
        Self {
            host,
            payload: None,
            phase: Phase::Hidden,
            note: "Copied".into(),
            hovered: false,
            hover_epoch: 0,
            generation: 0,
            auto_hide: None,
            hide: None,
            press: None,
        }
    }

    pub fn show(&mut self, payload: PreviewPayload, cx: &mut Context<Self>) {
        self.generation += 1;
        self.note = match payload.kind {
            CaptureKind::Video => "Recording copied".into(),
            CaptureKind::Image => "Copied".into(),
        };
        self.payload = Some(payload);
        self.phase = Phase::In;
        self.hovered = false;
        self.hover_epoch = 0;
        self.hide = None;
        self.press = None;
        self.restart_auto_hide(cx);
        cx.notify();
    }

    /// Slides out, then hides the window.
    pub fn dismiss(&mut self, cx: &mut Context<Self>) {
        if self.phase != Phase::In {
            return;
        }
        self.phase = Phase::Out;
        self.auto_hide = None;
        let generation = self.generation;
        self.hide = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor().timer(SLIDE_OUT).await;
                let _ = this.update(cx, |this, cx| {
                    if this.generation == generation {
                        this.set_hidden(cx);
                        order_out(cx);
                    }
                });
            }),
        );
        cx.notify();
    }

    /// Hidden immediately, e.g. when a new capture starts.
    pub fn set_hidden(&mut self, cx: &mut Context<Self>) {
        self.phase = Phase::Hidden;
        self.auto_hide = None;
        self.hide = None;
        self.press = None;
        cx.notify();
    }

    /// `ss preview preview --chip`.
    pub fn set_note(&mut self, note: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.note = note.into();
        cx.notify();
    }

    pub fn set_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.hovered == hovered {
            return;
        }
        self.hovered = hovered;
        self.hover_epoch += 1;
        self.restart_auto_hide(cx);
        cx.notify();
    }

    /// Auto-hide unless the pointer is resting on the card.
    fn restart_auto_hide(&mut self, cx: &mut Context<Self>) {
        if self.payload.is_none() || self.hovered || self.phase != Phase::In {
            self.auto_hide = None;
            return;
        }
        let generation = self.generation;
        self.auto_hide = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor().timer(VISIBLE_FOR).await;
                let _ = this.update(cx, |this, cx| {
                    if this.generation == generation {
                        this.dismiss(cx);
                    }
                });
            }),
        );
    }

    fn copy_text(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.payload.as_ref().map(|payload| payload.id.clone()) else {
            return;
        };
        let task = self.host.copy_text(&id, cx);
        let generation = self.generation;
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| match result {
                Ok(lines) if this.generation == generation => {
                    this.note = if lines > 0 {
                        "Text copied".into()
                    } else {
                        "No text found".into()
                    };
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => tracing::error!(%error, "could not copy text"),
            });
        })
        .detach();
    }

    fn open(&mut self, cx: &mut Context<Self>) {
        let Some(payload) = self.payload.clone() else {
            return;
        };
        match payload.kind {
            CaptureKind::Image => self.host.open_editor(&payload.id, cx),
            CaptureKind::Video => self.host.reveal(&payload.path),
        }
        self.dismiss(cx);
    }

    fn reveal(&mut self, cx: &mut Context<Self>) {
        if let Some(payload) = &self.payload {
            self.host.reveal(&payload.path);
        }
        self.dismiss(cx);
    }

    fn press(&mut self, event: &MouseDownEvent) {
        let current = MainThreadMarker::new()
            .and_then(|mtm| NSApplication::sharedApplication(mtm).currentEvent());
        self.press = Some(Press {
            position: event.position,
            event: current,
        });
    }

    /// Past the threshold, hands the gesture to an AppKit drag session.
    fn drag(&mut self, event: &MouseMoveEvent, window: &mut Window) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.press = None;
            return;
        }
        let Some(press) = &self.press else {
            return;
        };
        let delta = event.position - press.position;
        let distance = (f32::from(delta.x).powi(2) + f32::from(delta.y).powi(2)).sqrt();
        if distance < DRAG_THRESHOLD {
            return;
        }
        let Some(Press {
            event: Some(down), ..
        }) = self.press.take()
        else {
            return;
        };
        let (Some(payload), Some(view)) = (&self.payload, ns_view(window)) else {
            return;
        };
        crate::drag::start(view, down, &payload.path, &payload.thumbnail.drag_png);
    }

    fn render_card(&self, payload: &PreviewPayload, cx: &mut Context<Self>) -> AnyElement {
        let card_height = thumbnail_height(payload.width, payload.height) as f32;
        let is_image = payload.kind == CaptureKind::Image;
        let hovered = self.hovered;
        let epoch = self.hover_epoch;

        let thumbnail = div()
            .id("thumbnail")
            .absolute()
            .child(
                img(ImageSource::Render(payload.thumbnail.image.clone()))
                    .size_full()
                    .object_fit(ObjectFit::Cover),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, _| this.press(event)),
            )
            .on_mouse_move(
                cx.listener(|this, event: &MouseMoveEvent, window, _| this.drag(event, window)),
            )
            .on_click(cx.listener(|this, _, _, cx| this.open(cx)));
        // `group-hover:scale-[1.02]`, as a slightly larger centred box.
        let scale_box = move |element: gpui::Stateful<Div>, scale: f32| {
            let width = CARD_WIDTH as f32 * scale;
            let height = card_height * scale;
            element
                .left(px((CARD_WIDTH as f32 - width) / 2.0))
                .top(px((card_height - height) / 2.0))
                .w(px(width))
                .h(px(height))
        };
        let thumbnail = if epoch == 0 {
            scale_box(thumbnail, 1.0).into_any_element()
        } else {
            let (from, to) = if hovered { (1.0, 1.02) } else { (1.02, 1.0) };
            thumbnail
                .with_animation(
                    ("thumbnail-scale", epoch),
                    Animation::new(SCALE_TRANSITION).with_easing(ease_out_soft()),
                    move |element, delta| scale_box(element, from + (to - from) * delta),
                )
                .into_any_element()
        };

        let chip = div()
            .absolute()
            .top_2()
            .left_2()
            .flex()
            .items_center()
            .gap_1()
            .rounded_full()
            // More opaque than Electron's /55 + backdrop blur (plan §7.6).
            .bg(alpha(0x000000, 0.65))
            .px_2()
            .py(px(2.0))
            .text_size(px(TEXT_2XS))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(WHITE))
            .child(
                gpui_symbols::Icon::new("checkmark")
                    .size(px(9.0))
                    .color(rgb(WHITE))
                    .weight(gpui_symbols::SymbolWeight::Bold),
            )
            .child(self.note.clone())
            .with_animation(
                ("chip", self.generation),
                Animation::new(FADE_IN).with_easing(ease_out_soft()),
                |chip, delta| chip.opacity(delta),
            );

        let play = (!is_image).then(|| {
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    div()
                        .size(px(40.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(alpha(0x000000, 0.55))
                        .child(
                            gpui_symbols::Icon::new("play.fill")
                                .size(px(15.0))
                                .color(rgb(WHITE)),
                        ),
                )
        });

        let hover_controls = hovered.then(|| {
            let close = div()
                .id("dismiss")
                .debug_selector(|| "preview-dismiss".into())
                .absolute()
                .top_2()
                .right_2()
                .size(px(24.0))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(alpha(WHITE, 0.96))
                .shadow(vec![shadow(0.0, 1.0, 3.0, 0.0, 0.1)])
                .child(
                    gpui_symbols::Icon::new("xmark")
                        .size(px(9.0))
                        .color(rgb(INK_SOFT))
                        .weight(gpui_symbols::SymbolWeight::Bold),
                )
                .on_click(cx.listener(|this, _, _, cx| this.dismiss(cx)));
            let actions = div()
                .absolute()
                .left_2()
                .right_2()
                .bottom_2()
                .flex()
                .gap(px(6.0))
                .map(|bar| {
                    if is_image {
                        bar.child(
                            action_button("edit", "Edit")
                                .on_click(cx.listener(|this, _, _, cx| this.open(cx))),
                        )
                        .child(
                            action_button("copy-text", "Copy text")
                                .on_click(cx.listener(|this, _, _, cx| this.copy_text(cx))),
                        )
                    } else {
                        bar.child(
                            action_button("show-in-finder", "Show in Finder")
                                .on_click(cx.listener(|this, _, _, cx| this.reveal(cx))),
                        )
                    }
                })
                .with_animation(
                    ("actions", epoch),
                    Animation::new(HOVER_TRANSITION).with_easing(ease_out_soft()),
                    |bar, delta| bar.opacity(delta).bottom(px(8.0 - 8.0 * (1.0 - delta))),
                );
            vec![
                close
                    .with_animation(
                        ("dismiss", epoch),
                        Animation::new(Duration::from_millis(200)),
                        |close, delta| close.opacity(delta),
                    )
                    .into_any_element(),
                actions.into_any_element(),
            ]
        });

        let card = div()
            .id("preview-card")
            .relative()
            .size_full()
            .overflow_hidden()
            .rounded(px(RADIUS_PREVIEW))
            .bg(rgb(WHITE))
            .shadow(vec![
                shadow(0.0, 0.0, 0.0, 0.5, 0.14),
                BoxShadow {
                    color: gpui::rgba(0x14142859).into(),
                    offset: point(px(0.0), px(10.0)),
                    blur_radius: px(30.0),
                    spread_radius: px(-6.0),
                },
                shadow(0.0, 2.0, 6.0, 0.0, 0.08),
            ])
            .on_hover(cx.listener(|this, hovered: &bool, _, cx| this.set_hovered(*hovered, cx)))
            .child(thumbnail)
            .child(chip)
            .children(play)
            .children(hover_controls.into_iter().flatten());

        let card_width = CARD_WIDTH as f32;
        match self.phase {
            // `slide-in`: 520 ms spring from translateX(110%) with a fade.
            Phase::In => card
                .with_animation(
                    ("slide-in", self.generation),
                    Animation::new(SLIDE_IN),
                    move |card, delta| {
                        let progress = ease_spring()(delta);
                        card.left(px(card_width * 1.1 * (1.0 - progress)))
                            .opacity(progress.clamp(0.0, 1.0))
                    },
                )
                .into_any_element(),
            // `slide-out`: 260 ms to translateX(60%) while fading.
            Phase::Out => card
                .with_animation(
                    ("slide-out", self.generation),
                    Animation::new(SLIDE_OUT).with_easing(ease_out_soft()),
                    move |card, delta| card.left(px(card_width * 0.6 * delta)).opacity(1.0 - delta),
                )
                .into_any_element(),
            Phase::Hidden => div().into_any_element(),
        }
    }
}

fn shadow(x: f32, y: f32, blur: f32, spread: f32, opacity: f32) -> BoxShadow {
    BoxShadow {
        color: alpha(0x000000, opacity).into(),
        offset: point(px(x), px(y)),
        blur_radius: px(blur),
        spread_radius: px(spread),
    }
}

fn action_button(id: &'static str, label: &'static str) -> gpui::Stateful<Div> {
    div()
        .id(id)
        .debug_selector(move || format!("preview-{id}"))
        .h(px(28.0))
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(RADIUS_LG))
        .bg(alpha(WHITE, 0.97))
        .shadow(vec![
            shadow(0.0, 0.0, 0.0, 0.5, 0.1),
            shadow(0.0, 2.0, 6.0, 0.0, 0.12),
        ])
        .text_size(px(TEXT_HINT))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(INK))
        .child(label)
}

impl Render for PreviewView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let card = match (&self.payload, self.phase) {
            (Some(payload), phase) if phase != Phase::Hidden => {
                Some(self.render_card(&payload.clone(), cx))
            }
            _ => None,
        };
        div().size_full().p(px(MARGIN as f32)).children(card)
    }
}

/// The single preview window, reused for every capture.
struct PreviewController {
    handle: WindowHandle<PreviewView>,
    window: Retained<NSWindow>,
}

impl Global for PreviewController {}

/// Shows `payload` in the bottom-right corner of the display under the
/// pointer, without taking focus.
pub fn show(payload: PreviewPayload, host: Rc<dyn PreviewHost>, cx: &mut App) -> Result<()> {
    let display = displays::under_cursor()?;
    let frame = window_frame(display.work_area, payload.width, payload.height);
    if !cx.has_global::<PreviewController>() {
        let controller = create(host, frame, cx)?;
        cx.set_global(controller);
    }
    let controller = cx.global::<PreviewController>();
    let (handle, window) = (controller.handle, controller.window.clone());
    let cocoa = displays::flip(frame, displays::primary_height()?);
    window.setFrame_display(
        NSRect::new(
            NSPoint::new(cocoa.x, cocoa.y),
            NSSize::new(cocoa.width, cocoa.height),
        ),
        true,
    );
    handle
        .update(cx, |view, _, cx| view.show(payload, cx))
        .map_err(|error| eyre!("{error}"))?;
    window.orderFrontRegardless();
    Ok(())
}

/// The preview window, once it exists.
pub fn handle(cx: &App) -> Option<WindowHandle<PreviewView>> {
    cx.try_global::<PreviewController>()
        .map(|controller| controller.handle)
}

/// Hides the preview at once (a new capture is starting).
pub fn hide(cx: &mut App) {
    let Some(controller) = cx.try_global::<PreviewController>() else {
        return;
    };
    let handle = controller.handle;
    let _ = handle.update(cx, |view, _, cx| view.set_hidden(cx));
    order_out(cx);
}

fn order_out(cx: &App) {
    if let Some(controller) = cx.try_global::<PreviewController>() {
        controller.window.orderOut(None);
    }
}

fn create(host: Rc<dyn PreviewHost>, frame: Rect, cx: &mut App) -> Result<PreviewController> {
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(frame.width as f32), px(frame.height as f32)),
        ))),
        titlebar: None,
        focus: false,
        show: false,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let handle = cx
        .open_window(options, |_, cx| cx.new(|_| PreviewView::new(host)))
        .map_err(|error| eyre!("{error}"))?;
    let window = handle
        .update(cx, |_, window, _| {
            ns_view(window).and_then(|view| view.window())
        })
        .map_err(|error| eyre!("{error}"))?
        .ok_or_else(|| eyre!("the preview has no NSWindow"))?;
    // Electron: alwaysOnTop "floating", all Spaces including full-screen ones.
    window.setLevel(NSFloatingWindowLevel);
    window.setCollectionBehavior(
        NSWindowCollectionBehavior::CanJoinAllSpaces
            | NSWindowCollectionBehavior::FullScreenAuxiliary,
    );
    window.setHidesOnDeactivate(false);
    window.setHasShadow(false);
    Ok(PreviewController { handle, window })
}

/// The AppKit view behind a GPUI window.
fn ns_view(window: &Window) -> Option<Retained<NSView>> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: AppKit window handles carry the window's content NSView.
    unsafe { Retained::retain(appkit.ns_view.as_ptr().cast()) }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use gpui::{TestAppContext, VisualTestContext};
    use image::RgbaImage;

    use super::*;

    #[test]
    fn thumbnail_height_is_clamped_like_electron() {
        assert_eq!(thumbnail_height(1601, 948), 147.0);
        assert_eq!(thumbnail_height(1000, 100), 120.0);
        assert_eq!(thumbnail_height(500, 1000), 220.0);
        assert_eq!(thumbnail_height(0, 0), 120.0);
        // 248 × 0.5 = 124 exactly; 248 × 0.6 = 148.8 → 149.
        assert_eq!(thumbnail_height(1000, 500), 124.0);
        assert_eq!(thumbnail_height(1000, 600), 149.0);
    }

    #[test]
    fn window_sits_in_the_bottom_right_of_the_work_area() {
        let laptop = Rect::new(0.0, 33.0, 1512.0, 856.0);
        let frame = window_frame(laptop, 1601, 948);
        assert_eq!(
            frame,
            Rect::new(
                1512.0 - 284.0 - 6.0,
                33.0 + 856.0 - 183.0 - 6.0,
                284.0,
                183.0
            )
        );
        // A display left of and above the primary one.
        let external = Rect::new(-2560.0, -1415.0, 2560.0, 1415.0);
        let frame = window_frame(external, 400, 800);
        assert_eq!(
            frame,
            Rect::new(-290.0, -1415.0 + 1415.0 - 256.0 - 6.0, 284.0, 256.0)
        );
    }

    #[derive(Default)]
    struct FakeHost {
        lines: Cell<usize>,
        edited: Cell<usize>,
    }

    impl PreviewHost for FakeHost {
        fn open_editor(&self, _: &str, _: &mut App) {
            self.edited.set(self.edited.get() + 1);
        }

        fn reveal(&self, _: &Path) {}

        fn copy_text(&self, _: &str, _: &mut App) -> Task<Result<usize, SharedString>> {
            Task::ready(Ok(self.lines.get()))
        }
    }

    fn payload(id: &str) -> PreviewPayload {
        let image = RgbaImage::from_pixel(496, 294, image::Rgba([200, 200, 200, 255]));
        PreviewPayload {
            id: id.into(),
            kind: CaptureKind::Image,
            path: PathBuf::from(format!("/tmp/{id}.png")),
            width: 1601,
            height: 948,
            thumbnail: crate::thumbnail::for_preview(&image).unwrap(),
        }
    }

    fn open(
        cx: &mut TestAppContext,
        host: Rc<FakeHost>,
    ) -> (gpui::Entity<PreviewView>, &mut VisualTestContext) {
        cx.add_window_view(|_, _| PreviewView::new(host))
    }

    fn phase(view: &gpui::Entity<PreviewView>, cx: &mut VisualTestContext) -> Phase {
        view.read_with(cx, |view, _| view.phase)
    }

    #[gpui::test]
    fn hides_itself_after_six_seconds(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, Rc::default());
        view.update(cx, |view, cx| view.show(payload("a"), cx));
        cx.executor().advance_clock(Duration::from_millis(5900));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::In);
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::Out);
        cx.executor().advance_clock(SLIDE_OUT);
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::Hidden);
    }

    #[gpui::test]
    fn hovering_pauses_auto_hide(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, Rc::default());
        view.update(cx, |view, cx| view.show(payload("a"), cx));
        cx.executor().advance_clock(Duration::from_secs(3));
        view.update(cx, |view, cx| view.set_hovered(true, cx));
        cx.executor().advance_clock(Duration::from_secs(30));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::In);
        // Leaving restarts the full 6 s.
        view.update(cx, |view, cx| view.set_hovered(false, cx));
        cx.executor().advance_clock(Duration::from_millis(5900));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::In);
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::Out);
    }

    #[gpui::test]
    fn a_new_capture_restarts_the_cycle(cx: &mut TestAppContext) {
        let (view, cx) = open(cx, Rc::default());
        view.update(cx, |view, cx| view.show(payload("a"), cx));
        cx.executor().advance_clock(Duration::from_secs(4));
        view.update(cx, |view, cx| view.show(payload("b"), cx));
        let generation = view.read_with(cx, |view, _| view.generation);
        assert_eq!(generation, 2);
        cx.executor().advance_clock(Duration::from_millis(5900));
        cx.run_until_parked();
        assert_eq!(
            phase(&view, cx),
            Phase::In,
            "the first timer must not hide the second capture"
        );
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::Out);

        // A slide-out that is overtaken by a new capture must not hide it.
        view.update(cx, |view, cx| view.show(payload("c"), cx));
        cx.executor().advance_clock(SLIDE_OUT);
        cx.run_until_parked();
        assert_eq!(phase(&view, cx), Phase::In);
    }

    #[gpui::test]
    fn copy_text_updates_the_chip(cx: &mut TestAppContext) {
        let host = Rc::new(FakeHost::default());
        let (view, cx) = open(cx, host.clone());
        view.update(cx, |view, cx| view.show(payload("a"), cx));
        assert_eq!(view.read_with(cx, |view, _| view.note.clone()), "Copied");

        host.lines.set(3);
        view.update(cx, |view, cx| view.copy_text(cx));
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |view, _| view.note.clone()),
            "Text copied"
        );

        host.lines.set(0);
        view.update(cx, |view, cx| view.copy_text(cx));
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |view, _| view.note.clone()),
            "No text found"
        );
    }

    #[gpui::test]
    fn edit_opens_the_editor_and_dismisses(cx: &mut TestAppContext) {
        let host = Rc::new(FakeHost::default());
        let (view, cx) = open(cx, host.clone());
        view.update(cx, |view, cx| view.show(payload("a"), cx));
        view.update(cx, |view, cx| view.set_hovered(true, cx));
        cx.run_until_parked();
        let edit = cx.debug_bounds("preview-edit").unwrap();
        cx.simulate_click(edit.center(), gpui::Modifiers::none());
        cx.run_until_parked();
        assert_eq!(host.edited.get(), 1);
        assert_eq!(phase(&view, cx), Phase::Out);
    }
}
