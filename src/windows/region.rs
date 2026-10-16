//! The record-area overlay (port of `Region.tsx` and `selectRegion` in
//! `Windows.ts`): a dimmed full-display window where a drag picks the
//! rectangle to record.

use std::time::Duration;

use futures::channel::oneshot;
use gpui::{
    Animation, AnimationExt, App, AppContext, AsyncApp, Bounds, Context, FocusHandle, Focusable,
    FontWeight, Global, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Render, Subscription, Window, WindowBackgroundAppearance,
    WindowBounds, WindowHandle, WindowKind, WindowOptions, div, point, prelude::*, px, relative,
    rgb, size,
};
use objc2::rc::Retained;
use objc2_app_kit::{
    NSScreenSaverWindowLevel, NSView, NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_foundation::{NSPoint, NSRect, NSSize};

use crate::desktop_ui::{
    FADE_IN, INK, MONO_FONT, MUTED, RED_500, RISE_IN, TEXT_HINT, TEXT_XS, alpha, ease_out_soft,
};
use crate::displays::{self, Display, Rect};

const MIN_SIZE: f32 = 8.0;
/// Let the overlay disappear before anything is recorded.
const AFTER_CLOSE: Duration = Duration::from_millis(180);

struct Active {
    handle: WindowHandle<RegionView>,
    display: Display,
    result: Option<oneshot::Sender<Option<Rect>>>,
}

#[derive(Default)]
struct RegionOverlay(Option<Active>);

impl Global for RegionOverlay {}

/// Shows the overlay on the display under the pointer. Resolves to the
/// chosen rectangle in global top-left points, or `None` if cancelled.
/// A new request cancels the previous one.
pub fn select(cx: &mut App) -> oneshot::Receiver<Option<Rect>> {
    cancel(cx);
    let (sender, receiver) = oneshot::channel();
    match open(None, true, cx) {
        Ok((handle, display)) => {
            cx.set_global(RegionOverlay(Some(Active {
                handle,
                display,
                result: Some(sender),
            })));
        }
        Err(error) => {
            tracing::error!(%error, "could not show the region overlay");
            let _ = sender.send(None);
        }
    }
    receiver
}

/// `ss preview region`: the overlay without a pending request.
pub fn preview(drag: Option<Rect>, cx: &mut App) -> color_eyre::Result<()> {
    let (handle, _) = open(drag, false, cx)?;
    cx.set_global(RegionOverlay(Some(Active {
        handle,
        display: displays::under_cursor()?,
        result: None,
    })));
    Ok(())
}

fn open(
    drag: Option<Rect>,
    activate: bool,
    cx: &mut App,
) -> color_eyre::Result<(WindowHandle<RegionView>, Display)> {
    let display = displays::under_cursor()?;
    let gpui_display = cx
        .displays()
        .into_iter()
        .find(|candidate| u32::from(candidate.id()) == display.number);
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
            point(px(0.0), px(0.0)),
            size(
                px(display.bounds.width as f32),
                px(display.bounds.height as f32),
            ),
        ))),
        display_id: gpui_display.map(|display| display.id()),
        titlebar: None,
        focus: activate,
        show: true,
        kind: WindowKind::PopUp,
        is_movable: false,
        is_resizable: false,
        is_minimizable: false,
        window_background: WindowBackgroundAppearance::Transparent,
        ..Default::default()
    };
    let handle = cx
        .open_window(options, |window, cx| {
            cx.new(|cx| RegionView::new(drag, activate, window, cx))
        })
        .map_err(|error| color_eyre::eyre::eyre!("{error}"))?;
    let primary_height = displays::primary_height()?;
    handle
        .update(cx, |_, window, _| {
            let Some(native) = ns_view(window).and_then(|view| view.window()) else {
                return;
            };
            // Borderless so it may cover the menu bar; above everything, on every Space.
            native.setStyleMask(
                NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
            );
            native.setLevel(NSScreenSaverWindowLevel);
            native.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
            native.setHidesOnDeactivate(false);
            native.setHasShadow(false);
            let frame = displays::flip(display.bounds, primary_height);
            native.setFrame_display(
                NSRect::new(
                    NSPoint::new(frame.x, frame.y),
                    NSSize::new(frame.width, frame.height),
                ),
                true,
            );
            if activate {
                native.makeKeyAndOrderFront(None);
            } else {
                native.orderFrontRegardless();
            }
        })
        .map_err(|error| color_eyre::eyre::eyre!("{error}"))?;
    if activate {
        cx.activate(true);
    }
    Ok((handle, display))
}

/// Closes the overlay and resolves the request after the fade.
fn finish(local: Option<Option<Bounds<Pixels>>>, cx: &mut App) {
    let Some(active) = cx
        .try_global::<RegionOverlay>()
        .and_then(|overlay| overlay.0.as_ref())
    else {
        return;
    };
    let display = active.display;
    let handle = active.handle;
    let result = cx
        .global_mut::<RegionOverlay>()
        .0
        .take()
        .and_then(|mut active| active.result.take());
    let _ = handle.update(cx, |_, window, _| window.remove_window());
    let rect = local.map(|local| global_rect(display, local));
    if let Some(result) = result {
        cx.spawn(async move |cx: &mut AsyncApp| {
            cx.background_executor().timer(AFTER_CLOSE).await;
            let _ = result.send(rect);
        })
        .detach();
    }
}

fn cancel(cx: &mut App) {
    finish(None, cx);
}

/// A local selection (or the whole display) in global top-left points.
fn global_rect(display: Display, local: Option<Bounds<Pixels>>) -> Rect {
    match local {
        None => display.bounds,
        Some(local) => Rect::new(
            display.bounds.x + f64::from(f32::from(local.origin.x)),
            display.bounds.y + f64::from(f32::from(local.origin.y)),
            f64::from(f32::from(local.size.width)),
            f64::from(f32::from(local.size.height)),
        ),
    }
}

/// The rectangle between two corners, whichever way it was dragged.
fn rect_between(start: Point<Pixels>, end: Point<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(start.x.min(end.x), start.y.min(end.y)),
        size((end.x - start.x).abs(), (end.y - start.y).abs()),
    )
}

fn big_enough(rect: Bounds<Pixels>) -> bool {
    f32::from(rect.size.width) >= MIN_SIZE && f32::from(rect.size.height) >= MIN_SIZE
}

pub struct RegionView {
    focus: FocusHandle,
    start: Option<Point<Pixels>>,
    end: Option<Point<Pixels>>,
    _blur: Option<Subscription>,
}

impl RegionView {
    fn new(
        drag: Option<Rect>,
        interactive: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        // Losing focus cancels, like the Electron overlay's `blur`.
        let blur = interactive.then(|| {
            cx.observe_window_activation(window, |_, window, cx| {
                if !window.is_window_active() {
                    cx.defer(cancel);
                }
            })
        });
        let (start, end) = match drag {
            Some(rect) => (
                Some(point(px(rect.x as f32), px(rect.y as f32))),
                Some(point(
                    px((rect.x + rect.width) as f32),
                    px((rect.y + rect.height) as f32),
                )),
            ),
            None => (None, None),
        };
        Self {
            focus,
            start,
            end,
            _blur: blur,
        }
    }

    fn rect(&self) -> Option<Bounds<Pixels>> {
        Some(rect_between(self.start?, self.end?))
    }

    fn release(&mut self, cx: &mut Context<Self>) {
        match self.rect() {
            Some(rect) if big_enough(rect) => cx.defer(move |cx| finish(Some(Some(rect)), cx)),
            _ => {
                self.start = None;
                self.end = None;
                cx.notify();
            }
        }
    }
}

impl Focusable for RegionView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for RegionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let dim = |bounds: Bounds<Pixels>| {
            div()
                .absolute()
                .left(bounds.origin.x)
                .top(bounds.origin.y)
                .w(bounds.size.width)
                .h(bounds.size.height)
                .bg(alpha(0x000000, 0.32))
        };
        let selection = match self.rect() {
            None => vec![
                div()
                    .absolute()
                    .inset_0()
                    .bg(alpha(0x000000, 0.25))
                    .into_any_element(),
            ],
            Some(rect) => {
                let (left, top) = (rect.origin.x, rect.origin.y);
                let (right, bottom) = (rect.right(), rect.bottom());
                vec![
                    dim(Bounds::new(
                        point(px(0.0), px(0.0)),
                        size(viewport.width, top),
                    )),
                    dim(Bounds::new(
                        point(px(0.0), bottom),
                        size(viewport.width, viewport.height - bottom),
                    )),
                    dim(Bounds::new(
                        point(px(0.0), top),
                        size(left, rect.size.height),
                    )),
                    dim(Bounds::new(
                        point(right, top),
                        size(viewport.width - right, rect.size.height),
                    )),
                ]
                .into_iter()
                .map(IntoElement::into_any_element)
                .chain([
                    div()
                        .absolute()
                        .left(left)
                        .top(top)
                        .w(rect.size.width)
                        .h(rect.size.height)
                        .rounded(px(2.0))
                        .border_1()
                        .border_color(alpha(0xffffff, 0.9))
                        .into_any_element(),
                    div()
                        .absolute()
                        .left(left)
                        .top(bottom + px(8.0))
                        .px(px(6.0))
                        .py(px(2.0))
                        .rounded(px(6.0))
                        .bg(alpha(0x000000, 0.7))
                        .font_family(MONO_FONT)
                        .text_size(px(TEXT_XS))
                        .text_color(rgb(0xffffff))
                        .child(format!(
                            "{} × {}",
                            f32::from(rect.size.width).round() as i64,
                            f32::from(rect.size.height).round() as i64
                        ))
                        .into_any_element(),
                ])
                .collect()
            }
        };
        let hint = self.start.is_none().then(|| {
            div()
                .absolute()
                .top(relative(0.18))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .rounded(px(16.0))
                        // More opaque than Electron's white/90 + backdrop blur (plan §7.6).
                        .bg(alpha(0xffffff, 0.96))
                        .px_5()
                        .py_3()
                        .shadow(vec![gpui::BoxShadow {
                            color: alpha(0x000000, 0.18).into(),
                            offset: point(px(0.0), px(20.0)),
                            blur_radius: px(25.0),
                            spread_radius: px(-5.0),
                        }])
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(INK))
                                .child(
                                    div().size(px(8.0)).rounded_full().bg(rgb(RED_500)).shadow(vec![
                                        gpui::BoxShadow {
                                            color: alpha(0xef4444, 0.2).into(),
                                            offset: point(px(0.0), px(0.0)),
                                            blur_radius: px(0.0),
                                            spread_radius: px(3.0),
                                        },
                                    ]),
                                )
                                .child("Drag to choose what to record"),
                        )
                        .child(
                            div()
                                .mt_1()
                                .text_size(px(TEXT_HINT))
                                .text_color(rgb(MUTED))
                                .child("↩ whole screen · Esc cancel · click the menu bar timer to stop"),
                        ),
                )
                .with_animation(
                    "hint-rise-in",
                    Animation::new(RISE_IN).with_easing(ease_out_soft()),
                    |hint, delta| hint.opacity(delta).mt(px(8.0 * (1.0 - delta))),
                )
        });
        div()
            .id("region")
            .size_full()
            .relative()
            .cursor_crosshair()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                match event.keystroke.key.as_str() {
                    "escape" => cx.defer(cancel),
                    "enter" => cx.defer(|cx| finish(Some(None), cx)),
                    _ => {}
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    this.start = Some(event.position);
                    this.end = Some(event.position);
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                if this.start.is_some() {
                    this.end = Some(event.position);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.release(cx)),
            )
            .children(selection)
            .children(hint)
            .with_animation(
                "region-fade-in",
                Animation::new(FADE_IN).with_easing(ease_out_soft()),
                |overlay, delta| overlay.opacity(delta),
            )
    }
}

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
    use super::*;

    fn display() -> Display {
        Display {
            bounds: Rect::new(-1920.0, -98.0, 1920.0, 1080.0),
            work_area: Rect::new(-1920.0, -73.0, 1920.0, 1055.0),
            number: 2,
        }
    }

    #[test]
    fn drags_normalise_in_any_direction() {
        let rect = rect_between(point(px(300.0), px(200.0)), point(px(100.0), px(250.0)));
        assert_eq!(
            rect,
            Bounds::new(point(px(100.0), px(200.0)), size(px(200.0), px(50.0)))
        );
    }

    #[test]
    fn tiny_drags_are_clicks() {
        assert!(!big_enough(Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(7.9), px(40.0))
        )));
        assert!(!big_enough(Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(40.0), px(7.0))
        )));
        assert!(big_enough(Bounds::new(
            point(px(0.0), px(0.0)),
            size(px(8.0), px(8.0))
        )));
    }

    #[test]
    fn local_rects_become_global_points() {
        let local = Bounds::new(point(px(10.0), px(20.0)), size(px(300.0), px(200.0)));
        assert_eq!(
            global_rect(display(), Some(local)),
            Rect::new(-1910.0, -78.0, 300.0, 200.0)
        );
        // Enter records the whole display.
        assert_eq!(global_rect(display(), None), display().bounds);
    }
}
