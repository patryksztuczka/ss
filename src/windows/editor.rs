//! The editor (port of `renderer/src/windows/Editor.tsx` and `openEditor`
//! in `Windows.ts`): a stage with the composition and its OCR text layer, a
//! 300 pt sidebar of style controls, export, and keyboard shortcuts.

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

#[cfg(test)]
use gpui::Entity;
use gpui::{
    Animation, AnimationExt, AnyElement, App, AppContext, AsyncApp, Bounds, Context, Div,
    FocusHandle, Focusable, FontWeight, Global, ImageSource, IntoElement, KeyDownEvent, Keystroke,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render, RenderImage,
    SharedString, Task, WeakEntity, Window, WindowBounds, WindowHandle, div, img, point,
    prelude::*, px, rgb, size,
};
use image::RgbaImage;

use crate::app::CloseWindow;
use crate::backgrounds::{self, BACKGROUNDS};
use crate::compose::{self, Layout, Source};
use crate::desktop_ui::{
    ACCENT, ButtonVariant, CANVAS, INK, INK_SOFT, MUTED, PANEL, RADIUS_SWATCH, RISE_IN,
    SECTION_GAP, SliderTrack, TEXT_2XS, TEXT_BASE, TEXT_SM, TEXT_XS, accent_soft, alpha, button,
    ease_out_soft, kbd, line, mix_color, section, segmented, slider, toast, window_frame,
    window_options,
};
use crate::ocr::TextLayout;
use crate::settings::{AspectRatio, BackgroundId, EditorStyle};
use crate::text_selection::{self, Glyph, Selection};

const SIDEBAR_WIDTH: f32 = 300.0;
const HEADER_HEIGHT: f32 = 48.0;
const FOOTER_HEIGHT: f32 = 64.0;
const STAGE_GUTTER: f32 = 56.0;
const PERSIST_DELAY: Duration = Duration::from_millis(400);
const SHIMMER: Duration = Duration::from_millis(1600);
/// A drag must travel this far before it selects text.
const SELECT_THRESHOLD: f32 = 2.0;
const SWATCH_PIXELS: u32 = 96;

const RATIOS: [(AspectRatio, &str); 5] = [
    (AspectRatio::Auto, "Auto"),
    (AspectRatio::R16x9, "16:9"),
    (AspectRatio::R4x3, "4:3"),
    (AspectRatio::R3x2, "3:2"),
    (AspectRatio::R1x1, "1:1"),
];

/// What the editor does outside itself; tests and previews substitute it.
pub trait EditorHost {
    fn write_png(&self, png: &[u8]) -> Result<(), String>;
    fn write_text(&self, text: &str) -> Result<(), String>;
    fn save_style(&self, style: &EditorStyle, cx: &mut App);
    fn images_directory(&self, cx: &App) -> PathBuf;
    fn recognize(&self, id: &str, cx: &mut App) -> Task<Result<Arc<TextLayout>, SharedString>>;
}

/// Where the screenshot comes from.
pub enum Load {
    Path(PathBuf),
    Ready(Arc<Source>, [u8; 3]),
    /// Never finishes: `ss preview editor --loading`.
    Pending,
    Failed,
}

enum Content {
    Loading,
    Failed,
    Ready { source: Arc<Source>, inset: [u8; 3] },
}

enum Ocr {
    Loading,
    Failed,
    Ready {
        layout: Arc<TextLayout>,
        glyphs: Rc<Vec<Glyph>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
struct RasterKey {
    style: EditorStyle,
    /// Raster scale in thousandths, so float noise doesn't re-render.
    scale: u32,
    checker: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SliderKind {
    Padding,
    Inset,
    Radius,
    Shadow,
}

impl SliderKind {
    const ALL: [Self; 4] = [Self::Padding, Self::Inset, Self::Radius, Self::Shadow];

    fn index(self) -> usize {
        Self::ALL.iter().position(|kind| *kind == self).unwrap_or(0)
    }
}

struct TextPress {
    anchor: usize,
    origin: Point<Pixels>,
    moved: bool,
}

/// Where things are on screen this frame, for mouse mapping.
#[derive(Clone, Copy, Debug, Default)]
struct Geometry {
    /// Top-left of the composition in window coordinates.
    origin: Point<Pixels>,
    display_scale: f32,
}

#[derive(Default)]
struct EditorWindows(HashMap<String, WindowHandle<EditorWindow>>);

impl Global for EditorWindows {}

/// Opens the editor for a capture, or focuses the one already open.
/// Previews pass `activate: false` so they don't take focus.
pub fn open(
    id: &str,
    load: Load,
    host: Rc<dyn EditorHost>,
    activate: bool,
    cx: &mut App,
) -> gpui::Result<WindowHandle<EditorWindow>> {
    if let Some(handle) = cx.default_global::<EditorWindows>().0.get(id).copied()
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(handle);
    }
    let mut options = window_options("ss", size(px(1240.0), px(800.0)), cx);
    options.is_resizable = true;
    options.is_minimizable = true;
    options.window_min_size = Some(size(px(880.0), px(560.0)));
    options.focus = activate;
    if let Some((display_id, bounds)) = placement(cx) {
        options.display_id = Some(display_id);
        options.window_bounds = Some(WindowBounds::Windowed(bounds));
    }
    let id_owned = id.to_string();
    let handle = cx.open_window(options, |window, cx| {
        cx.new(|cx| EditorWindow::new(id_owned, load, host, window, cx))
    })?;
    cx.default_global::<EditorWindows>()
        .0
        .insert(id.to_string(), handle);
    if activate {
        cx.activate(true);
    }
    Ok(handle)
}

/// `min(1240, wa.w − 80) × min(800, wa.h − 80)`, centred on the work area of
/// the display under the pointer.
fn placement(cx: &App) -> Option<(gpui::DisplayId, Bounds<Pixels>)> {
    let display = crate::displays::under_cursor().ok()?;
    let gpui_display = cx
        .displays()
        .into_iter()
        .find(|candidate| u32::from(candidate.id()) == display.number)?;
    let frame = editor_frame(display.work_area);
    // GPUI positions windows relative to their display's top-left corner.
    let bounds = Bounds::new(
        point(
            px((frame.x - display.bounds.x) as f32),
            px((frame.y - display.bounds.y) as f32),
        ),
        size(px(frame.width as f32), px(frame.height as f32)),
    );
    Some((gpui_display.id(), bounds))
}

fn editor_frame(work_area: crate::displays::Rect) -> crate::displays::Rect {
    let width = 1240f64.min(work_area.width - 80.0);
    let height = 800f64.min(work_area.height - 80.0);
    crate::displays::Rect::new(
        (work_area.x + (work_area.width - width) / 2.0).round(),
        (work_area.y + (work_area.height - height) / 2.0).round(),
        width,
        height,
    )
}

/// `displayScale = max(0.01, min((sw − 112)/lw, (sh − 112)/lh, 1))`.
fn display_scale(stage: gpui::Size<Pixels>, layout: &Layout) -> f32 {
    let width = (f32::from(stage.width) - STAGE_GUTTER * 2.0) / layout.width as f32;
    let height = (f32::from(stage.height) - STAGE_GUTTER * 2.0) / layout.height as f32;
    width.min(height).clamp(0.01, 1.0)
}

pub struct EditorWindow {
    focus: FocusHandle,
    host: Rc<dyn EditorHost>,
    content: Content,
    style: EditorStyle,
    highlight: bool,
    ocr: Ocr,
    selection: Option<Selection>,
    press: Option<TextPress>,
    raster: Option<Arc<RenderImage>>,
    requested: Option<RasterKey>,
    render: Option<Task<()>>,
    toast: Option<(usize, SharedString)>,
    toasts: usize,
    toast_timer: Option<Task<()>>,
    persist: Option<Task<()>>,
    slider: Option<SliderKind>,
    tracks: [SliderTrack; 4],
    swatches: HashMap<(BackgroundId, [u8; 3]), Arc<RenderImage>>,
    colors: Option<Task<()>>,
    geometry: Geometry,
    _load: Option<Task<()>>,
}

impl EditorWindow {
    fn new(
        id: String,
        load: Load,
        host: Rc<dyn EditorHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        let style = crate::settings::model::SettingsModel::try_global(cx)
            .map(|settings| settings.read(cx).settings().editor.clone())
            .unwrap_or_default();
        let (content, load_task) = match load {
            Load::Ready(source, inset) => (Content::Ready { source, inset }, None),
            Load::Pending => (Content::Loading, None),
            Load::Failed => (Content::Failed, None),
            Load::Path(path) => {
                let decode = cx.background_spawn(async move {
                    let source = Source::from_png(&std::fs::read(path)?)?;
                    let inset = compose::detect_edge_color(&source);
                    Ok::<_, color_eyre::Report>((Arc::new(source), inset))
                });
                let task = cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                    let result = decode.await;
                    let _ = this.update(cx, |this, cx| {
                        this.content = match result {
                            Ok((source, inset)) => Content::Ready { source, inset },
                            Err(error) => {
                                tracing::warn!(%error, "could not load the capture");
                                Content::Failed
                            }
                        };
                        cx.notify();
                    });
                });
                (Content::Loading, Some(task))
            }
        };
        let ocr = host.recognize(&id, cx);
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = ocr.await;
            let _ = this.update(cx, |this, cx| {
                this.ocr = match result {
                    Ok(layout) => {
                        let glyphs = Rc::new(text_selection::glyphs(&layout));
                        Ocr::Ready { layout, glyphs }
                    }
                    Err(error) => {
                        tracing::warn!(%error, "text recognition failed");
                        Ocr::Failed
                    }
                };
                cx.notify();
            });
        })
        .detach();
        Self {
            focus,
            host,
            content,
            style,
            highlight: false,
            ocr: Ocr::Loading,
            selection: None,
            press: None,
            raster: None,
            requested: None,
            render: None,
            toast: None,
            toasts: 0,
            toast_timer: None,
            persist: None,
            slider: None,
            tracks: Default::default(),
            swatches: HashMap::new(),
            colors: None,
            geometry: Geometry::default(),
            _load: load_task,
        }
    }

    /// `ss preview editor`: highlight, a selected line, a toast.
    pub fn show_preview_state(
        &mut self,
        highlight: bool,
        select_line: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        self.highlight = highlight;
        if let (Some(line), Ocr::Ready { glyphs, .. }) = (select_line, &self.ocr)
            && let Some(index) = glyphs.iter().position(|glyph| glyph.line == line)
        {
            self.selection = Some(Selection::line(glyphs, index));
        }
        cx.notify();
    }

    pub fn set_style(&mut self, style: EditorStyle, cx: &mut Context<Self>) {
        self.style = style;
        cx.notify();
    }

    /// Applies a style change and saves it 400 ms after the last one.
    fn update(&mut self, change: impl FnOnce(&mut EditorStyle), cx: &mut Context<Self>) {
        change(&mut self.style);
        let style = self.style.clone();
        self.persist = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor().timer(PERSIST_DELAY).await;
                let _ = this.update(cx, |this, cx| this.host.clone().save_style(&style, cx));
            }),
        );
        cx.notify();
    }

    fn show_toast(&mut self, message: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.toasts += 1;
        let generation = self.toasts;
        self.toast = Some((generation, message.into()));
        self.toast_timer = Some(cx.spawn(
            async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                cx.background_executor()
                    .timer(crate::desktop_ui::TOAST)
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this
                        .toast
                        .as_ref()
                        .is_some_and(|(current, _)| *current == generation)
                    {
                        this.toast = None;
                        cx.notify();
                    }
                });
            },
        ));
        cx.notify();
    }

    fn ready(&self) -> Option<(Arc<Source>, [u8; 3])> {
        match &self.content {
            Content::Ready { source, inset } => Some((source.clone(), *inset)),
            _ => None,
        }
    }

    fn export(&self, cx: &mut Context<Self>) -> Option<Task<color_eyre::Result<Vec<u8>>>> {
        let (source, inset) = self.ready()?;
        let style = self.style.clone();
        Some(cx.background_spawn(async move { compose::export_png(&source, &style, inset) }))
    }

    /// Copies the composition; `close` is the double-click "copy & close".
    fn copy_image(&mut self, close: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(export) = self.export(cx) else {
            return;
        };
        let handle = window.window_handle();
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let png = export.await;
            let _ = this.update(cx, |this, cx| {
                let written = png
                    .map_err(|error| error.to_string())
                    .and_then(|png| this.host.write_png(&png));
                match written {
                    Ok(()) if close => {
                        let _ = handle.update(cx, |_, window, _| window.remove_window());
                    }
                    Ok(()) => this.show_toast("Image copied", cx),
                    Err(error) => {
                        tracing::error!(%error, "could not copy the image");
                        this.show_toast("Could not copy the image", cx);
                    }
                }
            });
        })
        .detach();
    }

    fn copy_text(&mut self, text: &str, message: &'static str, cx: &mut Context<Self>) {
        match self.host.write_text(text) {
            Ok(()) => self.show_toast(message, cx),
            Err(error) => tracing::error!(%error, "could not copy text"),
        }
    }

    fn copy_all_text(&mut self, cx: &mut Context<Self>) {
        match &self.ocr {
            Ocr::Ready { layout, .. } if !layout.lines.is_empty() => {
                let text = layout.text();
                self.copy_text(&text, "All text copied", cx);
            }
            _ => self.show_toast("No text found", cx),
        }
    }

    /// ⌘C: the selected text if there is any, otherwise the image.
    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = match (&self.ocr, self.selection) {
            (Ocr::Ready { glyphs, .. }, Some(selection)) => {
                text_selection::selected_text(glyphs, selection)
            }
            _ => String::new(),
        };
        if selected.is_empty() {
            self.copy_image(false, window, cx);
        } else {
            self.copy_text(&selected, "Text copied", cx);
        }
    }

    /// ⌘S writes `ss <date> at <time>.png` to the images folder; ⇧⌘S asks.
    fn save(&mut self, dialog: bool, cx: &mut Context<Self>) {
        let Some(export) = self.export(cx) else {
            return;
        };
        let directory = self.host.images_directory(cx);
        let name = format!(
            "{}.png",
            crate::capture_store::timestamp_name("ss", &jiff::Zoned::now())
        );
        let chosen = dialog.then(|| {
            let _ = std::fs::create_dir_all(&directory);
            cx.prompt_for_new_path(&directory, Some(&name))
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let path = match chosen {
                None => Some(directory.join(&name)),
                Some(chosen) => match chosen.await {
                    Ok(Ok(Some(path))) => Some(with_png_extension(path)),
                    _ => None,
                },
            };
            let Some(path) = path else {
                return;
            };
            let png = export.await;
            let written = cx
                .background_spawn({
                    let path = path.clone();
                    async move {
                        let png = png?;
                        if let Some(parent) = path.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&path, png)?;
                        Ok::<_, color_eyre::Report>(())
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| match written {
                Ok(()) => this.show_toast(format!("Saved to {}", crate::paths::tildify(&path)), cx),
                Err(error) => {
                    tracing::error!(%error, "could not save the image");
                    this.show_toast("Could not save the image", cx);
                }
            });
        })
        .detach();
    }

    /// Window shortcuts; returns whether the key was handled.
    fn handle_key(
        &mut self,
        keystroke: &Keystroke,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let modifiers = &keystroke.modifiers;
        let key = keystroke.key.as_str();
        if modifiers.platform {
            match key {
                "c" if modifiers.shift => self.copy_all_text(cx),
                "c" => self.copy(window, cx),
                "s" => self.save(modifiers.shift, cx),
                _ => return false,
            }
            return true;
        }
        if modifiers.alt || modifiers.control {
            return false;
        }
        match key {
            "escape" => {
                if self.selection.take().is_some() {
                    cx.notify();
                } else {
                    window.remove_window();
                }
            }
            "h" => {
                self.highlight = !self.highlight;
                cx.notify();
            }
            "r" => {
                let index = RATIOS
                    .iter()
                    .position(|(ratio, _)| *ratio == self.style.ratio)
                    .unwrap_or(0);
                let next = RATIOS[(index + 1) % RATIOS.len()].0;
                self.update(|style| style.ratio = next, cx);
            }
            "[" | "]" => {
                let delta = if key == "[" { -4.0 } else { 4.0 };
                self.update(
                    |style| style.padding = (style.padding + delta).clamp(0.0, 100.0),
                    cx,
                );
            }
            _ => match "1234567890".find(key).filter(|_| key.len() == 1) {
                Some(index) if index < BACKGROUNDS.len() => {
                    let id = BACKGROUNDS[index].id;
                    self.update(|style| style.background = id, cx);
                }
                _ => return false,
            },
        }
        true
    }

    fn slider_value(&self, kind: SliderKind) -> f64 {
        match kind {
            SliderKind::Padding => self.style.padding,
            SliderKind::Inset => self.style.inset,
            SliderKind::Radius => self.style.radius,
            SliderKind::Shadow => self.style.shadow,
        }
    }

    fn set_slider(&mut self, kind: SliderKind, x: Pixels, cx: &mut Context<Self>) {
        let Some(value) = self.tracks[kind.index()].value_at(x) else {
            return;
        };
        if value == self.slider_value(kind) {
            return;
        }
        self.update(
            |style| match kind {
                SliderKind::Padding => style.padding = value,
                SliderKind::Inset => style.inset = value,
                SliderKind::Radius => style.radius = value,
                SliderKind::Shadow => style.shadow = value,
            },
            cx,
        );
    }

    /// A point in window coordinates → image pixels.
    fn image_point(&self, position: Point<Pixels>) -> Option<(f64, f64)> {
        let (source, _) = self.ready()?;
        let layout = compose::compute_layout(&self.style, source.width(), source.height());
        let geometry = self.geometry;
        let scale = f64::from(geometry.display_scale);
        let x =
            f64::from(f32::from(position.x - geometry.origin.x)) / scale - layout.image.x as f64;
        let y =
            f64::from(f32::from(position.y - geometry.origin.y)) / scale - layout.image.y as f64;
        Some((x, y))
    }

    fn press_text(&mut self, event: &MouseDownEvent, cx: &mut Context<Self>) {
        let glyphs = match &self.ocr {
            Ocr::Ready { glyphs, .. } => glyphs.clone(),
            _ => return,
        };
        let hit = self
            .image_point(event.position)
            .and_then(|(x, y)| text_selection::hit_test(&glyphs, x, y));
        self.press = None;
        self.selection = match (hit, event.click_count) {
            (Some(index), 2) => Some(Selection::word(index)),
            (Some(index), count) if count >= 3 => Some(Selection::line(&glyphs, index)),
            (Some(index), _) => {
                self.press = Some(TextPress {
                    anchor: index,
                    origin: event.position,
                    moved: false,
                });
                None
            }
            (None, _) => None,
        };
        cx.notify();
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_drag(cx);
            return;
        }
        if let Some(kind) = self.slider {
            self.set_slider(kind, event.position.x, cx);
            return;
        }
        let Some(press) = &mut self.press else {
            return;
        };
        let delta = event.position - press.origin;
        if !press.moved && f32::from(delta.x).hypot(f32::from(delta.y)) < SELECT_THRESHOLD {
            return;
        }
        press.moved = true;
        let anchor = press.anchor;
        let (Ocr::Ready { glyphs, .. }, Some((x, y))) =
            (&self.ocr, self.image_point(event.position))
        else {
            return;
        };
        if let Some(focus) = text_selection::nearest(glyphs, x, y) {
            self.selection = Some(Selection::new(anchor, focus));
            cx.notify();
        }
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.slider.take().is_some() || self.press.take().is_some() {
            cx.notify();
        }
    }

    fn pick_background(&mut self, id: BackgroundId, cx: &mut Context<Self>) {
        // Clicking Custom again opens the colour picker.
        if id == BackgroundId::Custom && self.style.background == BackgroundId::Custom {
            self.open_color_panel(cx);
        }
        self.update(|style| style.background = id, cx);
    }

    fn open_color_panel(&mut self, cx: &mut Context<Self>) {
        let initial = backgrounds::parse_color(&self.style.custom_color);
        let Some(mut colors) = crate::color_panel::open(initial) else {
            return;
        };
        self.colors = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                use futures::StreamExt;
                while let Some(hex) = colors.next().await {
                    let updated = this.update(cx, |this, cx| {
                        this.update(
                            |style| {
                                style.background = BackgroundId::Custom;
                                style.custom_color = hex;
                            },
                            cx,
                        )
                    });
                    if updated.is_err() {
                        break;
                    }
                }
            }),
        );
    }

    /// Renders the stage raster in the background; the latest request wins
    /// and the previous raster stays on screen meanwhile.
    fn request_raster(&mut self, display_scale: f32, scale_factor: f32, cx: &mut Context<Self>) {
        let Some((source, inset)) = self.ready() else {
            return;
        };
        let raster_scale = (display_scale * scale_factor).min(1.0);
        // The checkerboard's 8 pt squares, in raster pixels.
        let checker = (8.0 * raster_scale / display_scale).round().max(1.0) as u32;
        let key = RasterKey {
            style: self.style.clone(),
            scale: (raster_scale * 1000.0).round() as u32,
            checker,
        };
        if self.requested.as_ref() == Some(&key) {
            return;
        }
        self.requested = Some(key.clone());
        let task = cx.background_spawn(async move {
            let mut image =
                compose::render_preview(&source, &key.style, inset, f64::from(key.scale) / 1000.0);
            if key.style.background == BackgroundId::None {
                image = over_checkerboard(&image, key.checker, (0xe9e6e2, 0xf4f2ef));
            }
            (key, crate::thumbnail::render_image(image))
        });
        self.render = Some(
            cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let (key, image) = task.await;
                let _ = this.update(cx, |this, cx| {
                    if this.requested.as_ref() == Some(&key) {
                        this.raster = Some(image);
                        cx.notify();
                    }
                });
            }),
        );
    }

    fn swatch(&mut self, id: BackgroundId) -> Arc<RenderImage> {
        let custom = backgrounds::parse_color(&self.style.custom_color);
        let key = (
            id,
            if id == BackgroundId::Custom {
                custom
            } else {
                [0; 3]
            },
        );
        let custom_color = self.style.custom_color.clone();
        self.swatches
            .entry(key)
            .or_insert_with(|| {
                let image = match id {
                    BackgroundId::None => {
                        let clear = RgbaImage::new(SWATCH_PIXELS, SWATCH_PIXELS);
                        over_checkerboard(&clear, 12, (0xe7e4e0, 0xfbfaf8))
                    }
                    _ => {
                        let mut pixmap =
                            tiny_skia::Pixmap::new(SWATCH_PIXELS, SWATCH_PIXELS).expect("size");
                        backgrounds::paint(&mut pixmap, backgrounds::by_id(id), &custom_color);
                        RgbaImage::from_raw(SWATCH_PIXELS, SWATCH_PIXELS, pixmap.take())
                            .expect("sized buffer")
                    }
                };
                crate::thumbnail::render_image(image)
            })
            .clone()
    }

    fn render_loading(&self) -> AnyElement {
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .flex_1()
                    .m(px(STAGE_GUTTER))
                    .rounded(px(16.0))
                    .bg(rgb(0xece9e5))
                    .with_animation(
                        "shimmer",
                        Animation::new(SHIMMER).repeat(),
                        |stage, delta| {
                            // A pulse between the two shimmer colours stands in for the moving gradient.
                            let wave = 1.0 - (delta * 2.0 - 1.0).abs();
                            stage.bg(mix_color(rgb(0xece9e5), rgb(0xf4f2ef), wave))
                        },
                    ),
            )
            .child(
                div()
                    .w(px(SIDEBAR_WIDTH))
                    .h_full()
                    .flex_none()
                    .border_l_1()
                    .border_color(line())
                    .bg(rgb(PANEL)),
            )
            .into_any_element()
    }

    fn render_stage(
        &mut self,
        source: &Source,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let layout = compose::compute_layout(&self.style, source.width(), source.height());
        let viewport = window.viewport_size();
        let stage = size(
            viewport.width - px(SIDEBAR_WIDTH),
            viewport.height - px(HEADER_HEIGHT + FOOTER_HEIGHT),
        );
        let scale = display_scale(stage, &layout);
        let composition = size(
            px(layout.width as f32 * scale),
            px(layout.height as f32 * scale),
        );
        let local = point(
            (stage.width - composition.width) / 2.0,
            (stage.height - composition.height) / 2.0,
        );
        self.geometry = Geometry {
            origin: point(local.x, local.y + px(HEADER_HEIGHT)),
            display_scale: scale,
        };
        self.request_raster(scale, window.scale_factor(), cx);

        let glyph_layer = match &self.ocr {
            Ocr::Ready { glyphs, .. } => {
                let selection = self.selection;
                let highlight = self.highlight;
                let (image_x, image_y) =
                    (layout.image.x as f32 * scale, layout.image.y as f32 * scale);
                Some(glyphs.iter().enumerate().map(move |(index, glyph)| {
                    let selected = selection.is_some_and(|selection| selection.contains(index));
                    div()
                        .absolute()
                        .left(px(image_x + glyph.x as f32 * scale))
                        .top(px(image_y + glyph.y as f32 * scale))
                        .w(px(glyph.width as f32 * scale))
                        .h(px(glyph.height as f32 * scale))
                        .rounded(px(glyph.height as f32 * scale * 0.15))
                        .cursor_text()
                        .when(highlight && !selected, |glyph| {
                            glyph.bg(alpha(ACCENT, 0.13))
                        })
                        .when(selected, |glyph| glyph.bg(alpha(ACCENT, 0.32)))
                }))
            }
            _ => None,
        };

        let composition_element = div()
            .id("composition")
            .debug_selector(|| "composition".into())
            .absolute()
            .left(local.x)
            .top(local.y)
            .w(composition.width)
            .h(composition.height)
            .when_some(self.raster.clone(), |element, raster| {
                element.child(img(ImageSource::Render(raster)).size_full())
            })
            .children(glyph_layer.into_iter().flatten())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.press_text(event, cx);
                }),
            )
            .with_animation(
                "rise-in",
                Animation::new(RISE_IN).with_easing(ease_out_soft()),
                |element, delta| element.opacity(delta).mt(px(8.0 * (1.0 - delta))),
            );

        div()
            .id("stage")
            .debug_selector(|| "stage".into())
            .relative()
            .flex_1()
            .min_h_0()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    // Double-clicking the backdrop (not the image) copies and closes.
                    if event.click_count == 2 {
                        this.copy_image(true, window, cx);
                    } else if this.selection.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(composition_element)
            .into_any_element()
    }

    fn render_footer(&self, cx: &mut Context<Self>, window: &Window) -> Div {
        let zoom = (self.geometry.display_scale * window.scale_factor() * 100.0).round();
        div()
            .h(px(FOOTER_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .gap_2()
            .px_5()
            .child(
                button("copy", "Copy", ButtonVariant::Primary, Some("⌘C".into()))
                    .debug_selector(|| "footer-copy".into())
                    .on_click(
                        cx.listener(|this, _, window, cx| this.copy_image(false, window, cx)),
                    ),
            )
            .child(
                button("save", "Save", ButtonVariant::Ghost, Some("⌘S".into()))
                    .debug_selector(|| "footer-save".into())
                    .on_click(cx.listener(|this, _, _, cx| this.save(false, cx))),
            )
            .child(
                button(
                    "save-as",
                    "Save As…",
                    ButtonVariant::Ghost,
                    Some("⇧⌘S".into()),
                )
                .on_click(cx.listener(|this, _, _, cx| this.save(true, cx))),
            )
            // A spacer, not `ml_auto`: GPUI's layout drops the row's gaps
            // when an auto margin shares a row inside a flex column.
            .child(div().flex_1())
            .child(
                div()
                    .text_size(px(TEXT_SM))
                    .text_color(rgb(MUTED))
                    .child(format!(
                        "Double-click the background to copy & close · {zoom}%"
                    )),
            )
    }

    fn render_sidebar(&mut self, inset: [u8; 3], cx: &mut Context<Self>) -> AnyElement {
        let slider_row = |this: &Self,
                          kind: SliderKind,
                          label: &'static str,
                          hint: Option<AnyElement>,
                          cx: &mut Context<Self>| {
            slider(
                ("slider", kind.index()),
                label,
                this.slider_value(kind),
                this.slider == Some(kind),
                hint,
                &this.tracks[kind.index()],
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.slider = Some(kind);
                    this.set_slider(kind, event.position.x, cx);
                }),
            )
        };
        let edge_hint = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .text_size(px(TEXT_XS))
            .text_color(rgb(MUTED))
            .child("edge color")
            .child(
                div()
                    .size(px(12.0))
                    .rounded_full()
                    .border_1()
                    .border_color(alpha(0x000000, 0.2))
                    .bg(gpui::Rgba {
                        r: f32::from(inset[0]) / 255.0,
                        g: f32::from(inset[1]) / 255.0,
                        b: f32::from(inset[2]) / 255.0,
                        a: 1.0,
                    }),
            )
            .into_any_element();

        let spacing = section("Spacing", None)
            .gap(px(14.0))
            .child(slider_row(self, SliderKind::Padding, "Padding", None, cx))
            .child(slider_row(
                self,
                SliderKind::Inset,
                "Inset",
                Some(edge_hint),
                cx,
            ));
        let card = section("Card", None).child(
            div()
                .flex()
                .gap_4()
                .child(div().flex_1().child(slider_row(
                    self,
                    SliderKind::Radius,
                    "Radius",
                    None,
                    cx,
                )))
                .child(div().flex_1().child(slider_row(
                    self,
                    SliderKind::Shadow,
                    "Shadow",
                    None,
                    cx,
                ))),
        );

        let swatches: Vec<AnyElement> = BACKGROUNDS
            .iter()
            .enumerate()
            .map(|(index, background)| {
                let selected = self.style.background == background.id;
                let image = self.swatch(background.id);
                let id = background.id;
                let plus_dark = backgrounds::parse_color(&self.style.custom_color)
                    .iter()
                    .map(|&channel| u32::from(channel))
                    .sum::<u32>()
                    > 382;
                div()
                    .id(("swatch", index))
                    .debug_selector(move || format!("swatch-{index}"))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(6.0))
                    .on_click(cx.listener(move |this, _, _, cx| this.pick_background(id, cx)))
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(44.0))
                            .rounded(px(RADIUS_SWATCH))
                            .border_1()
                            .border_color(alpha(0x000000, 0.08))
                            .when(selected, |swatch| {
                                swatch.shadow(vec![
                                    gpui::BoxShadow {
                                        color: rgb(PANEL).into(),
                                        offset: point(px(0.0), px(0.0)),
                                        blur_radius: px(0.0),
                                        spread_radius: px(2.0),
                                    },
                                    gpui::BoxShadow {
                                        color: rgb(ACCENT).into(),
                                        offset: point(px(0.0), px(0.0)),
                                        blur_radius: px(0.0),
                                        spread_radius: px(4.0),
                                    },
                                ])
                            })
                            .child(
                                img(ImageSource::Render(image))
                                    .size_full()
                                    .rounded(px(RADIUS_SWATCH)),
                            )
                            .when(id == BackgroundId::Custom, |swatch| {
                                swatch.child(
                                    div()
                                        .absolute()
                                        .inset_0()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_size(px(15.0))
                                        .text_color(rgb(if plus_dark { INK } else { 0xffffff }))
                                        .child("+"),
                                )
                            }),
                    )
                    .child(
                        div()
                            .text_size(px(TEXT_2XS))
                            .when(selected, |label| label.font_weight(FontWeight::MEDIUM))
                            .text_color(rgb(if selected { INK } else { MUTED }))
                            .child(background.label),
                    )
                    .into_any_element()
            })
            .collect();
        let background_section = section("Background", Some(kbd("1 – 0").into_any_element()))
            .child(
                div()
                    .grid()
                    .grid_cols(5)
                    .gap_x_2()
                    .gap_y_3()
                    .children(swatches),
            );

        let this = cx.entity().downgrade();
        let size_section = section("Size", Some(kbd("R").into_any_element())).child(segmented(
            "ratio",
            &RATIOS,
            self.style.ratio,
            move |ratio, _, cx| {
                let _ = this.update(cx, |this, cx| this.update(|style| style.ratio = ratio, cx));
            },
        ));

        let (status, loading, lines) = match &self.ocr {
            Ocr::Loading => ("Recognizing text…".to_string(), true, 0),
            Ocr::Failed => ("Text recognition unavailable".to_string(), false, 0),
            Ocr::Ready { layout, .. } if layout.lines.is_empty() => {
                ("No text found".to_string(), false, 0)
            }
            Ocr::Ready { layout, .. } => {
                let count = layout.lines.len();
                (
                    format!(
                        "{count} line{} · select it right on the image",
                        if count == 1 { "" } else { "s" }
                    ),
                    false,
                    count,
                )
            }
        };
        let status = div()
            .text_size(px(TEXT_SM))
            .line_height(px(19.5))
            .text_color(rgb(INK_SOFT))
            .child(status);
        let status = if loading {
            status
                .with_animation(
                    "ocr-shimmer",
                    Animation::new(SHIMMER).repeat(),
                    |text, delta| {
                        let wave = 1.0 - (delta * 2.0 - 1.0).abs();
                        text.text_color(mix_color(rgb(MUTED), rgb(0xd6d2cd), wave))
                    },
                )
                .into_any_element()
        } else {
            status.into_any_element()
        };
        let enabled = lines > 0;
        let text_section = section("Text", None).child(status).child(
            div()
                .flex()
                .gap_2()
                .child(
                    button(
                        "copy-all",
                        "Copy all",
                        ButtonVariant::Ghost,
                        Some("⇧⌘C".into()),
                    )
                    .flex_1()
                    .justify_center()
                    .when(!enabled, |button| button.opacity(0.4))
                    .when(enabled, |button| {
                        button.on_click(cx.listener(|this, _, _, cx| this.copy_all_text(cx)))
                    }),
                )
                .child(
                    button("show-text", "Show", ButtonVariant::Ghost, Some("H".into()))
                        .justify_center()
                        .when(self.highlight, |button| {
                            button.bg(accent_soft()).text_color(rgb(ACCENT))
                        })
                        .when(!enabled, |button| button.opacity(0.4))
                        .when(enabled, |button| {
                            button.on_click(cx.listener(|this, _, _, cx| {
                                this.highlight = !this.highlight;
                                cx.notify();
                            }))
                        }),
                ),
        );

        div()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .border_l_1()
            .border_color(line())
            .bg(rgb(PANEL))
            .child(div().h(px(HEADER_HEIGHT)).flex_none())
            .child(
                div()
                    .id("sidebar")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_5()
                    .pb_6()
                    .flex()
                    .flex_col()
                    .gap(px(SECTION_GAP))
                    .child(spacing)
                    .child(card)
                    .child(background_section)
                    .child(size_section)
                    .child(text_section),
            )
            .into_any_element()
    }
}

/// Composites `image` over a two-colour checkerboard of `square` pixels.
fn over_checkerboard(image: &RgbaImage, square: u32, (light, dark): (u32, u32)) -> RgbaImage {
    let channel = |hex: u32, shift: u32| ((hex >> shift) & 255) as f32;
    RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        let tile = if ((x / square) + (y / square)).is_multiple_of(2) {
            light
        } else {
            dark
        };
        let [r, g, b, a] = image.get_pixel(x, y).0;
        let alpha = f32::from(a) / 255.0;
        let mix = |top: u8, shift: u32| {
            (f32::from(top) * alpha + channel(tile, shift) * (1.0 - alpha)).round() as u8
        };
        image::Rgba([mix(r, 16), mix(g, 8), mix(b, 0), 255])
    })
}

/// Save As may drop the extension; the file is a PNG either way.
fn with_png_extension(path: PathBuf) -> PathBuf {
    match path.extension() {
        Some(extension) if extension.eq_ignore_ascii_case("png") => path,
        _ => {
            let mut name = path
                .file_name()
                .map(|name| name.to_os_string())
                .unwrap_or_default();
            name.push(".png");
            path.with_file_name(name)
        }
    }
}

impl Focusable for EditorWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for EditorWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = window_frame()
            .flex_row()
            .bg(rgb(CANVAS))
            .track_focus(&self.focus)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.handle_key(&event.keystroke, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_move(
                cx.listener(|this, event: &MouseMoveEvent, _, cx| this.mouse_move(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_drag(cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.end_drag(cx)),
            );

        let body = match &self.content {
            Content::Loading => self.render_loading(),
            Content::Failed => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(TEXT_BASE))
                .text_color(rgb(INK_SOFT))
                .child("This capture is no longer available.")
                .into_any_element(),
            Content::Ready { source, inset } => {
                let (source, inset) = (source.clone(), *inset);
                let stage = self.render_stage(&source, window, cx);
                let footer = self.render_footer(cx, window);
                let sidebar = self.render_sidebar(inset, cx);
                div()
                    .size_full()
                    .flex()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(HEADER_HEIGHT))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(TEXT_SM))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(MUTED))
                                    .child(format!("{} × {}", source.width(), source.height())),
                            )
                            .child(stage)
                            .child(footer),
                    )
                    .child(sidebar)
                    .into_any_element()
            }
        };
        frame
            .child(body)
            .when_some(self.toast.clone(), |frame, (generation, message)| {
                frame.child(toast(generation, message))
            })
    }
}

/// The editor in the running app.
pub struct AppEditorHost {
    pub default_images: PathBuf,
}

impl EditorHost for AppEditorHost {
    fn write_png(&self, png: &[u8]) -> Result<(), String> {
        crate::clipboard::write_png(png).map_err(|error| error.to_string())
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        crate::clipboard::write_text(text).map_err(|error| error.to_string())
    }

    fn save_style(&self, style: &EditorStyle, cx: &mut App) {
        let settings = crate::settings::model::SettingsModel::global(cx);
        let result = settings.update(cx, |model, cx| {
            model.update(
                crate::settings::SettingsPatch {
                    editor: Some(style.clone()),
                    ..Default::default()
                },
                cx,
            )
        });
        if let Err(error) = result {
            tracing::error!(%error, "could not save the editor style");
        }
    }

    fn images_directory(&self, cx: &App) -> PathBuf {
        crate::settings::model::SettingsModel::global(cx)
            .read(cx)
            .settings()
            .images_directory
            .as_ref()
            .map_or_else(|| self.default_images.clone(), PathBuf::from)
    }

    fn recognize(&self, id: &str, cx: &mut App) -> Task<Result<Arc<TextLayout>, SharedString>> {
        match crate::actions::recognize(id, cx) {
            Some(task) => cx.foreground_executor().spawn(task),
            None => Task::ready(Err("This capture is no longer available.".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui::{TestAppContext, VisualTestContext};

    use super::*;
    use crate::ocr::{TextBox, TextLine};

    #[derive(Default)]
    struct FakeHost {
        pngs: RefCell<Vec<usize>>,
        texts: RefCell<Vec<String>>,
        saved: RefCell<Vec<EditorStyle>>,
    }

    impl EditorHost for FakeHost {
        fn write_png(&self, png: &[u8]) -> Result<(), String> {
            self.pngs.borrow_mut().push(png.len());
            Ok(())
        }

        fn write_text(&self, text: &str) -> Result<(), String> {
            self.texts.borrow_mut().push(text.into());
            Ok(())
        }

        fn save_style(&self, style: &EditorStyle, _: &mut App) {
            self.saved.borrow_mut().push(style.clone());
        }

        fn images_directory(&self, _: &App) -> PathBuf {
            std::env::temp_dir()
        }

        fn recognize(&self, _: &str, _: &mut App) -> Task<Result<Arc<TextLayout>, SharedString>> {
            let word = |text: &str, x: f64| TextBox {
                text: text.into(),
                x,
                y: 0.1,
                w: 0.2,
                h: 0.1,
            };
            Task::ready(Ok(Arc::new(TextLayout {
                width: 120.0,
                height: 80.0,
                lines: vec![TextLine {
                    text: "Hello world".into(),
                    x: 0.1,
                    y: 0.1,
                    w: 0.6,
                    h: 0.1,
                    words: vec![word("Hello", 0.1), word("world", 0.4)],
                }],
            })))
        }
    }

    fn open(
        cx: &mut TestAppContext,
    ) -> (Rc<FakeHost>, Entity<EditorWindow>, &mut VisualTestContext) {
        let host = Rc::new(FakeHost::default());
        let source = Arc::new(Source::from_rgba(RgbaImage::from_pixel(
            120,
            80,
            image::Rgba([40, 80, 120, 255]),
        )));
        let editor_host: Rc<dyn EditorHost> = host.clone();
        let (view, cx) = cx.add_window_view(|window, cx| {
            EditorWindow::new(
                "a".into(),
                Load::Ready(source, [255, 255, 255]),
                editor_host,
                window,
                cx,
            )
        });
        cx.simulate_resize(size(px(1240.0), px(800.0)));
        cx.run_until_parked();
        (host, view, cx)
    }

    fn style(view: &Entity<EditorWindow>, cx: &mut VisualTestContext) -> EditorStyle {
        view.read_with(cx, |view, _| view.style.clone())
    }

    fn key(cx: &mut VisualTestContext, keys: &str) {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
    }

    #[gpui::test]
    fn digits_ratio_and_brackets_edit_the_style(cx: &mut TestAppContext) {
        let (_, view, cx) = open(cx);
        key(cx, "2");
        assert_eq!(style(&view, cx).background, BackgroundId::Cool);
        key(cx, "0");
        assert_eq!(style(&view, cx).background, BackgroundId::Custom);
        key(cx, "9");
        assert_eq!(style(&view, cx).background, BackgroundId::None);

        key(cx, "r");
        assert_eq!(style(&view, cx).ratio, AspectRatio::R16x9);
        key(cx, "r r r r");
        assert_eq!(style(&view, cx).ratio, AspectRatio::Auto);

        key(cx, "[");
        assert_eq!(style(&view, cx).padding, 32.0);
        key(cx, "] ] ]");
        assert_eq!(style(&view, cx).padding, 44.0);
        view.update(cx, |view, _| view.style.padding = 98.0);
        key(cx, "]");
        assert_eq!(style(&view, cx).padding, 100.0);
        view.update(cx, |view, _| view.style.padding = 2.0);
        key(cx, "[");
        assert_eq!(style(&view, cx).padding, 0.0);

        // Other modifiers are not shortcuts.
        key(cx, "alt-2");
        assert_eq!(style(&view, cx).background, BackgroundId::None);
    }

    #[gpui::test]
    fn h_toggles_highlight_and_escape_clears_then_closes(cx: &mut TestAppContext) {
        let (_, view, cx) = open(cx);
        key(cx, "h");
        assert!(view.read_with(cx, |view, _| view.highlight));
        key(cx, "h");
        assert!(!view.read_with(cx, |view, _| view.highlight));

        view.update(cx, |view, _| view.selection = Some(Selection::word(0)));
        let window = cx.window_handle();
        key(cx, "escape");
        assert_eq!(view.read_with(cx, |view, _| view.selection), None);
        assert!(cx.cx.windows().contains(&window));
        key(cx, "escape");
        assert!(!cx.cx.windows().contains(&window));
    }

    #[gpui::test]
    fn command_c_copies_the_selection_or_the_image(cx: &mut TestAppContext) {
        let (host, view, cx) = open(cx);
        view.update(cx, |view, _| view.selection = Some(Selection::new(1, 0)));
        key(cx, "cmd-c");
        assert_eq!(*host.texts.borrow(), ["Hello world"]);
        assert!(host.pngs.borrow().is_empty());
        let toast = view.read_with(cx, |view, _| {
            view.toast.clone().map(|(_, message)| message.to_string())
        });
        assert_eq!(toast.as_deref(), Some("Text copied"));

        view.update(cx, |view, _| view.selection = None);
        key(cx, "cmd-c");
        assert_eq!(host.pngs.borrow().len(), 1, "no selection copies the image");
        let toast = view.read_with(cx, |view, _| {
            view.toast.clone().map(|(_, message)| message.to_string())
        });
        assert_eq!(toast.as_deref(), Some("Image copied"));

        key(cx, "cmd-shift-c");
        assert_eq!(
            host.texts.borrow().last().map(String::as_str),
            Some("Hello world")
        );
    }

    #[gpui::test]
    fn style_changes_persist_400ms_after_the_last_one(cx: &mut TestAppContext) {
        let (host, view, cx) = open(cx);
        key(cx, "]");
        cx.executor().advance_clock(Duration::from_millis(300));
        key(cx, "]");
        cx.executor().advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        assert!(
            host.saved.borrow().is_empty(),
            "still within 400 ms of the last change"
        );
        cx.executor().advance_clock(Duration::from_millis(150));
        cx.run_until_parked();
        let saved = host.saved.borrow();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].padding, style(&view, cx).padding);
    }

    #[gpui::test]
    fn one_window_per_capture(cx: &mut TestAppContext) {
        let host: Rc<dyn EditorHost> = Rc::new(FakeHost::default());
        let source = Arc::new(Source::from_rgba(RgbaImage::new(10, 10)));
        let first = cx.update(|cx| open_window(cx, "a", source.clone(), host.clone()));
        let again = cx.update(|cx| open_window(cx, "a", source.clone(), host.clone()));
        let other = cx.update(|cx| open_window(cx, "b", source.clone(), host.clone()));
        assert_eq!(first, again);
        assert_ne!(first, other);
        assert_eq!(cx.windows().len(), 2);
    }

    fn open_window(
        cx: &mut App,
        id: &str,
        source: Arc<Source>,
        host: Rc<dyn EditorHost>,
    ) -> WindowHandle<EditorWindow> {
        super::open(id, Load::Ready(source, [0; 3]), host, false, cx).unwrap()
    }

    #[gpui::test]
    fn footer_buttons_are_spaced(cx: &mut TestAppContext) {
        let (_, _, cx) = open(cx);
        let copy = cx.debug_bounds("footer-copy").unwrap();
        let save = cx.debug_bounds("footer-save").unwrap();
        assert!(save.left() - copy.right() >= px(7.9), "{copy:?} {save:?}");
    }

    #[test]
    fn display_scale_fits_the_stage_with_a_gutter() {
        let layout = compose::compute_layout(&EditorStyle::default(), 1601, 948);
        // Stage 940 × 688: min((940 − 112) / 1813, (688 − 112) / 1160, 1).
        let scale = display_scale(size(px(940.0), px(688.0)), &layout);
        assert!((scale - (828.0 / 1813.0f32).min(576.0 / 1160.0)).abs() < 1e-6);
        let small = compose::compute_layout(&EditorStyle::default(), 100, 50);
        assert_eq!(display_scale(size(px(940.0), px(688.0)), &small), 1.0);
    }

    #[test]
    fn editor_window_is_centred_and_bounded() {
        use crate::displays::Rect;
        assert_eq!(
            editor_frame(Rect::new(0.0, 33.0, 1512.0, 856.0)),
            Rect::new(136.0, 73.0, 1240.0, 776.0)
        );
        assert_eq!(
            editor_frame(Rect::new(-1000.0, 0.0, 1000.0, 700.0)),
            Rect::new(-960.0, 40.0, 920.0, 620.0)
        );
    }

    #[test]
    fn save_as_forces_png() {
        assert_eq!(
            with_png_extension("/a/shot".into()),
            PathBuf::from("/a/shot.png")
        );
        assert_eq!(
            with_png_extension("/a/shot.PNG".into()),
            PathBuf::from("/a/shot.PNG")
        );
        assert_eq!(
            with_png_extension("/a/shot.jpg".into()),
            PathBuf::from("/a/shot.jpg.png")
        );
    }
}
