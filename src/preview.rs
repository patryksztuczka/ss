//! `ss preview …`: one window with fixture data for UI work and screenshots
//! (debug builds only, like HEX's `preview`). Uses its own temp data folder
//! and a fake setup backend, so it never touches macOS state or real data.

use std::path::Path;
use std::time::Duration;

use clap::ValueEnum;
use color_eyre::eyre::{Result, eyre};
use std::rc::Rc;

use gpui::{App, Application, AsyncApp, SharedString, Task};
use objc2_app_kit::{NSApplication, NSFloatingWindowLevel};
use objc2_foundation::MainThreadMarker;

use crate::capture_store::CaptureKind;
use crate::paths::Paths;
use crate::settings::ShortcutAction;
use crate::settings::model::SettingsModel;
use crate::setup::{FakeSetup, Setup};
use crate::system::ScreenAccess;
use crate::windows::onboarding::{self, Step};
use crate::windows::preview::{self, PreviewHost, PreviewPayload};

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Target {
    Settings,
    Onboarding,
    /// The capture preview card.
    Preview,
    /// The editor, on ss's onboarding screenshot and its OCR fixture.
    Editor,
    /// The record-area overlay on the display under the pointer.
    Region,
    /// The menu bar item showing a running recording's timer.
    TrayRecording,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum PreviewStep {
    Welcome,
    Permissions,
    Shortcuts,
    Done,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
pub enum Scroll {
    Top,
    Bottom,
}

pub struct Options {
    pub step: PreviewStep,
    pub scroll: Scroll,
    /// Screen Recording reported as granted.
    pub granted: bool,
    /// Shortcuts that failed to register (two of them).
    pub failed: bool,
    /// Settings: a shortcut row recording, with the ⌥-only hint.
    pub recording: bool,
    /// Preview: a recording instead of a screenshot.
    pub video: bool,
    /// Preview: hover controls showing.
    pub hover: bool,
    /// Preview: the status chip text.
    pub chip: Option<String>,
    /// Editor: background id ("sky", "none", …).
    pub background: Option<String>,
    /// Editor: aspect ratio ("auto", "16:9", …).
    pub ratio: Option<String>,
    /// Editor: highlight the recognised text.
    pub highlight: bool,
    /// Editor: select this OCR line.
    pub select: Option<usize>,
    /// Editor: stay in the loading state.
    pub loading: bool,
    /// Editor: show the "no longer available" state.
    pub load_failed: bool,
    /// Region: "x,y,w,h" of a selection in progress.
    pub drag: Option<String>,
}

pub fn run(target: Target, options: Options) -> Result<()> {
    let data = std::env::temp_dir().join(format!("ss-preview-{}", std::process::id()));
    std::fs::create_dir_all(&data)?;
    let paths = Paths::new(
        data,
        Path::new("/Users/preview/Pictures"),
        Path::new("/Users/preview/Movies"),
    );
    Application::new().run(move |cx: &mut App| {
        crate::app::install_basics(cx);
        SettingsModel::init(paths.settings_file.clone(), cx);
        let failed = if options.failed {
            vec![ShortcutAction::CaptureScreen, ShortcutAction::RecordScreen]
        } else {
            Vec::new()
        };
        let mut setup = FakeSetup::new(if options.granted {
            ScreenAccess::Granted
        } else {
            ScreenAccess::Denied
        });
        setup.failed = failed.clone();
        Setup::install(setup, cx);
        let result = match target {
            Target::Settings => {
                open_settings(&paths, &options, failed, cx).map_err(|error| eyre!("{error}"))
            }
            Target::Onboarding => {
                open_onboarding(&options, failed, cx).map_err(|error| eyre!("{error}"))
            }
            Target::Preview => open_preview(&options, cx),
            Target::Editor => open_editor(&options, cx),
            Target::Region => open_region(&options, cx),
            Target::TrayRecording => open_tray_recording(cx),
        };
        match result {
            Ok(()) => float_windows(),
            Err(error) => {
                tracing::error!(%error, "could not open the preview");
                cx.quit();
            }
        }
    });
    Ok(())
}

/// GPUI stops drawing occluded windows, and previews don't take focus, so
/// lift them above other apps' windows without activating ss.
fn float_windows() {
    let Some(marker) = MainThreadMarker::new() else {
        return;
    };
    for window in NSApplication::sharedApplication(marker).windows() {
        window.setLevel(NSFloatingWindowLevel);
        window.orderFrontRegardless();
    }
}

fn open_settings(
    paths: &Paths,
    options: &Options,
    failed: Vec<ShortcutAction>,
    cx: &mut App,
) -> gpui::Result<()> {
    let handle = crate::windows::settings::open_at(paths, false, cx)?;
    let bottom = matches!(options.scroll, Scroll::Bottom);
    let recording = options.recording;
    handle.update(cx, |view, _, cx| {
        view.show_preview_state(failed, recording, cx)
    })?;
    if bottom {
        // Scroll once the first frame has laid the content out.
        cx.spawn(async move |cx: &mut AsyncApp| {
            cx.background_executor()
                .timer(Duration::from_millis(300))
                .await;
            let _ = handle.update(cx, |view, _, cx| view.scroll_to_bottom(cx));
        })
        .detach();
    }
    Ok(())
}

/// The preview card on ss's onboarding screenshot (`tests/fixtures`).
fn open_preview(options: &Options, cx: &mut App) -> Result<()> {
    let fixture = fixture("ocr-welcome.png");
    let png = std::fs::read(&fixture)?;
    let (width, height) = crate::capture_store::png_size(&png).unwrap_or((1, 1));
    let image = crate::thumbnail::from_png(&png, crate::thumbnail::PREVIEW_THUMBNAIL_WIDTH)?;
    let payload = PreviewPayload {
        id: "preview".into(),
        kind: if options.video {
            CaptureKind::Video
        } else {
            CaptureKind::Image
        },
        path: fixture,
        width,
        height,
        thumbnail: crate::thumbnail::for_preview(&image)?,
    };
    preview::show(payload, Rc::new(LoggingHost), cx)?;
    let handle = preview::handle(cx).ok_or_else(|| eyre!("no preview window"))?;
    let hover = options.hover;
    let chip = options.chip.clone();
    handle
        .update(cx, |view, _, cx| {
            if hover {
                view.set_hovered(true, cx);
            }
            if let Some(chip) = chip {
                view.set_note(chip, cx);
            }
        })
        .map_err(|error| eyre!("{error}"))?;
    Ok(())
}

fn open_region(options: &Options, cx: &mut App) -> Result<()> {
    let drag = match &options.drag {
        Some(text) => {
            let values: Vec<f64> = text
                .split(',')
                .map(|value| value.trim().parse())
                .collect::<std::result::Result<_, _>>()?;
            let [x, y, width, height] = values[..] else {
                return Err(eyre!("--drag wants x,y,w,h"));
            };
            Some(crate::displays::Rect::new(x, y, width, height))
        }
        None => None,
    };
    crate::windows::region::preview(drag, cx)
}

/// The tray as it looks 3 s into a recording; menu actions are ignored.
fn open_tray_recording(cx: &mut App) -> Result<()> {
    let (sender, receiver) = futures::channel::mpsc::unbounded();
    // Keep the receiver alive so menu sends don't fail.
    std::mem::forget(receiver);
    crate::tray::install(sender, &crate::settings::Shortcuts::default())?;
    crate::tray::set_recording(true);
    let started = std::time::Instant::now() - Duration::from_secs(3);
    cx.spawn(async move |cx: &mut AsyncApp| {
        loop {
            crate::tray::set_title(&format!(
                "● {}",
                crate::tray::format_elapsed(started.elapsed())
            ));
            cx.background_executor().timer(Duration::from_secs(1)).await;
        }
    })
    .detach();
    Ok(())
}

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn open_editor(options: &Options, cx: &mut App) -> Result<()> {
    use crate::windows::editor::{self, Load};
    let mut style = crate::settings::model::SettingsModel::global(cx)
        .read(cx)
        .settings()
        .editor
        .clone();
    if let Some(background) = &options.background {
        style.background = serde_json::from_value(serde_json::json!(background))?;
    }
    if let Some(ratio) = &options.ratio {
        style.ratio = serde_json::from_value(serde_json::json!(ratio))?;
    }
    let load = if options.loading {
        Load::Pending
    } else if options.load_failed {
        Load::Failed
    } else {
        let source = crate::compose::Source::from_png(&std::fs::read(fixture("ocr-welcome.png"))?)?;
        let inset = crate::compose::detect_edge_color(&source);
        Load::Ready(std::sync::Arc::new(source), inset)
    };
    let handle = editor::open("preview", load, Rc::new(PreviewEditorHost), false, cx)
        .map_err(|error| eyre!("{error}"))?;
    handle
        .update(cx, |view, _, cx| view.set_style(style, cx))
        .map_err(|error| eyre!("{error}"))?;
    let (highlight, select) = (options.highlight, options.select);
    // OCR arrives on the next tick; select once it has.
    cx.spawn(async move |cx: &mut AsyncApp| {
        cx.background_executor()
            .timer(Duration::from_millis(150))
            .await;
        let _ = handle.update(cx, |view, _, cx| {
            view.show_preview_state(highlight, select, cx)
        });
    })
    .detach();
    Ok(())
}

/// The editor's side effects only log; OCR comes from the fixture.
struct PreviewEditorHost;

impl crate::windows::editor::EditorHost for PreviewEditorHost {
    fn write_png(&self, png: &[u8]) -> std::result::Result<(), String> {
        tracing::info!(bytes = png.len(), "preview: copy image");
        Ok(())
    }

    fn write_text(&self, text: &str) -> std::result::Result<(), String> {
        tracing::info!(%text, "preview: copy text");
        Ok(())
    }

    fn save_style(&self, style: &crate::settings::EditorStyle, _: &mut App) {
        tracing::info!(?style, "preview: save style");
    }

    fn images_directory(&self, _: &App) -> std::path::PathBuf {
        std::env::temp_dir().join("ss-preview-images")
    }

    fn recognize(
        &self,
        _: &str,
        _: &mut App,
    ) -> Task<std::result::Result<std::sync::Arc<crate::ocr::TextLayout>, SharedString>> {
        let layout = std::fs::read(fixture("ocr-welcome.ocr.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .map(std::sync::Arc::new)
            .ok_or_else(|| SharedString::from("fixture OCR is missing"));
        Task::ready(layout)
    }
}

/// Preview buttons only log; nothing touches the clipboard or Finder.
struct LoggingHost;

impl PreviewHost for LoggingHost {
    fn open_editor(&self, id: &str, _: &mut App) {
        tracing::info!(id, "preview: edit");
    }

    fn reveal(&self, path: &Path) {
        tracing::info!(path = %path.display(), "preview: show in Finder");
    }

    fn copy_text(&self, id: &str, _: &mut App) -> Task<Result<usize, SharedString>> {
        tracing::info!(id, "preview: copy text");
        Task::ready(Ok(0))
    }
}

fn open_onboarding(
    options: &Options,
    failed: Vec<ShortcutAction>,
    cx: &mut App,
) -> gpui::Result<()> {
    let step = match options.step {
        PreviewStep::Welcome => Step::Welcome,
        PreviewStep::Permissions => Step::Permissions,
        PreviewStep::Shortcuts => Step::Shortcuts,
        PreviewStep::Done => Step::Done,
    };
    let handle = onboarding::open_at(step, false, cx)?;
    handle.update(cx, |view, _, cx| view.show_failures(failed, cx))?;
    Ok(())
}
