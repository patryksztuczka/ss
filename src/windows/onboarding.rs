//! The setup guide (port of `renderer/src/windows/Onboarding.tsx`):
//! Welcome → Permissions → Shortcuts → Done.

use std::sync::Arc;
use std::time::Duration;

use gpui::{
    Animation, AnimationExt, AnyElement, App, AsyncApp, Context, Entity, FocusHandle, Focusable,
    FontWeight, Global, Image, ImageFormat, IntoElement, KeyDownEvent, Render, Task, WeakEntity,
    Window, WindowHandle, div, img, linear_color_stop, linear_gradient, prelude::*, px, rgb,
};

use crate::accelerator::accelerator_parts;
use crate::app::CloseWindow;
use crate::desktop_ui::{
    ACCENT, ButtonVariant, EMERALD_500, EMERALD_600, INK, INK_SOFT, KeyCapSize, MUTED, RED_500,
    RISE_IN, TEXT_2XS, TEXT_BASE, TEXT_BODY, TEXT_CONTROL, TEXT_EMPHASIS, TEXT_HINT, TEXT_SM,
    TITLEBAR_HEIGHT, WHITE, accent_soft, alpha, button, card, card_shadows, divided_card,
    ease_out_soft, ease_spring, key_caps, mix_color, onboarding_window_size, rich_text,
    status_pill, step_title, switch_toggle, window_frame, window_options,
};
use crate::settings::model::SettingsModel;
use crate::settings::{SettingsPatch, ShortcutAction, ShortcutPreset};
use crate::setup::Setup;
use crate::system::{ScreenAccess, SetupState};

const ICON: &[u8] = include_bytes!("../../resources/onboarding-icon.png");
const POLL_INTERVAL: Duration = Duration::from_millis(1500);
const DOT_TRANSITION: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Welcome,
    Permissions,
    Shortcuts,
    Done,
}

impl Step {
    const ALL: [Self; 4] = [
        Self::Welcome,
        Self::Permissions,
        Self::Shortcuts,
        Self::Done,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|step| *step == self).unwrap_or(0)
    }

    fn next(self) -> Self {
        Self::ALL
            .get(self.index() + 1)
            .copied()
            .unwrap_or(Self::Done)
    }

    fn previous(self) -> Self {
        Self::ALL[self.index().saturating_sub(1)]
    }
}

const ACTION_LABELS: [(ShortcutAction, &str); 5] = [
    (ShortcutAction::CaptureArea, "Capture area or window"),
    (ShortcutAction::CaptureScreen, "Capture screen"),
    (ShortcutAction::RecordArea, "Record area"),
    (ShortcutAction::RecordScreen, "Record screen"),
    (ShortcutAction::OpenLast, "Edit last capture"),
];

#[derive(Default)]
struct OpenOnboardingWindow(Option<WindowHandle<OnboardingWindow>>);

impl Global for OpenOnboardingWindow {}

/// Focuses the setup guide, opening it if needed.
pub fn open_or_focus(cx: &mut App) {
    if let Err(error) = open_at(Step::Welcome, true, cx) {
        tracing::error!(%error, "could not open the setup guide");
    }
}

/// Opens (or focuses) the guide at `step`; previews pass `activate: false`
/// so they don't take focus from the user.
pub fn open_at(
    step: Step,
    activate: bool,
    cx: &mut App,
) -> gpui::Result<WindowHandle<OnboardingWindow>> {
    if let Some(handle) = cx.default_global::<OpenOnboardingWindow>().0
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        cx.activate(true);
        return Ok(handle);
    }
    let settings = SettingsModel::global(cx);
    let mut options = window_options("Welcome to ss", onboarding_window_size(), cx);
    options.focus = activate;
    let handle = cx.open_window(options, |window, cx| {
        cx.new(|cx| OnboardingWindow::new(settings, step, window, cx))
    })?;
    cx.set_global(OpenOnboardingWindow(Some(handle)));
    if activate {
        cx.activate(true);
    }
    Ok(handle)
}

pub struct OnboardingWindow {
    focus: FocusHandle,
    settings: Entity<SettingsModel>,
    setup: Option<SetupState>,
    step: Step,
    /// The step before the last transition, for the progress-dot animation.
    previous_step: Step,
    preset: ShortcutPreset,
    busy: bool,
    failed: Vec<ShortcutAction>,
    requested: bool,
    poll: Option<Task<()>>,
    icon: Arc<Image>,
}

impl OnboardingWindow {
    fn new(
        settings: Entity<SettingsModel>,
        step: Step,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        focus.focus(window);
        let current = settings.read(cx).settings();
        let alongside = ShortcutPreset::Alongside.shortcuts();
        let preset =
            if current.onboarded && current.shortcuts.capture_area == alongside.capture_area {
                ShortcutPreset::Alongside
            } else {
                ShortcutPreset::System
            };
        let mut this = Self {
            focus,
            settings,
            setup: None,
            step,
            previous_step: step,
            preset,
            busy: false,
            failed: Vec::new(),
            requested: false,
            poll: None,
            icon: Arc::new(Image::from_bytes(ImageFormat::Png, ICON.to_vec())),
        };
        this.refresh_setup(cx);
        this
    }

    /// `ss preview onboarding --failed`.
    pub fn show_failures(&mut self, failed: Vec<ShortcutAction>, cx: &mut Context<Self>) {
        self.failed = failed;
        cx.notify();
    }

    fn go(&mut self, step: Step, cx: &mut Context<Self>) {
        self.previous_step = self.step;
        self.step = step;
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
                if state.screen_access == ScreenAccess::Granted {
                    this.poll = None;
                }
                this.setup = Some(state);
                cx.notify();
            })
        })
        .detach();
    }

    fn primary_label(&self) -> &'static str {
        let granted = self
            .setup
            .as_ref()
            .is_some_and(|setup| setup.screen_access == ScreenAccess::Granted);
        match self.step {
            Step::Welcome => "Get Started",
            Step::Permissions if !granted => "Skip for Now",
            Step::Shortcuts => "Use These Shortcuts",
            Step::Done => "Start Using ss",
            Step::Permissions => "Continue",
        }
    }

    fn next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self.step {
            Step::Shortcuts => {
                self.busy = true;
                cx.notify();
                let task = Setup::global(cx).apply_preset(self.preset, cx);
                cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                    let result = task.await;
                    this.update(cx, |this, cx| {
                        match result {
                            Ok(outcome) => {
                                this.failed = outcome.failed;
                                // Stay on this step until every shortcut is registered.
                                if this.failed.is_empty() {
                                    this.go(Step::Done, cx);
                                }
                            }
                            Err(error) => tracing::error!(%error, "could not apply shortcuts"),
                        }
                        this.busy = false;
                        this.refresh_setup(cx);
                    })
                })
                .detach();
            }
            Step::Done => {
                let result = self.settings.update(cx, |model, cx| {
                    model.update(
                        SettingsPatch {
                            onboarded: Some(true),
                            ..SettingsPatch::default()
                        },
                        cx,
                    )
                });
                if let Err(error) = result {
                    tracing::error!(%error, "could not save onboarding");
                }
                window.remove_window();
            }
            step => self.go(step.next(), cx),
        }
    }

    fn back(&mut self, cx: &mut Context<Self>) {
        self.go(self.step.previous(), cx);
    }

    /// Asks once, then polls every 1.5 s for the toggle in System Settings.
    fn request_screen_access(&mut self, cx: &mut Context<Self>) {
        self.requested = true;
        let task = Setup::global(cx).request_screen_access(cx);
        self.set_setup_when_ready(task, cx);
        if self.poll.is_none() {
            self.poll = Some(
                cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
                    loop {
                        cx.background_executor().timer(POLL_INTERVAL).await;
                        let refreshed = this.update(cx, |this, cx| this.refresh_setup(cx));
                        if refreshed.is_err() {
                            break;
                        }
                    }
                }),
            );
        }
        cx.notify();
    }

    fn render_dots(&self) -> impl IntoElement {
        let from = self.previous_step.index();
        let to = self.step.index();
        div()
            .h(px(TITLEBAR_HEIGHT))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .gap(px(6.0))
            .children((0..Step::ALL.len()).map(move |index| {
                let (from_width, from_color) = dot(index, from);
                let (to_width, to_color) = dot(index, to);
                div()
                    .id(("dot", index * 10 + to))
                    .debug_selector(move || format!("dot-{index}"))
                    .h(px(6.0))
                    .w(px(to_width))
                    .rounded_full()
                    .bg(to_color)
                    .with_animation(
                        ("dot-animation", index * 10 + to),
                        Animation::new(DOT_TRANSITION).with_easing(ease_out_soft()),
                        move |dot, delta| {
                            dot.w(px(from_width + (to_width - from_width) * delta))
                                .bg(mix_color(from_color, to_color, delta))
                        },
                    )
            }))
    }

    fn render_welcome(&self) -> AnyElement {
        let features = [
            (
                "Clipboard first",
                "Every capture is ready to paste. Nothing lands on your Desktop.",
            ),
            (
                "Nice backgrounds",
                "Turn a raw screenshot into something worth sharing in a second.",
            ),
            (
                "Selectable text",
                "Copy words straight out of any screenshot, on-device.",
            ),
        ];
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .pb(px(24.0))
            .child(img(self.icon.clone()).size(px(104.0)).mb(px(20.0)))
            .child(step_title(
                "Welcome to ss",
                "Screenshots and recordings that go straight to your clipboard. Let's set things up — it takes less than a minute.",
            ))
            .child(
                div()
                    .w_full()
                    .max_w(px(600.0))
                    .flex()
                    .gap(px(12.0))
                    .children(features.into_iter().map(|(title, text)| {
                        card()
                            .flex_1()
                            .min_w_0()
                            .p_4()
                            .child(
                                div()
                                    .text_size(px(TEXT_BASE))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .mt_1()
                                    .text_size(px(TEXT_SM))
                                    .line_height(px(19.5))
                                    .text_color(rgb(INK_SOFT))
                                    .child(text),
                            )
                    })),
            )
            .into_any_element()
    }

    fn render_permissions(&self, setup: &SetupState, cx: &mut Context<Self>) -> AnyElement {
        let granted = setup.screen_access == ScreenAccess::Granted;
        let app_name = setup.permission_host.as_deref().unwrap_or("ss");
        let note: Option<String> = if granted {
            Some("You're good to go.".into())
        } else if self.requested {
            Some(format!(
                "In System Settings, switch on **{app_name}** under Screen & System Audio Recording. This page updates by itself."
            ))
        } else {
            setup.permission_host.as_ref().map(|host| {
                format!(
                    "Running in development: macOS asks **{host}** (the app that started ss) for this permission."
                )
            })
        };
        let icon = div()
            .size(px(44.0))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(11.0))
            .bg(linear_gradient(
                135.0,
                linear_color_stop(rgb(0x8fe6f8), 0.0),
                linear_color_stop(rgb(0x9a7cf2), 1.0),
            ))
            .child(
                gpui_symbols::Icon::new("display")
                    .size(px(22.0))
                    .color(rgb(WHITE))
                    .weight(gpui_symbols::SymbolWeight::Medium),
            );
        div()
            .w_full()
            .max_w(px(560.0))
            .flex_1()
            .flex()
            .flex_col()
            .justify_center()
            .pb(px(32.0))
            .child(step_title(
                "One permission",
                "ss needs to see your screen to capture it. Everything stays on your Mac.",
            ))
            .child(
                card()
                    .flex_row()
                    .items_center()
                    .gap_4()
                    .p_4()
                    .child(icon)
                    .child(
                        div()
                            .min_w_0()
                            .flex_1()
                            .child(
                                div()
                                    .text_size(px(TEXT_EMPHASIS))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("Screen Recording"),
                            )
                            .child(
                                div()
                                    .text_size(px(TEXT_CONTROL))
                                    .text_color(rgb(INK_SOFT))
                                    .child("Required for screenshots and recordings."),
                            ),
                    )
                    .child(if granted {
                        status_pill(true, "Granted").into_any_element()
                    } else {
                        button(
                            "grant-access",
                            if self.requested {
                                "Open Settings"
                            } else {
                                "Grant Access"
                            },
                            ButtonVariant::Primary,
                            None,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.request_screen_access(cx)))
                        .into_any_element()
                    }),
            )
            .child(
                div()
                    .mt_4()
                    .min_h(px(60.0))
                    .text_center()
                    .text_size(px(TEXT_CONTROL))
                    .line_height(px(20.0))
                    .text_color(rgb(INK_SOFT))
                    .when_some(note, |note_box, text| note_box.child(rich_text(&text))),
            )
            .into_any_element()
    }

    fn render_shortcuts(&self, cx: &mut Context<Self>) -> AnyElement {
        let options = [
            (
                ShortcutPreset::System,
                "Replace macOS shortcuts",
                "ss takes over the keys you already know. The macOS screenshot shortcuts are switched off — you can bring them back in Settings.",
                Some("Recommended"),
            ),
            (
                ShortcutPreset::Alongside,
                "Keep macOS shortcuts",
                "ss uses its own keys, and the built-in screenshot tool keeps working as before.",
                None,
            ),
        ];
        let keys = self.preset.shortcuts();
        let cards = options
            .into_iter()
            .enumerate()
            .map(|(index, (preset, title, text, badge))| {
                let selected = preset == self.preset;
                let sample = preset.shortcuts();
                card()
                    .id(("preset", index))
                    .debug_selector(move || format!("preset-{index}"))
                    .flex_1()
                    .min_w_0()
                    .p_4()
                    .shadow(card_shadows(selected))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.preset = preset;
                        cx.notify();
                    }))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(TEXT_BODY))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(title),
                            )
                            .when_some(badge, |header, badge| {
                                header.child(
                                    div()
                                        .px_2()
                                        .py(px(2.0))
                                        .rounded_full()
                                        .bg(accent_soft())
                                        .text_size(px(TEXT_2XS))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(rgb(ACCENT))
                                        .child(badge),
                                )
                            }),
                    )
                    .child(
                        div().mt_3().flex().gap_2().children(
                            [
                                &sample.capture_screen,
                                &sample.capture_area,
                                &sample.record_area,
                            ]
                            .into_iter()
                            .map(|accelerator| {
                                key_caps(&accelerator_parts(accelerator), KeyCapSize::Medium)
                            }),
                        ),
                    )
                    .child(
                        div()
                            .mt_3()
                            .text_size(px(TEXT_SM))
                            .line_height(px(19.5))
                            .text_color(rgb(INK_SOFT))
                            .child(text),
                    )
            });
        let rows = ACTION_LABELS
            .iter()
            .enumerate()
            .map(|(index, &(action, label))| {
                let failed = self.failed.contains(&action);
                div()
                    .debug_selector(move || format!("preset-row-{index}"))
                    .h(px(36.0))
                    .px_4()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(TEXT_CONTROL))
                            .text_color(rgb(if failed { RED_500 } else { INK }))
                            .child(if failed {
                                format!("{label} — taken by another app")
                            } else {
                                label.to_string()
                            }),
                    )
                    .child(key_caps(
                        &accelerator_parts(keys.get(action)),
                        KeyCapSize::Medium,
                    ))
                    .into_any_element()
            });
        div()
            .w_full()
            .max_w(px(620.0))
            .flex_1()
            .flex()
            .flex_col()
            .justify_center()
            .pb(px(16.0))
            .child(step_title(
                "Your shortcuts",
                "Pick the keys you'll press every day.",
            ))
            .child(div().flex().gap(px(12.0)).children(cards))
            .child(divided_card(rows).mt_4())
            .into_any_element()
    }

    fn render_done(&self, setup: &SetupState, cx: &mut Context<Self>) -> AnyElement {
        let capture_area = self
            .settings
            .read(cx)
            .settings()
            .shortcuts
            .capture_area
            .clone();
        let login = setup.launch_at_login;
        div()
            .w_full()
            .max_w(px(520.0))
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .pb(px(24.0))
            .child(
                div()
                    .size(px(64.0))
                    .mb(px(20.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(alpha(EMERALD_500, 0.12))
                    .child(
                        gpui_symbols::Icon::new("checkmark")
                            .size(px(28.0))
                            .color(rgb(EMERALD_600))
                            .weight(gpui_symbols::SymbolWeight::Semibold),
                    )
                    .with_animation(
                        "done-check",
                        Animation::new(Duration::from_millis(600)),
                        |check, delta| {
                            // The spring overshoots, so it is applied here rather than as easing.
                            let progress = ease_spring()(delta);
                            check
                                .opacity(progress.min(1.0))
                                .mt(px(8.0 * (1.0 - progress)))
                        },
                    ),
            )
            .child(step_title(
                "You're all set",
                "ss lives in your menu bar. Try it now — press the shortcut below.",
            ))
            .child(
                div()
                    .mb(px(28.0))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_2()
                    .child(key_caps(
                        &accelerator_parts(&capture_area),
                        KeyCapSize::Large,
                    ))
                    .child(
                        div()
                            .text_size(px(TEXT_SM))
                            .text_color(rgb(MUTED))
                            .child("Capture an area · press Space for a window"),
                    ),
            )
            .child(
                card()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .px_4()
                    .py_3()
                    .child(
                        div()
                            .child(
                                div()
                                    .text_size(px(TEXT_BASE))
                                    .font_weight(FontWeight::MEDIUM)
                                    .child("Launch ss at login"),
                            )
                            .child(div().text_size(px(TEXT_HINT)).text_color(rgb(MUTED)).child(
                                if login.available {
                                    "So your shortcuts always work."
                                } else {
                                    "Available in the installed app."
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("launch-at-login")
                            .child(switch_toggle(
                                if login.enabled { 1.0 } else { 0.0 },
                                !login.available,
                            ))
                            .when(login.available, |switch| {
                                switch.on_click(cx.listener(move |this, _, _, cx| {
                                    let task =
                                        Setup::global(cx).set_launch_at_login(!login.enabled, cx);
                                    this.set_setup_when_ready(task, cx);
                                }))
                            }),
                    ),
            )
            .into_any_element()
    }
}

/// A progress dot's width and colour: wide ink for the current step,
/// faded for done (40%) and upcoming (12%) steps.
fn dot(index: usize, current: usize) -> (f32, gpui::Rgba) {
    let (width, opacity) = match index.cmp(&current) {
        std::cmp::Ordering::Equal => (20.0, 1.0),
        std::cmp::Ordering::Less => (6.0, 0.4),
        std::cmp::Ordering::Greater => (6.0, 0.12),
    };
    (width, alpha(INK, opacity))
}

/// Back is hidden on the first step and once setup is done.
fn shows_back(step: Step) -> bool {
    step != Step::Welcome && step != Step::Done
}

impl Focusable for OnboardingWindow {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for OnboardingWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let frame = window_frame()
            .track_focus(&self.focus)
            .on_action(|_: &CloseWindow, window, _| window.remove_window())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "enter" && !this.busy {
                    this.next(window, cx);
                }
            }));
        let Some(setup) = self.setup.clone() else {
            return frame;
        };
        let step = self.step;
        let content = match step {
            Step::Welcome => self.render_welcome(),
            Step::Permissions => self.render_permissions(&setup, cx),
            Step::Shortcuts => self.render_shortcuts(cx),
            Step::Done => self.render_done(&setup, cx),
        };
        let show_back = shows_back(step);
        frame
            .child(self.render_dots())
            .child(
                div()
                    .id(("step", step.index()))
                    .debug_selector(|| "onboarding-step".into())
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .items_center()
                    .px(px(56.0))
                    .child(content)
                    .with_animation(
                        ("step-rise-in", step.index()),
                        Animation::new(RISE_IN).with_easing(ease_out_soft()),
                        |content, delta| content.opacity(delta).mt(px(8.0 * (1.0 - delta))),
                    ),
            )
            .child(
                div()
                    .h(px(72.0))
                    .flex_none()
                    .px_8()
                    .flex()
                    .items_center()
                    .justify_between()
                    .child(div().when(show_back, |slot| {
                        slot.child(
                            div()
                                .id("back")
                                .debug_selector(|| "onboarding-back".into())
                                .text_size(px(TEXT_BASE))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(INK_SOFT))
                                .hover(|back| back.text_color(rgb(INK)))
                                .on_click(cx.listener(|this, _, _, cx| this.back(cx)))
                                .child("Back"),
                        )
                    }))
                    .child(
                        button(
                            "primary",
                            self.primary_label(),
                            ButtonVariant::Primary,
                            Some("↩".into()),
                        )
                        .debug_selector(|| "onboarding-primary".into())
                        .h(px(36.0))
                        .px_4()
                        .when(self.busy, |button| button.opacity(0.4))
                        .on_click(cx.listener(|this, _, window, cx| this.next(window, cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{Modifiers, TestAppContext, VisualTestContext, size};

    use super::*;
    use crate::settings;
    use crate::setup::FakeSetup;

    struct Fixture<'a> {
        _directory: tempfile::TempDir,
        path: PathBuf,
        view: Entity<OnboardingWindow>,
        cx: &'a mut VisualTestContext,
    }

    fn open(cx: &mut TestAppContext, failed: Vec<ShortcutAction>) -> Fixture<'_> {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let model = cx.update(|cx| {
            let mut setup = FakeSetup::new(ScreenAccess::Denied);
            setup.failed = failed;
            Setup::install(setup, cx);
            SettingsModel::init(path.clone(), cx)
        });
        let (view, cx) = cx
            .add_window_view(|window, cx| OnboardingWindow::new(model, Step::Welcome, window, cx));
        cx.simulate_resize(size(px(760.0), px(560.0)));
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
            let bounds = self
                .cx
                .debug_bounds(selector)
                .unwrap_or_else(|| panic!("{selector} is not rendered"));
            self.cx.simulate_click(bounds.center(), Modifiers::none());
            self.cx.run_until_parked();
        }

        fn step(&mut self) -> Step {
            self.view.read_with(self.cx, |view, _| view.step)
        }
    }

    #[gpui::test]
    fn walks_through_every_step_and_finishes(cx: &mut TestAppContext) {
        let mut fixture = open(cx, Vec::new());
        assert_eq!(fixture.step(), Step::Welcome);
        fixture.click("onboarding-primary");
        assert_eq!(fixture.step(), Step::Permissions);
        fixture.click("onboarding-back");
        assert_eq!(fixture.step(), Step::Welcome);
        // Enter triggers the primary button.
        fixture.cx.simulate_keystrokes("enter");
        fixture.cx.run_until_parked();
        assert_eq!(fixture.step(), Step::Permissions);
        fixture.click("onboarding-primary");
        assert_eq!(fixture.step(), Step::Shortcuts);

        fixture.click("onboarding-primary");
        assert_eq!(fixture.step(), Step::Done);
        let saved = settings::read(&fixture.path).settings;
        assert_eq!(saved.shortcuts, ShortcutPreset::System.shortcuts());
        assert!(!saved.onboarded);

        let window = fixture.cx.window_handle();
        fixture.click("onboarding-primary");
        assert!(settings::read(&fixture.path).settings.onboarded);
        assert!(!fixture.cx.cx.windows().contains(&window));
    }

    #[gpui::test]
    fn stays_on_shortcuts_when_a_shortcut_fails(cx: &mut TestAppContext) {
        let mut fixture = open(cx, vec![ShortcutAction::RecordScreen]);
        fixture.click("onboarding-primary");
        fixture.click("onboarding-primary");
        assert_eq!(fixture.step(), Step::Shortcuts);
        fixture.click("preset-1");
        fixture.click("onboarding-primary");
        assert_eq!(fixture.step(), Step::Shortcuts);
        let failed = fixture
            .view
            .read_with(fixture.cx, |view, _| view.failed.clone());
        assert_eq!(failed, [ShortcutAction::RecordScreen]);
        assert_eq!(
            settings::read(&fixture.path).settings.shortcuts,
            ShortcutPreset::Alongside.shortcuts()
        );
    }

    #[gpui::test]
    fn initial_preset_follows_onboarding_ts(cx: &mut TestAppContext) {
        let fixture = open(cx, Vec::new());
        let preset = fixture.view.read_with(fixture.cx, |view, _| view.preset);
        assert_eq!(preset, ShortcutPreset::System);
    }

    #[test]
    fn progress_dots_widen_the_current_step() {
        assert_eq!(dot(1, 1), (20.0, alpha(INK, 1.0)));
        assert_eq!(dot(0, 1), (6.0, alpha(INK, 0.4)));
        assert_eq!(dot(3, 1), (6.0, alpha(INK, 0.12)));
    }

    #[test]
    fn back_is_hidden_on_the_first_and_last_steps() {
        let visible: Vec<_> = Step::ALL.into_iter().map(shows_back).collect();
        assert_eq!(visible, [false, true, true, false]);
    }
}
