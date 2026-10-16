//! Everything a shortcut, the tray menu or a window can trigger (port of
//! `Actions.ts`). Captures run on the background executor; one at a time.

use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use futures::FutureExt;
use futures::future::Shared;
use gpui::{App, AppContext, AsyncApp, Entity, Global, SharedString, Task};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSArray, NSString, NSURL};

use crate::capture_store::{Capture, CaptureKind, CaptureStore};
use crate::displays::{self, Rect};
use crate::notify::notify;
use crate::ocr::TextLayout;
use crate::paths::Paths;
use crate::recorder::{Recorder, RecorderState};
use crate::screencapture::{self, CaptureError};
use crate::settings::model::SettingsModel;
use crate::settings::{AfterCapture, ShortcutAction};
use crate::windows::preview::{self, PreviewHost, PreviewPayload};

/// One recognition per capture, shared by the prefetch, Copy text and the editor.
type OcrTask = Shared<Task<Result<Arc<TextLayout>, SharedString>>>;
/// Successful recognitions kept in memory (Electron's cache capacity).
const OCR_CAPACITY: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    CaptureArea,
    CaptureScreen,
    RecordArea,
    RecordScreen,
    StopRecording,
    OpenLast,
    OpenFolder(Folder),
    OpenOnboarding,
    OpenSettings,
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Folder {
    Images,
    Recordings,
}

impl From<ShortcutAction> for Action {
    fn from(action: ShortcutAction) -> Self {
        match action {
            ShortcutAction::CaptureArea => Self::CaptureArea,
            ShortcutAction::CaptureScreen => Self::CaptureScreen,
            ShortcutAction::RecordArea => Self::RecordArea,
            ShortcutAction::RecordScreen => Self::RecordScreen,
            ShortcutAction::OpenLast => Self::OpenLast,
        }
    }
}

pub struct Actions {
    paths: Paths,
    store: Arc<CaptureStore>,
    busy: Arc<AtomicBool>,
    ocr: HashMap<String, OcrTask>,
    ocr_order: VecDeque<String>,
    recorder: Arc<Recorder>,
    recording: Option<Entity<RecordingStatus>>,
    ticker: Option<Task<()>>,
}

/// Whether a recording runs, for observers such as the menu bar timer.
#[derive(Default)]
pub struct RecordingStatus {
    pub started_at: Option<SystemTime>,
}

impl Global for Actions {}

impl Actions {
    pub fn new(paths: Paths, store: CaptureStore) -> Self {
        Self {
            paths,
            store: Arc::new(store),
            busy: Arc::new(AtomicBool::new(false)),
            ocr: HashMap::new(),
            ocr_order: VecDeque::new(),
            recorder: Arc::new(Recorder::new(Default::default())),
            recording: None,
            ticker: None,
        }
    }

    /// Stops a recording synchronously (≤ 5 s + 2 s), for the quit path.
    pub fn stop_recording_blocking(&self) {
        self.recorder.stop();
    }
}

/// The observable recording state, created on first use.
pub fn recording_status(cx: &mut App) -> Entity<RecordingStatus> {
    if let Some(status) = cx.global::<Actions>().recording.clone() {
        return status;
    }
    let status = cx.new(|_| RecordingStatus::default());
    cx.observe(&status, |status, cx| {
        let started_at = status.read(cx).started_at;
        crate::tray::set_recording(started_at.is_some());
        // "● m:ss", ticking every second while recording.
        let ticker = started_at.map(|started_at| {
            cx.spawn(async move |cx: &mut AsyncApp| {
                loop {
                    let elapsed = started_at.elapsed().unwrap_or_default();
                    crate::tray::set_title(&format!("● {}", crate::tray::format_elapsed(elapsed)));
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                }
            })
        });
        cx.global_mut::<Actions>().ticker = ticker;
    })
    .detach();
    cx.global_mut::<Actions>().recording = Some(status.clone());
    status
}

pub fn perform(action: Action, cx: &mut App) {
    tracing::info!(?action, "action");
    match action {
        Action::CaptureArea => capture(None, cx),
        Action::CaptureScreen => match displays::under_cursor() {
            Ok(display) => capture(Some(display.bounds), cx),
            Err(error) => report_failure(&error.to_string(), cx),
        },
        // TODO(M7): region overlay and the recorder.
        Action::RecordArea => toggle_recording(true, cx),
        Action::RecordScreen => toggle_recording(false, cx),
        Action::StopRecording => stop_recording(cx),
        Action::OpenLast => open_last(cx),
        Action::OpenFolder(folder) => open_folder(folder, cx),
        Action::OpenOnboarding => crate::windows::onboarding::open_or_focus(cx),
        Action::OpenSettings => {
            let paths = cx.global::<Actions>().paths.clone();
            if let Err(error) = crate::windows::settings::open_or_focus(&paths, cx) {
                tracing::error!(%error, "could not open Settings");
            }
        }
        Action::Quit => crate::app::quit(cx),
    }
}

/// Released when the capture pipeline finishes, however it ends.
struct BusyGuard(Arc<AtomicBool>);

impl BusyGuard {
    fn acquire(busy: &Arc<AtomicBool>) -> Option<Self> {
        busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| Self(busy.clone()))
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Captures `area`, or lets the user pick one when `None`.
fn capture(area: Option<Rect>, cx: &mut App) {
    let actions = cx.global::<Actions>();
    // Repeated presses while the crosshair is up are ignored.
    let Some(busy) = BusyGuard::acquire(&actions.busy) else {
        tracing::debug!("a capture is already running");
        return;
    };
    let capture = actions.store.new_image();
    preview::hide(cx);
    let task = cx.background_spawn({
        let capture = capture.clone();
        async move { take_screenshot(&capture.path, area) }
    });
    cx.spawn(async move |cx| {
        let result = task.await;
        let _ = cx.update(|cx| match result {
            Ok(Some(png)) => after_image(capture, &png, cx),
            Ok(None) => tracing::debug!("capture cancelled"),
            Err(CaptureError::Permission(detail)) => {
                tracing::warn!(%detail, "Screen Recording permission is missing");
                crate::windows::onboarding::open_or_focus(cx);
            }
            Err(error) => report_failure(&error.to_string(), cx),
        });
        drop(busy);
    })
    .detach();
}

/// Runs screencapture and reads the PNG back. Blocking.
fn take_screenshot(path: &Path, area: Option<Rect>) -> Result<Option<Vec<u8>>, CaptureError> {
    let saved = match area {
        Some(area) => Some(screencapture::rect(path, area)?),
        None => screencapture::interactive(path)?,
    };
    saved
        .map(|path| {
            fs::read(&path)
                .map_err(|error| CaptureError::Failed(format!("Cannot read capture: {error}")))
        })
        .transpose()
}

fn after_image(capture: Capture, png: &[u8], cx: &mut App) {
    let store = cx.global::<Actions>().store.clone();
    store.register(capture.clone());
    cx.background_spawn({
        let store = store.clone();
        async move {
            if let Err(error) = store.prune() {
                tracing::warn!(%error, "could not prune capture history");
            }
        }
    })
    .detach();
    if let Err(error) = crate::clipboard::write_png(png) {
        return report_failure(&format!("Cannot copy image to clipboard: {error}"), cx);
    }
    tracing::info!(id = %capture.id, bytes = png.len(), "copied capture to the clipboard");
    // Warm up OCR so text is selectable by the time the editor opens.
    recognize(&capture.id, cx);
    match SettingsModel::global(cx).read(cx).settings().after_capture {
        AfterCapture::Nothing => {}
        AfterCapture::Preview => show_preview(capture, png.to_vec(), cx),
        AfterCapture::Editor => open_editor(&capture.id, cx),
    }
}

/// Builds the thumbnail in the background, then slides the preview in.
fn show_preview(capture: Capture, png: Vec<u8>, cx: &mut App) {
    let thumbnail = cx.background_spawn(async move {
        let size = crate::capture_store::png_size(&png).unwrap_or((1, 1));
        let image = crate::thumbnail::from_png(&png, crate::thumbnail::PREVIEW_THUMBNAIL_WIDTH)?;
        Ok::<_, color_eyre::Report>((size, crate::thumbnail::for_preview(&image)?))
    });
    cx.spawn(async move |cx| {
        let result = match thumbnail.await {
            Ok(((width, height), thumbnail)) => cx.update(|cx| {
                let payload = PreviewPayload {
                    id: capture.id,
                    kind: capture.kind,
                    path: capture.path,
                    width,
                    height,
                    thumbnail,
                };
                preview::show(payload, Rc::new(AppPreviewHost), cx)
            }),
            Err(error) => Ok(Err(error)),
        };
        if let Ok(Err(error)) = result {
            tracing::error!(%error, "could not show the preview");
        }
    })
    .detach();
}

/// The editor for an image; recordings are revealed in Finder instead.
pub fn open_editor(id: &str, cx: &mut App) {
    let Some(capture) = cx.global::<Actions>().store.get(id) else {
        return report_failure("This capture is no longer available.", cx);
    };
    preview::hide(cx);
    match capture.kind {
        CaptureKind::Image => {
            let host = Rc::new(crate::windows::editor::AppEditorHost {
                default_images: cx.global::<Actions>().paths.default_images.clone(),
            });
            let load = crate::windows::editor::Load::Path(capture.path);
            if let Err(error) = crate::windows::editor::open(id, load, host, true, cx) {
                tracing::error!(%error, "could not open the editor");
            }
        }
        CaptureKind::Video => reveal(&capture.path),
    }
}

/// Recognises a capture's text once; concurrent callers share the work.
/// `None` if the capture is unknown. Failures are not remembered.
pub fn recognize(id: &str, cx: &mut App) -> Option<OcrTask> {
    let actions = cx.global::<Actions>();
    if let Some(task) = actions.ocr.get(id) {
        return Some(task.clone());
    }
    let capture = actions.store.get(id)?;
    let cache = actions.store.directory().join(format!("{id}.ocr.json"));
    let work = cx.background_spawn(async move {
        crate::ocr::load_or_recognize(&capture.path, &cache)
            .map(Arc::new)
            .map_err(|error| SharedString::from(error.to_string()))
    });
    let key = id.to_string();
    let task = cx
        .spawn(async move |cx| {
            let result = work.await;
            if result.is_err() {
                let _ = cx.update(|cx| forget_ocr(&key, cx));
            }
            result
        })
        .shared();
    let actions = cx.global_mut::<Actions>();
    actions.ocr.insert(id.to_string(), task.clone());
    actions.ocr_order.push_back(id.to_string());
    while actions.ocr_order.len() > OCR_CAPACITY {
        if let Some(oldest) = actions.ocr_order.pop_front() {
            actions.ocr.remove(&oldest);
        }
    }
    Some(task)
}

fn forget_ocr(id: &str, cx: &mut App) {
    let actions = cx.global_mut::<Actions>();
    actions.ocr.remove(id);
    actions.ocr_order.retain(|candidate| candidate != id);
}

/// Copies a capture's text (lines joined with `\n`) when there is any, and
/// resolves to the number of lines.
pub fn copy_text(id: &str, cx: &mut App) -> Task<Result<usize, SharedString>> {
    let Some(task) = recognize(id, cx) else {
        return Task::ready(Err("This capture is no longer available.".into()));
    };
    cx.spawn(async move |cx| {
        let layout = task.await?;
        let text = layout.text();
        if !text.is_empty() {
            cx.update(|_| crate::clipboard::write_text(&text))
                .map_err(|error| SharedString::from(error.to_string()))?
                .map_err(|error| SharedString::from(error.to_string()))?;
        }
        Ok(layout.lines.len())
    })
}

/// The preview's buttons in the running app.
struct AppPreviewHost;

impl PreviewHost for AppPreviewHost {
    fn open_editor(&self, id: &str, cx: &mut App) {
        open_editor(id, cx);
    }

    fn reveal(&self, path: &Path) {
        reveal(path);
    }

    fn copy_text(&self, id: &str, cx: &mut App) -> Task<Result<usize, SharedString>> {
        copy_text(id, cx)
    }
}

/// Stops on the background executor; the finaliser does the rest.
fn stop_recording(cx: &mut App) {
    let recorder = cx.global::<Actions>().recorder.clone();
    cx.background_spawn(async move { recorder.stop() }).detach();
}

/// `toggleRecording`: stops a running recording, otherwise picks an area
/// (or the display under the pointer) and starts one.
fn toggle_recording(pick_area: bool, cx: &mut App) {
    let actions = cx.global::<Actions>();
    if matches!(actions.recorder.state(), RecorderState::Recording { .. }) {
        return stop_recording(cx);
    }
    let Some(busy) = BusyGuard::acquire(&actions.busy) else {
        tracing::debug!("a capture is already running");
        return;
    };
    preview::hide(cx);
    if pick_area {
        let selection = crate::windows::region::select(cx);
        cx.spawn(async move |cx: &mut AsyncApp| {
            if let Ok(Some(rect)) = selection.await {
                let _ = cx.update(|cx| start_recording(rect, cx));
            }
            drop(busy);
        })
        .detach();
    } else {
        match displays::under_cursor() {
            Ok(display) => start_recording(display.bounds, cx),
            Err(error) => report_failure(&error.to_string(), cx),
        }
        drop(busy);
    }
}

fn start_recording(rect: Rect, cx: &mut App) {
    let directory = SettingsModel::global(cx)
        .read(cx)
        .settings()
        .recordings_directory
        .as_ref()
        .map_or_else(
            || cx.global::<Actions>().paths.default_recordings.clone(),
            PathBuf::from,
        );
    let actions = cx.global::<Actions>();
    let (store, recorder) = (actions.store.clone(), actions.recorder.clone());
    let capture = match store.new_recording(&directory) {
        Ok(capture) => capture,
        Err(error) => {
            return report_failure(
                &format!("Cannot create {}: {error}", directory.display()),
                cx,
            );
        }
    };
    let finished = match recorder.start(capture, |path| screencapture::record(path, rect)) {
        Ok(finished) => finished,
        Err(error) => return report_failure(&error.to_string(), cx),
    };
    let status = recording_status(cx);
    status.update(cx, |status, cx| {
        status.started_at = Some(SystemTime::now());
        cx.notify();
    });
    // The single place a recording is finalised, however it ended.
    cx.spawn(async move |cx: &mut AsyncApp| {
        let finished = finished.await;
        let _ = cx.update(|cx| {
            status.update(cx, |status, cx| {
                status.started_at = None;
                cx.notify();
            });
            if let Ok(finished) = &finished {
                tracing::info!(exit_code = finished.exit_code, "recording finished");
            }
            if let Ok(finished) = finished
                && finished.produced_file
            {
                store.register(finished.capture.clone());
                after_recording(finished.capture, rect, cx);
            }
        });
    })
    .detach();
}

/// `afterRecording`: the file goes on the clipboard, then the preview.
fn after_recording(capture: Capture, rect: Rect, cx: &mut App) {
    if let Err(error) = crate::clipboard::write_file_url(&capture.path) {
        return report_failure(&format!("Cannot copy the recording: {error}"), cx);
    }
    let path = capture.path.clone();
    let thumbnail = cx.background_spawn(async move {
        let poster =
            crate::thumbnail::video_poster(&path, crate::thumbnail::PREVIEW_THUMBNAIL_WIDTH)
                .unwrap_or_else(|error| {
                    tracing::warn!(%error, "no poster frame");
                    crate::thumbnail::empty()
                });
        crate::thumbnail::for_preview(&poster)
    });
    cx.spawn(async move |cx: &mut AsyncApp| {
        let result = match thumbnail.await {
            Ok(thumbnail) => cx.update(|cx| {
                let payload = PreviewPayload {
                    id: capture.id,
                    kind: CaptureKind::Video,
                    path: capture.path,
                    width: rect.width.round() as u32,
                    height: rect.height.round() as u32,
                    thumbnail,
                };
                preview::show(payload, Rc::new(AppPreviewHost), cx)
            }),
            Err(error) => Ok(Err(error)),
        };
        if let Ok(Err(error)) = result {
            tracing::error!(%error, "could not show the recording preview");
        }
    })
    .detach();
}

fn open_last(cx: &mut App) {
    let Some(latest) = cx.global::<Actions>().store.latest() else {
        return notify("ss", "Nothing captured yet");
    };
    open_editor(&latest.id, cx);
}

fn open_folder(folder: Folder, cx: &mut App) {
    let actions = cx.global::<Actions>();
    let settings = SettingsModel::global(cx).read(cx).settings();
    let directory = match folder {
        Folder::Images => settings
            .images_directory
            .as_ref()
            .map_or_else(|| actions.paths.default_images.clone(), PathBuf::from),
        Folder::Recordings => settings
            .recordings_directory
            .as_ref()
            .map_or_else(|| actions.paths.default_recordings.clone(), PathBuf::from),
    };
    if let Err(error) = fs::create_dir_all(&directory) {
        return report_failure(
            &format!("Cannot create {}: {error}", directory.display()),
            cx,
        );
    }
    if !NSWorkspace::sharedWorkspace().openURL(&file_url(&directory)) {
        report_failure(&format!("Cannot open {}", directory.display()), cx);
    }
}

pub fn reveal(path: &Path) {
    let urls = NSArray::from_retained_slice(&[file_url(path)]);
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&urls);
}

fn file_url(path: &Path) -> objc2::rc::Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

fn report_failure(message: &str, _cx: &mut App) {
    tracing::error!(%message, "action failed");
    notify("ss", message);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_guard_admits_one_capture_at_a_time() {
        let busy = Arc::new(AtomicBool::new(false));
        let first = BusyGuard::acquire(&busy);
        assert!(first.is_some());
        assert!(BusyGuard::acquire(&busy).is_none());
        drop(first);
        assert!(BusyGuard::acquire(&busy).is_some());
    }

    #[test]
    fn shortcuts_map_to_their_actions() {
        assert_eq!(
            Action::from(ShortcutAction::CaptureArea),
            Action::CaptureArea
        );
        assert_eq!(Action::from(ShortcutAction::OpenLast), Action::OpenLast);
    }
}
