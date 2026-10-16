//! Shared visual tokens and controls. Values come from the Electron app's
//! `renderer/src/index.css` theme and `components/ui.tsx`. Every window builds
//! from these; windows must not introduce their own headers, rows or widths.

// Tokens and controls cover every planned window; later milestones use the rest.
#![allow(dead_code)]

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, Bounds, BoxShadow, Div, ElementId, FontWeight,
    HighlightStyle, Hsla, IntoElement, MouseButton, Pixels, Rgba, SharedString, Size, Stateful,
    StyledText, TitlebarOptions, Window, WindowBounds, WindowControlArea, WindowOptions, div,
    point, prelude::*, px, rgb, size,
};

// Palette (`@theme` in index.css).
pub(crate) const CANVAS: u32 = 0xf7f5f2;
pub(crate) const STAGE: u32 = 0xefece8;
pub(crate) const PANEL: u32 = 0xfbfaf8;
pub(crate) const INK: u32 = 0x1c1b1a;
pub(crate) const INK_SOFT: u32 = 0x57534e;
pub(crate) const MUTED: u32 = 0x9a948d;
pub(crate) const ACCENT: u32 = 0x7c5cff;
pub(crate) const WHITE: u32 = 0xffffff;
/// `--color-line` is stone-900 at 7%.
const LINE_RGB: u32 = 0x1c1917;
const LINE_ALPHA: f32 = 0.07;
const ACCENT_SOFT_ALPHA: f32 = 0.12;

// Tailwind 4 status colours used by ui.tsx and Settings.tsx (sRGB of the oklch values).
pub(crate) const EMERALD_500: u32 = 0x00bc7d;
pub(crate) const EMERALD_600: u32 = 0x009966;
pub(crate) const EMERALD_700: u32 = 0x007a55;
pub(crate) const AMBER_500: u32 = 0xfe9a00;
pub(crate) const AMBER_700: u32 = 0xbb4d00;
pub(crate) const RED_500: u32 = 0xfb2c36;

/// `SF Mono` is not a public font family; Menlo is next in the CSS stack.
pub(crate) const MONO_FONT: &str = "Menlo";

// Radii.
pub(crate) const RADIUS_MD: f32 = 6.0;
pub(crate) const RADIUS_LG: f32 = 8.0;
pub(crate) const RADIUS_XL: f32 = 12.0;
pub(crate) const RADIUS_SWATCH: f32 = 10.0;
pub(crate) const RADIUS_PREVIEW: f32 = 14.0;

// Type scale.
pub(crate) const TEXT_2XS: f32 = 10.5;
pub(crate) const TEXT_XS: f32 = 11.0;
pub(crate) const TEXT_HINT: f32 = 11.5;
pub(crate) const TEXT_SM: f32 = 12.0;
pub(crate) const TEXT_CONTROL: f32 = 12.5;
pub(crate) const TEXT_BASE: f32 = 13.0;
pub(crate) const TEXT_BODY: f32 = 13.5;
pub(crate) const TEXT_EMPHASIS: f32 = 14.0;
pub(crate) const TEXT_LG: f32 = 16.0;
pub(crate) const TEXT_TITLE: f32 = 22.0;

// Control and layout sizes.
pub(crate) const BUTTON_HEIGHT: f32 = 32.0;
pub(crate) const SEGMENT_HEIGHT: f32 = 28.0;
pub(crate) const COMPACT_ROW_HEIGHT: f32 = 44.0;
pub(crate) const STATUS_PILL_HEIGHT: f32 = 28.0;
pub(crate) const SWITCH_WIDTH: f32 = 38.0;
pub(crate) const SWITCH_HEIGHT: f32 = 22.0;
const SWITCH_KNOB: f32 = 18.0;
pub(crate) const TITLEBAR_HEIGHT: f32 = 48.0;
pub(crate) const CONTENT_PADDING: f32 = 28.0;
pub(crate) const SECTION_GAP: f32 = 28.0;
const SECTION_INNER_GAP: f32 = 10.0;
pub(crate) const TRAFFIC_LIGHT_INSET: f32 = 18.0;

// Motion.
pub(crate) const TRANSITION: Duration = Duration::from_millis(200);
pub(crate) const SWITCH_TRANSITION: Duration = Duration::from_millis(300);
pub(crate) const FADE_IN: Duration = Duration::from_millis(260);
pub(crate) const RISE_IN: Duration = Duration::from_millis(420);
pub(crate) const SLIDE_IN: Duration = Duration::from_millis(520);
pub(crate) const SLIDE_OUT: Duration = Duration::from_millis(260);
pub(crate) const TOAST: Duration = Duration::from_millis(2200);

pub(crate) fn alpha(hex: u32, alpha: f32) -> Rgba {
    Rgba {
        a: alpha,
        ..rgb(hex)
    }
}

pub(crate) fn line() -> Rgba {
    alpha(LINE_RGB, LINE_ALPHA)
}

pub(crate) fn accent_soft() -> Rgba {
    alpha(ACCENT, ACCENT_SOFT_ALPHA)
}

fn black(opacity: f32) -> Hsla {
    alpha(0x000000, opacity).into()
}

/// `0 0 0 0.5px rgb(0 0 0 / a)`: the hairline ring ui.tsx uses instead of borders.
fn ring(opacity: f32) -> BoxShadow {
    BoxShadow {
        color: black(opacity),
        offset: point(px(0.0), px(0.0)),
        blur_radius: px(0.0),
        spread_radius: px(0.5),
    }
}

fn drop_shadow(offset_y: f32, blur: f32, opacity: f32) -> BoxShadow {
    BoxShadow {
        color: black(opacity),
        offset: point(px(0.0), px(offset_y)),
        blur_radius: px(blur),
        spread_radius: px(0.0),
    }
}

/// `cubic-bezier(x1, y1, x2, y2)` as a GPUI easing function.
pub(crate) fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32) -> impl Fn(f32) -> f32 + Copy {
    let curve = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    move |progress: f32| {
        let x = progress.clamp(0.0, 1.0);
        // x(t) is monotonic for x1, x2 in 0..=1, so bisection always converges.
        let (mut low, mut high) = (0.0_f32, 1.0_f32);
        let mut t = x;
        for _ in 0..32 {
            let estimate = curve(x1, x2, t);
            if (estimate - x).abs() < 1e-6 {
                break;
            }
            if estimate < x {
                low = t;
            } else {
                high = t;
            }
            t = (low + high) / 2.0;
        }
        curve(y1, y2, t)
    }
}

/// `--ease-out-soft`.
pub(crate) fn ease_out_soft() -> impl Fn(f32) -> f32 + Copy {
    cubic_bezier(0.22, 1.0, 0.36, 1.0)
}

/// `--ease-spring`; overshoots past 1 before settling. GPUI asserts that
/// `Animation::with_easing` stays within 0..=1, so run the animation linear
/// and apply this inside the animator closure instead.
pub(crate) fn ease_spring() -> impl Fn(f32) -> f32 + Copy {
    cubic_bezier(0.34, 1.36, 0.64, 1.0)
}

pub(crate) fn mix_color(from: Rgba, to: Rgba, position: f32) -> Rgba {
    let position = position.clamp(0.0, 1.0);
    Rgba {
        r: from.r + (to.r - from.r) * position,
        g: from.g + (to.g - from.g) * position,
        b: from.b + (to.b - from.b) * position,
        a: from.a + (to.a - from.a) * position,
    }
}

/// Options for a standard ss window: centred, fixed size, transparent
/// titlebar with the traffic lights inset like `hiddenInset`.
pub(crate) fn window_options(
    title: &'static str,
    window_size: Size<Pixels>,
    cx: &App,
) -> WindowOptions {
    WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            window_size,
            cx,
        ))),
        titlebar: Some(TitlebarOptions {
            title: Some(title.into()),
            appears_transparent: true,
            traffic_light_position: Some(point(px(TRAFFIC_LIGHT_INSET), px(TRAFFIC_LIGHT_INSET))),
        }),
        is_resizable: false,
        is_minimizable: false,
        app_id: Some("dev.sztuczka.ss".into()),
        ..Default::default()
    }
}

/// The root of every window.
pub(crate) fn window_frame() -> Div {
    div()
        .size_full()
        .relative()
        .overflow_hidden()
        .flex()
        .flex_col()
        .bg(rgb(CANVAS))
        .text_color(rgb(INK))
        .text_size(px(TEXT_BASE))
}

/// The draggable header strip (`.drag` in Electron). Double-click zooms or
/// minimizes per the system preference.
pub(crate) fn titlebar(title: Option<&'static str>) -> Div {
    div()
        .h(px(TITLEBAR_HEIGHT))
        .w_full()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .window_control_area(WindowControlArea::Drag)
        .on_mouse_down(MouseButton::Left, |event, window, _| {
            if event.click_count == 2 {
                window.titlebar_double_click();
            }
        })
        .when_some(title, |bar, title| {
            bar.child(
                div()
                    .text_size(px(TEXT_BASE))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ButtonVariant {
    Ghost,
    Primary,
}

/// `<Button>`: a 32 pt chip, white with a hairline ring, or solid ink when
/// primary. `shortcut` adds a trailing [`kbd`] hint.
pub(crate) fn button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: ButtonVariant,
    shortcut: Option<SharedString>,
) -> Stateful<Div> {
    let primary = variant == ButtonVariant::Primary;
    div()
        .id(id)
        .h(px(BUTTON_HEIGHT))
        .px_3()
        .flex_none()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(RADIUS_LG))
        .text_size(px(TEXT_BASE))
        .font_weight(FontWeight::MEDIUM)
        .map(|button| {
            if primary {
                button
                    .bg(rgb(INK))
                    .text_color(rgb(WHITE))
                    .shadow(vec![drop_shadow(1.0, 2.0, 0.05)])
                    .hover(|button| button.bg(alpha(INK, 0.9)))
            } else {
                button
                    .bg(rgb(WHITE))
                    .text_color(rgb(INK))
                    .shadow(vec![ring(0.1), drop_shadow(1.0, 2.0, 0.05)])
                    .hover(|button| button.bg(alpha(WHITE, 0.7)))
            }
        })
        .child(label.into())
        .when_some(shortcut, |button, shortcut| {
            button.child(kbd(shortcut).when(primary, |hint| hint.text_color(alpha(WHITE, 0.6))))
        })
}

/// `<Kbd>`: an inline shortcut hint.
pub(crate) fn kbd(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(TEXT_XS))
        .text_color(rgb(MUTED))
        .child(text.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyCapSize {
    Medium,
    Large,
}

/// `<KeyCaps>`: one white cap per key, e.g. [⇧][⌘][4].
pub(crate) fn key_caps(keys: &[String], cap_size: KeyCapSize) -> Div {
    let (height, padding, text) = match cap_size {
        KeyCapSize::Medium => (24.0, 6.0, TEXT_SM),
        KeyCapSize::Large => (36.0, 8.0, TEXT_LG),
    };
    div()
        .flex()
        .items_center()
        .gap_1()
        .children(keys.iter().map(|key| {
            div()
                .h(px(height))
                .min_w(px(height))
                .px(px(padding))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(RADIUS_MD))
                .bg(rgb(WHITE))
                .shadow(vec![ring(0.14), drop_shadow(1.5, 0.0, 0.08)])
                .text_size(px(text))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(INK))
                .child(key.clone())
        }))
}

/// `<Section>`: an uppercase label with an optional trailing aside; append
/// the section's content as children.
pub(crate) fn section(title: &'static str, aside: Option<AnyElement>) -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .gap(px(SECTION_INNER_GAP))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(TEXT_XS))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(MUTED))
                        .child(title.to_uppercase()),
                )
                .when_some(aside, |header, aside| header.child(aside)),
        )
}

/// The [`card`] shadow, plus the 2 pt accent ring of a selected option.
pub(crate) fn card_shadows(selected: bool) -> Vec<BoxShadow> {
    if selected {
        vec![BoxShadow {
            color: rgb(ACCENT).into(),
            offset: point(px(0.0), px(0.0)),
            blur_radius: px(0.0),
            spread_radius: px(2.0),
        }]
    } else {
        vec![ring(0.08), drop_shadow(1.0, 2.0, 0.03)]
    }
}

/// The white rounded card behind grouped rows.
pub(crate) fn card() -> Div {
    div()
        .w_full()
        .flex()
        .flex_col()
        .rounded(px(RADIUS_XL))
        .bg(rgb(WHITE))
        .shadow(vec![ring(0.08), drop_shadow(1.0, 2.0, 0.03)])
}

/// A [`card`] whose rows are separated by hairlines (`divide-y divide-line`).
pub(crate) fn divided_card(rows: impl IntoIterator<Item = AnyElement>) -> Div {
    card().children(rows.into_iter().enumerate().map(|(index, row)| {
        div()
            .w_full()
            .when(index > 0, |row| row.border_t_1().border_color(line()))
            .child(row)
    }))
}

/// A card row: title and optional description on the left, a fixed control
/// on the right. The copy column shrinks and wraps; the control never moves.
/// A truncated description must also set `w_full`.
pub(crate) fn settings_row(
    title: impl Into<SharedString>,
    description: Option<AnyElement>,
    control: impl IntoElement,
) -> Div {
    div()
        .w_full()
        .px_4()
        .py(px(10.0))
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            // Block layout, not a flex column: GPUI caches a nowrap text's
            // first measurement, and a column's intrinsic pass would leave
            // a truncated description as just "…".
            div()
                .debug_selector(|| "settings-copy".into())
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(TEXT_BASE)).child(title.into()))
                .children(description),
        )
        .child(control)
}

/// The muted second line of a [`settings_row`].
pub(crate) fn row_description(text: impl Into<SharedString>) -> AnyElement {
    div()
        .text_size(px(TEXT_HINT))
        .text_color(rgb(MUTED))
        .child(text.into())
        .into_any_element()
}

/// A single-line 44 pt row: label left, control right.
pub(crate) fn compact_row(label: impl Into<SharedString>, control: impl IntoElement) -> Div {
    div()
        .w_full()
        .h(px(COMPACT_ROW_HEIGHT))
        .px_4()
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_x_hidden()
                .truncate()
                .text_size(px(TEXT_BASE))
                .child(label.into()),
        )
        .child(control)
}

/// Muted helper copy under a section's content.
pub(crate) fn hint(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(TEXT_HINT))
        .line_height(px(18.0))
        .text_color(rgb(MUTED))
        .child(text.into())
}

/// The accent text button used for section asides ("Open Setup Guide").
pub(crate) fn link_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
) -> Stateful<Div> {
    div()
        .id(id)
        .text_size(px(TEXT_HINT))
        .font_weight(FontWeight::MEDIUM)
        .text_color(rgb(ACCENT))
        .hover(|link| link.underline())
        .child(label.into())
}

/// `<Segmented>`: equal-width options on a recessed track. Clicking a
/// different option calls `on_change`.
pub(crate) fn segmented<T: Copy + PartialEq + 'static>(
    id: impl Into<ElementId>,
    options: &[(T, &'static str)],
    selected: T,
    on_change: impl Fn(T, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let on_change = Rc::new(on_change);
    div()
        .id(id)
        .w_full()
        .p(px(2.0))
        .flex()
        .rounded(px(RADIUS_LG))
        .bg(alpha(0x000000, 0.045))
        .children(options.iter().enumerate().map(|(index, &(value, label))| {
            let active = value == selected;
            let on_change = on_change.clone();
            div()
                .id(index)
                .debug_selector(move || format!("segmented-item-{index}"))
                .h(px(SEGMENT_HEIGHT))
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(7.0))
                .text_size(px(TEXT_SM))
                .font_weight(FontWeight::MEDIUM)
                .map(|item| {
                    if active {
                        item.bg(rgb(WHITE))
                            .text_color(rgb(INK))
                            .shadow(vec![ring(0.08), drop_shadow(1.0, 2.0, 0.08)])
                    } else {
                        item.text_color(rgb(INK_SOFT))
                            .hover(|item| item.text_color(rgb(INK)))
                    }
                })
                .on_click(move |_, window, cx| {
                    if !active {
                        on_change(value, window, cx);
                    }
                })
                .child(label)
        }))
}

/// `<Switch>` at `position` 0 (off) to 1 (on). Callers animate `position`
/// with [`ease_spring`] over [`SWITCH_TRANSITION`].
pub(crate) fn switch_toggle(position: f32, disabled: bool) -> Div {
    let inset = 2.0;
    let travel = SWITCH_WIDTH - SWITCH_KNOB - inset * 2.0;
    div()
        .relative()
        .w(px(SWITCH_WIDTH))
        .h(px(SWITCH_HEIGHT))
        .flex_none()
        .rounded_full()
        .bg(mix_color(alpha(0x000000, 0.12), rgb(ACCENT), position))
        .when(disabled, |switch| switch.opacity(0.4))
        .child(
            div()
                .absolute()
                .top(px(inset))
                .left(px(inset + travel * position))
                .size(px(SWITCH_KNOB))
                .rounded_full()
                .bg(rgb(WHITE))
                .shadow(vec![drop_shadow(1.0, 3.0, 0.25)]),
        )
}

/// `<StatusPill>`: green with a check when `ok`, amber with a dot otherwise.
pub(crate) fn status_pill(ok: bool, label: impl Into<SharedString>) -> Div {
    let (background, foreground) = if ok {
        (alpha(EMERALD_500, 0.10), rgb(EMERALD_700))
    } else {
        (alpha(AMBER_500, 0.12), rgb(AMBER_700))
    };
    div()
        .h(px(STATUS_PILL_HEIGHT))
        .px(px(10.0))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(6.0))
        .rounded_full()
        .bg(background)
        .text_size(px(TEXT_SM))
        .font_weight(FontWeight::MEDIUM)
        .text_color(foreground)
        .child(if ok {
            gpui_symbols::Icon::new("checkmark")
                .size(px(11.0))
                .color(foreground)
                .weight(gpui_symbols::SymbolWeight::Semibold)
                .into_any_element()
        } else {
            div()
                .size(px(6.0))
                .rounded_full()
                .bg(foreground)
                .into_any_element()
        })
        .child(label.into())
}

/// Where a slider's track is, reported every frame so a drag that leaves the
/// track can still map the pointer to a value.
#[derive(Clone, Default)]
pub(crate) struct SliderTrack(Rc<Cell<Option<Bounds<Pixels>>>>);

impl SliderTrack {
    /// The value under `x`, or `None` before the first frame.
    pub(crate) fn value_at(&self, x: Pixels) -> Option<f64> {
        self.0.get().map(|bounds| slider_value(bounds, x))
    }
}

const SLIDER_HEIGHT: f32 = 18.0;
const SLIDER_KNOB: f32 = 18.0;

/// A range input's value under `x`: 0–100 in steps of 1, with the knob's
/// centre travelling between half a knob from either end.
pub(crate) fn slider_value(bounds: Bounds<Pixels>, x: Pixels) -> f64 {
    let travel = (f32::from(bounds.size.width) - SLIDER_KNOB).max(1.0);
    let offset = f32::from(x - bounds.origin.x) - SLIDER_KNOB / 2.0;
    (f64::from(offset / travel).clamp(0.0, 1.0) * 100.0).round()
}

/// `<Slider>`: a label row (value or `hint` on the right) over a 4 pt track
/// filled with the accent up to `value`, and an 18 pt knob that grows while
/// `active`. `on_press` starts a drag; the window follows the pointer.
pub(crate) fn slider(
    id: impl Into<ElementId>,
    label: &'static str,
    value: f64,
    active: bool,
    hint: Option<AnyElement>,
    track: &SliderTrack,
    on_press: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Div {
    let fraction = (value / 100.0).clamp(0.0, 1.0) as f32;
    let knob = if active {
        SLIDER_KNOB * 1.12
    } else {
        SLIDER_KNOB
    };
    let bounds = track.0.clone();
    div()
        .w_full()
        .flex()
        .flex_col()
        .child(
            div()
                .mb(px(6.0))
                .flex()
                .items_baseline()
                .justify_between()
                .child(
                    div()
                        .text_size(px(TEXT_BASE))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(INK))
                        .child(label),
                )
                .child(hint.unwrap_or_else(|| {
                    div()
                        .text_size(px(TEXT_XS))
                        .text_color(rgb(MUTED))
                        .child(format!("{}", value.round() as i64))
                        .into_any_element()
                })),
        )
        .child(
            div()
                .id(id)
                .relative()
                .w_full()
                .h(px(SLIDER_HEIGHT))
                .on_mouse_down(MouseButton::Left, on_press)
                .child(
                    gpui::canvas(move |area, _, _| bounds.set(Some(area)), |_, _, _, _| {})
                        .absolute()
                        .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .top(px((SLIDER_HEIGHT - 4.0) / 2.0))
                        .w_full()
                        .h(px(4.0))
                        .rounded_full()
                        .bg(alpha(LINE_RGB, 0.09))
                        .child(
                            div()
                                .h_full()
                                .w(gpui::relative(fraction))
                                .rounded_full()
                                .bg(rgb(ACCENT)),
                        ),
                )
                // The knob travels over the track minus its own width.
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left_0()
                        .right(px(SLIDER_KNOB))
                        .child(
                            div()
                                .absolute()
                                .left(gpui::relative(fraction))
                                .top(px((SLIDER_HEIGHT - knob) / 2.0))
                                .ml(px((SLIDER_KNOB - knob) / 2.0))
                                .size(px(knob))
                                .rounded_full()
                                .bg(rgb(WHITE))
                                .shadow(vec![ring(0.12), drop_shadow(1.0, 3.0, 0.18)]),
                        ),
                ),
        )
}

/// `useToast`'s pill: fades and rises in, holds, then fades out over 2.2 s.
pub(crate) fn toast(generation: usize, message: SharedString) -> impl IntoElement {
    div()
        .absolute()
        .bottom(px(80.0))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(
            div()
                .rounded_full()
                .bg(alpha(INK, 0.9))
                .px_4()
                .py_2()
                .text_size(px(TEXT_SM))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(WHITE))
                .shadow(vec![
                    drop_shadow(10.0, 15.0, 0.1),
                    drop_shadow(4.0, 6.0, 0.1),
                ])
                .child(message),
        )
        .with_animation(
            ("toast", generation),
            Animation::new(TOAST),
            |toast, delta| {
                let (opacity, rise) = toast_keyframe(delta);
                toast.opacity(opacity).mb(px(rise))
            },
        )
}

/// The `toast` keyframes: 0% → 12% in from 6 pt below, hold to 80%, then
/// out 4 pt upward, each segment eased with `ease_out_soft`.
fn toast_keyframe(progress: f32) -> (f32, f32) {
    let ease = ease_out_soft();
    if progress < 0.12 {
        let t = ease(progress / 0.12);
        (t, -6.0 * (1.0 - t))
    } else if progress < 0.8 {
        (1.0, 0.0)
    } else {
        let t = ease((progress - 0.8) / 0.2);
        (1.0 - t, 4.0 * t)
    }
}

pub(crate) fn settings_window_size() -> Size<Pixels> {
    size(px(560.0), px(760.0))
}

pub(crate) fn onboarding_window_size() -> Size<Pixels> {
    size(px(760.0), px(560.0))
}

/// Copy where `**`-delimited spans are bold ink, as in `<b className="…">`.
pub(crate) fn rich_text(markup: &str) -> StyledText {
    let mut text = String::new();
    let mut bold = Vec::new();
    for (index, part) in markup.split("**").enumerate() {
        let start = text.len();
        text.push_str(part);
        if index % 2 == 1 {
            bold.push((
                start..text.len(),
                HighlightStyle {
                    font_weight: Some(FontWeight::SEMIBOLD),
                    color: Some(rgb(INK).into()),
                    ..HighlightStyle::default()
                },
            ));
        }
    }
    StyledText::new(text).with_highlights(bold)
}

/// The onboarding heading: a 22 pt title over centred supporting copy.
pub(crate) fn step_title(title: &'static str, body: &'static str) -> Div {
    div()
        .mb(px(28.0))
        .flex()
        .flex_col()
        .items_center()
        .child(
            div()
                .text_size(px(TEXT_TITLE))
                .font_weight(FontWeight::SEMIBOLD)
                .child(title),
        )
        .child(
            div()
                .mt(px(6.0))
                .max_w(px(440.0))
                .text_center()
                .text_size(px(TEXT_BODY))
                .line_height(px(22.0))
                .text_color(rgb(INK_SOFT))
                .child(body),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easings_start_at_zero_and_end_at_one() {
        for easing in [
            cubic_bezier(0.22, 1.0, 0.36, 1.0),
            cubic_bezier(0.34, 1.36, 0.64, 1.0),
        ] {
            assert!(easing(0.0).abs() < 1e-4);
            assert!((easing(1.0) - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn ease_out_soft_is_monotonic_and_front_loaded() {
        let easing = ease_out_soft();
        let mut previous = 0.0;
        for step in 1..=100 {
            let value = easing(step as f32 / 100.0);
            assert!(value >= previous - 1e-5, "not monotonic at {step}");
            previous = value;
        }
        assert!(
            easing(0.25) > 0.6,
            "ease-out should cover most distance early"
        );
    }

    #[test]
    fn ease_out_soft_stays_within_gpuis_range() {
        let easing = ease_out_soft();
        for step in 0..=1000 {
            let value = easing(step as f32 / 1000.0);
            assert!((0.0..=1.0).contains(&value), "{value} at {step}");
        }
    }

    #[test]
    fn spring_overshoots_then_settles() {
        let easing = ease_spring();
        let peak = (0..=100)
            .map(|step| easing(step as f32 / 100.0))
            .fold(0.0_f32, f32::max);
        assert!(peak > 1.01, "peak {peak}");
        assert!((easing(1.0) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn linear_bezier_is_identity() {
        let linear = cubic_bezier(0.0, 0.0, 1.0, 1.0);
        for step in 0..=10 {
            let x = step as f32 / 10.0;
            assert!((linear(x) - x).abs() < 1e-3);
        }
    }

    #[test]
    fn slider_values_follow_the_knob_centre() {
        let bounds = Bounds::new(point(px(100.0), px(0.0)), size(px(218.0), px(18.0)));
        assert_eq!(slider_value(bounds, px(109.0)), 0.0);
        assert_eq!(slider_value(bounds, px(50.0)), 0.0);
        assert_eq!(slider_value(bounds, px(209.0)), 50.0);
        assert_eq!(slider_value(bounds, px(309.0)), 100.0);
        assert_eq!(slider_value(bounds, px(400.0)), 100.0);
    }

    #[test]
    fn toast_fades_in_holds_and_fades_out() {
        assert_eq!(toast_keyframe(0.0), (0.0, -6.0));
        assert_eq!(toast_keyframe(0.5), (1.0, 0.0));
        let (opacity, rise) = toast_keyframe(1.0);
        assert!(opacity.abs() < 1e-4 && (rise - 4.0).abs() < 1e-4);
    }

    #[test]
    fn mix_color_interpolates_and_clamps() {
        let from = alpha(0x000000, 0.0);
        let to = rgb(0xffffff);
        assert_eq!(mix_color(from, to, 0.5).r, 0.5);
        assert_eq!(mix_color(from, to, 2.0), to);
        assert_eq!(mix_color(from, to, -1.0), from);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use gpui::{Context, Render, TestAppContext};

    struct RowFixture;

    impl Render for RowFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w_full().child(
                settings_row(
                    "Screen Recording",
                    Some(row_description(
                        "Required to capture your screen. In development the permission belongs to the terminal that started ss, not to ss itself.",
                    )),
                    div()
                        .debug_selector(|| "settings-control".into())
                        .w(px(120.0))
                        .h(px(BUTTON_HEIGHT))
                        .flex_none(),
                )
                .debug_selector(|| "settings-row".into()),
            )
        }
    }

    #[gpui::test]
    fn settings_row_wraps_copy_without_moving_the_control(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| RowFixture);
        let mut heights = Vec::new();
        for width in [560.0, 420.0, 300.0] {
            cx.simulate_resize(size(px(width), px(400.0)));
            cx.run_until_parked();
            let row = cx.debug_bounds("settings-row").unwrap();
            let copy = cx.debug_bounds("settings-copy").unwrap();
            let control = cx.debug_bounds("settings-control").unwrap();
            assert_eq!(row.size.width, px(width));
            assert_eq!(control.size.width, px(120.0));
            assert!(control.right() <= row.right(), "{width}: control overflows");
            assert!(
                copy.right() + px(16.0) <= control.left(),
                "{width}: copy {copy:?} crowds control {control:?}"
            );
            heights.push(row.size.height);
        }
        assert!(heights[2] > heights[0], "narrow row must grow: {heights:?}");
    }

    struct SegmentedFixture;

    impl Render for SegmentedFixture {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(504.0)).child(segmented(
                "fixture",
                &[(0, "Show preview"), (1, "Open editor"), (2, "Just copy")],
                1,
                |_, _, _| {},
            ))
        }
    }

    #[gpui::test]
    fn segmented_items_share_the_track_equally(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| SegmentedFixture);
        cx.run_until_parked();
        let items: Vec<_> = (0..3)
            .map(|index| {
                let selector = ["segmented-item-0", "segmented-item-1", "segmented-item-2"][index];
                cx.debug_bounds(selector).unwrap()
            })
            .collect();
        // 500 / 3 is snapped to whole pixels, so allow one point of rounding.
        for item in &items {
            assert!((item.size.width - items[0].size.width).abs() <= px(1.0));
            assert_eq!(item.size.height, px(SEGMENT_HEIGHT));
        }
        // 504 pt track minus 2 pt padding on each side.
        assert_eq!(items[0].left(), px(2.0));
        assert_eq!(items[2].right(), px(502.0));
    }
}
