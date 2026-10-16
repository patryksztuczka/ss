//! The Settings window (port of `renderer/src/windows/Settings.tsx`).

use std::path::PathBuf;

use gpui::{
    Animation, AnimationExt, App, AsyncApp, Context, Entity, FocusHandle, Focusable, FontWeight,
    Global, IntoElement, Keystroke, PathPromptOptions, Render, ScrollHandle, SharedString,
    Subscription, Task, WeakEntity, Window, WindowHandle, div, prelude::*, px, rgb,
};

use crate::accelerator::{accelerator_from_keystroke, format_accelerator};
use crate::app::CloseWindow;
use crate::desktop_ui::{
    ACCENT, ButtonVariant, CONTENT_PADDING, INK, MONO_FONT, MUTED, RED_500, RISE_IN, SECTION_GAP,
    TEXT_CONTROL, TEXT_XS, accent_soft, alpha, button, compact_row, divided_card, ease_out_soft,
    hint, link_button, row_description, section, segmented, settings_row, settings_window_size,
    status_pill, switch_toggle, titlebar, window_frame, window_options,
};
use crate::paths::{Paths, tildify};
use crate::settings::model::SettingsModel;
use crate::settings::{
    AfterCapture, SettingsPatch, ShortcutAction, ShortcutPreset, is_usable_accelerator,
};
use crate::setup::{self, Setup};
use crate::system::{ScreenAccess, SetupState};

const SHORTCUTS: [(ShortcutAction, &str); 5] = [
    (ShortcutAction::CaptureArea, "Capture area or window"),
    (ShortcutAction::CaptureScreen, "Capture screen"),
    (ShortcutAction::RecordArea, "Record area · stop"),
    (ShortcutAction::RecordScreen, "Record screen · stop"),
    (ShortcutAction::OpenLast, "Edit last capture"),
];

const AFTER_CAPTURE: [(AfterCapture, &str); 3] = [
    (AfterCapture::Preview, "Show preview"),
    (AfterCapture::Editor, "Open editor"),
    (AfterCapture::Nothing, "Just copy"),
];

const SHORTCUT_BUTTON_MIN_WIDTH: f32 = 92.0;
const RECORDER_HINT: &str =
    "Click a shortcut and press a new combination. ⌫ clears it, Esc cancels.";
const OPTION_ONLY_HINT: &str = "Include ⌘ or ⌃ — macOS ignores shortcuts that only use ⌥ / ⇧.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Folder {
    Images,
    Recordings,
}

/// What a key press does while a shortcut row is recording.
#[derive(Clone, Debug, PartialEq, Eq)]
enum RecorderInput {
    Cancel,
    Clear,
    /// Only modifiers so far, or a key that cannot be a shortcut.
    Ignore,
    /// ⌥/⇧-only: macOS would never fire it.
    Rejected,
    Accept(String),
}

fn recorder_input(keystroke: &Keystroke) -> RecorderInput {
    let modifiers = &keystroke.modifiers;
    match keystroke.key.as_str() {
        "escape" => RecorderInput::Cancel,
        "backspace" if !modifiers.platform && !modifiers.alt && !modifiers.control => {
            RecorderInput::Clear
        }
        _ => match accelerator_from_keystroke(keystroke) {
            None => RecorderInput::Ignore,
            Some(accelerator) if !is_usable_accelerator(&accelerator) => RecorderInput::Rejected,
            Some(accelerator) => RecorderInput::Accept(accelerator),
        },
    }
}

#[derive(Default)]
struct OpenSettingsWindow(Option<WindowHandle<SettingsWindow>>);

impl Global for OpenSettingsWindow {}

/// Focuses the Settings window, opening it if needed.
pub fn open_or_focus(paths: &Paths, cx: &mut App) -> gpui::Result<WindowHandle<SettingsWindow>> {
    open_at(paths, true, cx)
}

/// Previews pass `activate: false` so they don't take focus from the user.
pub fn open_at(
    paths: &Paths,
    activate: bool,
    cx: &mut App,
) -> gpui::Result<WindowHandle<SettingsWindow>> {
    if let Some(handle) = cx.default_global::<OpenSettingsWindow>().0
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(handle);
    }
    let settings = SettingsModel::global(cx);
    let default_images = paths.default_images.clone();
    let default_recordings = paths.default_recordings.clone();
    let mut options = window_options("ss Settings", settings_window_size(), cx);
    options.focus = activate;
    let handle = cx.open_window(options, |window, cx| {
        cx.new(|cx| SettingsWindow::new(settings, default_images, default_recordings, window, cx))
    })?;
    cx.set_global(OpenSettingsWindow(Some(handle)));
    if activate {
        cx.activate(true);
    }
    Ok(handle)
}

pub struct SettingsWindow {
    focus: FocusHandle,
    settings: Entity<SettingsModel>,
    /// `None` until the first probe returns; the window stays blank like Electron's.
    setup: Option<SetupState>,
    failed: Vec<ShortcutAction>,
    recording: Option<ShortcutAction>,
    hint: Option<&'static str>,
    default_images: PathBuf,
    default_recordings: PathBuf,
    scroll: ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl SettingsWindow {
    fn new(
        settings: Entity<SettingsModel>,
        default_images: PathBuf,
        default_recordings: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        let this = cx.entity().downgrade();
        let window_handle = window.window_handle();
        // Interceptors run before key bindings, so ⌘W is recorded instead of closing.
        let recorder = cx.intercept_keystrokes(move |event, window, cx| {
            if window.window_handle() != window_handle {
                return;
            }
            let handled = this
                .upgrade()
                .is_some_and(|this| this.update(cx, |this, cx| this.record(&event.keystroke, cx)));
            if handled {
                cx.stop_propagation();
            }
        });
        let subscriptions = vec![
            cx.observe(&settings, |_, _, cx| cx.notify()),
            // Permission and macOS shortcut state can change outside ss.
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.refresh_setup(cx);
                }
            }),
            recorder,
        ];
        let mut this = Self {
            focus,
            settings,
            setup: None,
            failed: Vec::new(),
            recording: None,
            hint: None,
            default_images,
            default_recordings,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        };
        this.refresh_setup(cx);
        this
    }

    /// `ss preview settings`: failed shortcuts, and optionally a row
    /// recording with the ⌥-only hint showing.
    pub fn show_preview_state(
        &mut self,
        failed: Vec<ShortcutAction>,
        recording: bool,
        cx: &mut Context<Self>,
    ) {
        self.failed = failed;
        if recording {
            self.recording = Some(ShortcutAction::CaptureArea);
            self.hint = Some(OPTION_ONLY_HINT);
        }
        cx.notify();
    }

    pub fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.scroll.scroll_to_bottom();
        cx.notify();
    }

    fn refresh_setup(&mut self, cx: &mut Context<Self>) {
        let task = Setup::global(cx).state(cx);
        self.set_setup_when_ready(task, cx);
    }

    fn set_setup_when_ready(&mut self, task: Task<SetupState>, cx: &mut Context<Self>) {
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let state = task.await;
            this.update(cx, |this, cx| {
                this.setup = Some(state);
                cx.notify();
            })
        })
        .detach();
    }

    fn request_screen_access(&mut self, cx: &mut Context<Self>) {
        let task = Setup::global(cx).request_screen_access(cx);
        self.set_setup_when_ready(task, cx);
    }

    fn apply_preset(&mut self, preset: ShortcutPreset, cx: &mut Context<Self>) {
        let task = Setup::global(cx).apply_preset(preset, cx);
        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let result = task.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(outcome) => this.failed = outcome.failed,
                    Err(error) => tracing::error!(%error, ?preset, "could not apply shortcuts"),
                }
                this.refresh_setup(cx);
            })
        })
        .detach();
    }

    fn set_launch_at_login(&mut self, enabled: bool, cx: &mut Context<Self>) {
        let task = Setup::global(cx).set_launch_at_login(enabled, cx);
        self.set_setup_when_ready(task, cx);
    }

    fn toggle_recording(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        if self.recording == Some(action) {
            self.stop_recording(cx);
        } else {
            self.recording = Some(action);
            self.hint = None;
            // Release the global shortcuts so pressing them doesn't fire a capture.
            setup::pause_hotkeys(true, cx);
            cx.notify();
        }
    }

    fn stop_recording(&mut self, cx: &mut Context<Self>) {
        self.recording = None;
        self.hint = None;
        setup::pause_hotkeys(false, cx);
        cx.notify();
    }

    /// Handles a key press while recording. Returns whether it was consumed.
    fn record(&mut self, keystroke: &Keystroke, cx: &mut Context<Self>) -> bool {
        let Some(action) = self.recording else {
            return false;
        };
        match recorder_input(keystroke) {
            RecorderInput::Cancel => self.stop_recording(cx),
            RecorderInput::Clear => {
                self.save_shortcut(action, "", cx);
                self.stop_recording(cx);
            }
            RecorderInput::Ignore => {}
            RecorderInput::Rejected => {
                self.hint = Some(OPTION_ONLY_HINT);
                cx.notify();
            }
            RecorderInput::Accept(accelerator) => {
                self.save_shortcut(action, &accelerator, cx);
                self.stop_recording(cx);
            }
        }
        true
    }

    fn save_shortcut(&mut self, action: ShortcutAction, accelerator: &str, cx: &mut Context<Self>) {
        let mut shortcuts = self.settings.read(cx).settings().shortcuts.clone();
        shortcuts.set(action, accelerator);
        self.failed = setup::save_shortcuts(shortcuts, cx);
    }

    fn pick_folder(&mut self, folder: Folder, cx: &mut Context<Self>) {
        let paths = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Choose".into()),
        });
        let settings = self.settings.clone();
        cx.spawn(async move |_, cx: &mut AsyncApp| {
            let Ok(Ok(Some(paths))) = paths.await else {
                return;
            };
            let Some(directory) = paths.into_iter().next() else {
                return;
            };
            let directory = Some(directory.to_string_lossy().into_owned());
            let patch = match folder {
                Folder::Images => SettingsPatch {
                    images_directory: Some(directory),
                    ..SettingsPatch::default()
                },
                Folder::Recordings => SettingsPatch {
                    recordings_directory: Some(directory),
                    ..SettingsPatch::default()
                },
            };
            let result = settings.update(cx, |model, cx| model.update(patch, cx));
            if let Ok(Err(error)) = result {
                tracing::error!(%error, "could not save the folder");
            }
        })
        .detach();
    }

    fn render_system(&self, setup: &SetupState, cx: &mut Context<Self>) -> impl IntoElement {
        let screen_recording = settings_row(
            "Screen Recording",
            Some(row_description(match &setup.permission_host {
                None => SharedString::from("Required to capture your screen."),
                Some(host) => format!("In development, granted to {host}.").into(),
            })),
            if setup.screen_access == ScreenAccess::Granted {
                status_pill(true, "Granted").into_any_element()
            } else {
                button("grant-access", "Grant Access…", ButtonVariant::Ghost, None)
                    .on_click(cx.listener(|this, _, _, cx| this.request_screen_access(cx)))
                    .into_any_element()
            },
        );
        let (description, label, preset, id) = if setup.system_screenshot_shortcuts {
            (
                "On — ⇧⌘3 / ⇧⌘4 / ⇧⌘5 open the built-in tool.",
                "Use for ss",
                ShortcutPreset::System,
                "use-for-ss",
            )
        } else {
            (
                "Off — ⇧⌘3 / ⇧⌘4 / ⇧⌘5 are free for ss.",
                "Give back to macOS",
                ShortcutPreset::Alongside,
                "give-back",
            )
        };
        let screenshot_shortcuts = settings_row(
            "macOS screenshot shortcuts",
            Some(row_description(description)),
            button(id, label, ButtonVariant::Ghost, None)
                .on_click(cx.listener(move |this, _, _, cx| this.apply_preset(preset, cx))),
        );
        let login = setup.launch_at_login;
        let login_description = if login.requires_approval {
            link_button(
                "approve-login-item",
                "Approve ss in System Settings → Login Items",
            )
            .on_click(|_, _, cx| Setup::global(cx).open_login_items(cx))
            .into_any_element()
        } else if login.available {
            row_description("Keeps your shortcuts working after a restart.")
        } else {
            row_description("Available in the installed app.")
        };
        let launch_at_login = settings_row(
            "Launch at login",
            Some(login_description),
            div()
                .id("launch-at-login")
                .child(switch_toggle(
                    if login.enabled { 1.0 } else { 0.0 },
                    !login.available,
                ))
                .when(login.available, |switch| {
                    switch.on_click(cx.listener(move |this, _, _, cx| {
                        this.set_launch_at_login(!login.enabled, cx)
                    }))
                }),
        );
        section(
            "System",
            Some(
                link_button("open-setup-guide", "Open Setup Guide")
                    .on_click(|_, _, cx| crate::windows::onboarding::open_or_focus(cx))
                    .into_any_element(),
            ),
        )
        .child(divided_card([
            screen_recording.into_any_element(),
            screenshot_shortcuts.into_any_element(),
            launch_at_login.into_any_element(),
        ]))
    }

    fn render_shortcuts(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let shortcuts = self.settings.read(cx).settings().shortcuts.clone();
        let rows: Vec<_> = SHORTCUTS
            .iter()
            .enumerate()
            .map(|(index, &(action, label))| {
                let recording = self.recording == Some(action);
                let failed = self.failed.contains(&action);
                let shortcut = div()
                    .id(("shortcut", index))
                    .debug_selector(move || format!("shortcut-{index}"))
                    .h(px(28.0))
                    .min_w(px(SHORTCUT_BUTTON_MIN_WIDTH))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(8.0))
                    .text_size(px(TEXT_CONTROL))
                    .font_weight(FontWeight::MEDIUM)
                    .map(|button| {
                        if recording {
                            button
                                .bg(accent_soft())
                                .text_color(rgb(ACCENT))
                                .border_1()
                                .border_color(alpha(ACCENT, 0.4))
                        } else {
                            button
                                .bg(alpha(0x000000, 0.045))
                                .hover(|button| button.bg(alpha(0x000000, 0.07)))
                                .text_color(rgb(if failed { RED_500 } else { INK }))
                        }
                    })
                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_recording(action, cx)))
                    .child(if recording {
                        "Press keys…".to_string()
                    } else {
                        format_accelerator(shortcuts.get(action))
                    });
                compact_row(
                    label,
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .when(failed, |control| {
                            control.child(
                                div()
                                    .text_size(px(TEXT_XS))
                                    .text_color(rgb(RED_500))
                                    .child("in use elsewhere"),
                            )
                        })
                        .child(shortcut),
                )
                .into_any_element()
            })
            .collect();
        section("Shortcuts", None)
            .child(divided_card(rows))
            .child(match self.hint {
                Some(text) => hint(text)
                    .debug_selector(|| "recorder-hint".into())
                    .text_color(rgb(RED_500)),
                None => hint(RECORDER_HINT),
            })
    }

    fn render_after_capture(&self, cx: &App) -> impl IntoElement {
        let selected = self.settings.read(cx).settings().after_capture;
        let settings = self.settings.clone();
        section("After capture", None)
            .child(segmented(
                "after-capture",
                &AFTER_CAPTURE,
                selected,
                move |after_capture, _, cx| {
                    let result = settings.update(cx, |model, cx| {
                        model.update(
                            SettingsPatch {
                                after_capture: Some(after_capture),
                                ..SettingsPatch::default()
                            },
                            cx,
                        )
                    });
                    if let Err(error) = result {
                        tracing::error!(%error, "could not save settings");
                    }
                },
            ))
            .child(hint(
                "Every capture goes straight to the clipboard. Nothing is ever saved to the Desktop.",
            ))
    }

    fn render_folders(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let settings = self.settings.read(cx).settings();
        let images = settings.images_directory.as_ref().map_or_else(
            || tildify(&self.default_images),
            |path| tildify(path.as_ref()),
        );
        let recordings = settings.recordings_directory.as_ref().map_or_else(
            || tildify(&self.default_recordings),
            |path| tildify(path.as_ref()),
        );
        section("Folders", None).child(divided_card([
            folder_row("Saved screenshots (⌘S)", images, Folder::Images, cx),
            folder_row("Recordings", recordings, Folder::Recordings, cx),
        ]))
    }
}

fn folder_row(
    label: &'static str,
    path: String,
    folder: Folder,
    cx: &mut Context<SettingsWindow>,
) -> gpui::AnyElement {
    let id = match folder {
        Folder::Images => "change-images",
        Folder::Recordings => "change-recordings",
    };
    settings_row(
        label,
        Some(
            div()
                .w_full()
                .min_w_0()
                .overflow_x_hidden()
                .truncate()
                .font_family(MONO_FONT)
                .text_size(px(TEXT_XS))
                .text_color(rgb(MUTED))
                .child(path)
                .into_any_element(),
        ),
        button(id, "Change…", ButtonVariant::Ghost, None)
            .on_click(cx.listener(move |this, _, _, cx| this.pick_folder(folder, cx))),
    )
    .into_any_element()
}

impl Focusable for SettingsWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = window_frame()
            .track_focus(&self.focus)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .child(titlebar(Some("Settings")));
        let Some(setup) = self.setup.clone() else {
            return frame;
        };
        frame.child(
            div()
                .id("settings-content")
                .track_scroll(&self.scroll)
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(CONTENT_PADDING))
                .pt(px(12.0))
                .pb(px(CONTENT_PADDING))
                .flex()
                .flex_col()
                .gap(px(SECTION_GAP))
                .child(self.render_system(&setup, cx))
                .child(self.render_shortcuts(cx))
                .child(self.render_after_capture(cx))
                .child(self.render_folders(cx))
                .child(
                    div()
                        .debug_selector(|| "settings-footer".into())
                        .w_full()
                        .flex_none()
                        .text_center()
                        .text_size(px(TEXT_XS))
                        .text_color(rgb(MUTED))
                        .child(concat!("ss ", env!("CARGO_PKG_VERSION"))),
                )
                .with_animation(
                    "settings-rise-in",
                    Animation::new(RISE_IN).with_easing(ease_out_soft()),
                    |content, delta| content.opacity(delta).mt(px(8.0 * (1.0 - delta))),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use gpui::{KeyBinding, Modifiers, TestAppContext, VisualTestContext, size};

    use super::*;
    use crate::settings;
    use crate::setup::FakeSetup;

    struct Fixture<'a> {
        _directory: tempfile::TempDir,
        path: PathBuf,
        view: Entity<SettingsWindow>,
        cx: &'a mut VisualTestContext,
    }

    fn open(cx: &mut TestAppContext) -> Fixture<'_> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let model = cx.update(|cx| {
            cx.bind_keys([KeyBinding::new("cmd-w", CloseWindow, None)]);
            Setup::install(FakeSetup::new(ScreenAccess::Denied), cx);
            SettingsModel::init(path.clone(), cx)
        });
        let (view, cx) = cx.add_window_view(|window, cx| {
            SettingsWindow::new(
                model,
                "/Pictures/ss".into(),
                "/Movies/ss".into(),
                window,
                cx,
            )
        });
        cx.simulate_resize(size(px(560.0), px(760.0)));
        cx.run_until_parked();
        Fixture {
            _directory: directory,
            path,
            view,
            cx,
        }
    }

    impl Fixture<'_> {
        fn click(&mut self, selector: &'static str) {
            let bounds = self.cx.debug_bounds(selector).unwrap();
            self.cx.simulate_click(bounds.center(), Modifiers::none());
            self.cx.run_until_parked();
        }

        fn saved(&self) -> settings::Settings {
            settings::read(&self.path).settings
        }

        fn recording(&mut self) -> Option<ShortcutAction> {
            self.view.read_with(self.cx, |view, _| view.recording)
        }
    }

    #[gpui::test]
    fn choosing_after_capture_persists_it(cx: &mut TestAppContext) {
        let mut fixture = open(cx);
        fixture.click("segmented-item-2");
        assert_eq!(fixture.saved().after_capture, AfterCapture::Nothing);
    }

    #[gpui::test]
    fn command_w_closes_the_window(cx: &mut TestAppContext) {
        let fixture = open(cx);
        let window = fixture.cx.window_handle();
        fixture.cx.simulate_keystrokes("cmd-w");
        fixture.cx.run_until_parked();
        assert!(!fixture.cx.cx.windows().contains(&window));
    }

    #[gpui::test]
    fn escape_cancels_recording(cx: &mut TestAppContext) {
        let mut fixture = open(cx);
        fixture.click("shortcut-0");
        assert_eq!(fixture.recording(), Some(ShortcutAction::CaptureArea));
        fixture.cx.simulate_keystrokes("escape");
        fixture.cx.run_until_parked();
        assert_eq!(fixture.recording(), None);
        assert!(!fixture.path.exists(), "cancelling must not save");
    }

    #[gpui::test]
    fn backspace_clears_the_shortcut(cx: &mut TestAppContext) {
        let mut fixture = open(cx);
        fixture.click("shortcut-4");
        fixture.cx.simulate_keystrokes("backspace");
        fixture.cx.run_until_parked();
        assert_eq!(fixture.recording(), None);
        assert_eq!(fixture.saved().shortcuts.open_last, "");
    }

    #[gpui::test]
    fn option_only_combos_are_rejected_and_keep_recording(cx: &mut TestAppContext) {
        let mut fixture = open(cx);
        fixture.click("shortcut-0");
        fixture.cx.simulate_keystrokes("alt-shift-4");
        fixture.cx.run_until_parked();
        assert_eq!(fixture.recording(), Some(ShortcutAction::CaptureArea));
        let hint = fixture.view.read_with(fixture.cx, |view, _| view.hint);
        assert_eq!(hint, Some(OPTION_ONLY_HINT));
        assert!(!fixture.path.exists());
    }

    #[gpui::test]
    fn usable_combos_are_saved_even_if_they_are_window_shortcuts(cx: &mut TestAppContext) {
        let mut fixture = open(cx);
        let window = fixture.cx.window_handle();
        fixture.click("shortcut-1");
        // ⌘W would close the window if the recorder did not intercept it.
        fixture.cx.simulate_keystrokes("cmd-w");
        fixture.cx.run_until_parked();
        assert!(fixture.cx.cx.windows().contains(&window));
        assert_eq!(fixture.recording(), None);
        assert_eq!(fixture.saved().shortcuts.capture_screen, "Command+W");

        fixture.click("shortcut-0");
        fixture.cx.simulate_keystrokes("cmd-shift-k");
        fixture.cx.run_until_parked();
        assert_eq!(fixture.saved().shortcuts.capture_area, "Shift+Command+K");
        let hint = fixture.view.read_with(fixture.cx, |view, _| view.hint);
        assert_eq!(hint, None);
    }

    #[gpui::test]
    fn the_body_scrolls_to_the_version_footer(cx: &mut TestAppContext) {
        let fixture = open(cx);
        let footer = fixture.cx.debug_bounds("settings-footer").unwrap();
        assert!(
            footer.top() > px(760.0),
            "the M1 layout already overflows: {footer:?}"
        );
        fixture
            .view
            .update(fixture.cx, |view, cx| view.scroll_to_bottom(cx));
        fixture.cx.run_until_parked();
        let footer = fixture.cx.debug_bounds("settings-footer").unwrap();
        assert!(
            footer.bottom() <= px(760.0),
            "footer still below the window: {footer:?}"
        );
    }

    #[test]
    fn recorder_input_matches_settings_tsx() {
        let input = |text: &str| recorder_input(&Keystroke::parse(text).unwrap());
        assert_eq!(input("escape"), RecorderInput::Cancel);
        assert_eq!(input("backspace"), RecorderInput::Clear);
        assert_eq!(input("shift-backspace"), RecorderInput::Clear);
        assert_eq!(
            input("cmd-backspace"),
            RecorderInput::Accept("Command+Backspace".into())
        );
        assert_eq!(input("a"), RecorderInput::Ignore);
        assert_eq!(input("alt-a"), RecorderInput::Rejected);
        assert_eq!(input("f6"), RecorderInput::Accept("F6".into()));
    }
}
