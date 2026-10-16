//! What Settings and the setup guide do to the system (the old `setup:*`
//! IPC channels). Windows talk to a [`SetupBackend`] global so tests and
//! previews can substitute one that never touches macOS.

use std::cell::RefCell;
use std::rc::Rc;

use gpui::{App, AppContext, Global, Task};
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};

use crate::hotkeys::Hotkeys;
use crate::settings::model::SettingsModel;
use crate::settings::{Settings, SettingsPatch, ShortcutAction, ShortcutPreset, Shortcuts};
use crate::system::{self, ScreenAccess, SetupState};

#[derive(Clone, Debug, PartialEq)]
pub struct PresetOutcome {
    pub settings: Settings,
    pub failed: Vec<ShortcutAction>,
}

pub trait SetupBackend {
    fn state(&self, cx: &mut App) -> Task<SetupState>;
    /// Prompts for Screen Recording (first time only), opens System Settings
    /// if still denied, then reports the new state.
    fn request_screen_access(&self, cx: &mut App) -> Task<SetupState>;
    /// Hands ⇧⌘3/4/5 to ss ("system") or back to macOS ("alongside"), saves
    /// the preset's shortcuts and registers them.
    fn apply_preset(
        &self,
        preset: ShortcutPreset,
        cx: &mut App,
    ) -> Task<Result<PresetOutcome, String>>;
    fn set_launch_at_login(&self, enabled: bool, cx: &mut App) -> Task<SetupState>;
    fn open_login_items(&self, cx: &mut App);
}

#[derive(Clone)]
pub struct Setup(Rc<dyn SetupBackend>);

impl Global for Setup {}

impl Setup {
    pub fn install(backend: impl SetupBackend + 'static, cx: &mut App) {
        cx.set_global(Self(Rc::new(backend)));
    }

    pub fn global(cx: &App) -> Rc<dyn SetupBackend> {
        cx.global::<Self>().0.clone()
    }
}

/// The real backend.
pub struct SystemSetup;

impl SetupBackend for SystemSetup {
    fn state(&self, cx: &mut App) -> Task<SetupState> {
        cx.background_spawn(async { system::setup_state() })
    }

    fn request_screen_access(&self, cx: &mut App) -> Task<SetupState> {
        let request = cx.background_spawn(async { system::request_screen_access() });
        cx.spawn(async move |cx| {
            if request.await == ScreenAccess::Denied {
                let _ = cx.update(|_| open_url(system::SCREEN_RECORDING_SETTINGS));
            }
            cx.background_spawn(async { system::setup_state() }).await
        })
    }

    fn apply_preset(
        &self,
        preset: ShortcutPreset,
        cx: &mut App,
    ) -> Task<Result<PresetOutcome, String>> {
        // macOS must release ⇧⌘3/4/5 before ss can claim them, and get them back otherwise.
        let enabled = preset == ShortcutPreset::Alongside;
        let toggle = cx.background_spawn(async move { system::set_screenshot_shortcuts(enabled) });
        cx.spawn(async move |cx| {
            toggle.await.map_err(|error| error.to_string())?;
            cx.update(|cx| save_preset(preset, cx))
                .map_err(|error| error.to_string())
        })
    }

    fn set_launch_at_login(&self, enabled: bool, cx: &mut App) -> Task<SetupState> {
        cx.background_spawn(async move {
            if system::is_bundled()
                && let Err(error) = crate::login_item::set_enabled(enabled)
            {
                tracing::warn!(%error, enabled, "could not change launch at login");
            }
            system::setup_state()
        })
    }

    fn open_login_items(&self, cx: &mut App) {
        cx.background_spawn(async { crate::login_item::open_settings() })
            .detach();
    }
}

/// Saves `shortcuts` and registers them. Returns the actions that failed.
pub fn save_shortcuts(shortcuts: Shortcuts, cx: &mut App) -> Vec<ShortcutAction> {
    let settings = SettingsModel::global(cx);
    let result = settings.update(cx, |model, cx| {
        model.update(
            SettingsPatch {
                shortcuts: Some(shortcuts.clone()),
                ..SettingsPatch::default()
            },
            cx,
        )
    });
    if let Err(error) = result {
        tracing::error!(%error, "could not save shortcuts");
    }
    match cx.try_global::<Hotkeys>() {
        Some(_) => cx.global_mut::<Hotkeys>().apply(&shortcuts),
        None => Vec::new(),
    }
}

fn save_preset(preset: ShortcutPreset, cx: &mut App) -> PresetOutcome {
    let failed = save_shortcuts(preset.shortcuts(), cx);
    PresetOutcome {
        settings: SettingsModel::global(cx).read(cx).settings().clone(),
        failed,
    }
}

/// Releases global shortcuts while Settings records a new one.
pub fn pause_hotkeys(paused: bool, cx: &mut App) {
    if cx.has_global::<Hotkeys>() {
        cx.global_mut::<Hotkeys>().pause(paused);
    }
}

fn open_url(url: &str) {
    if let Some(url) = NSURL::URLWithString(&NSString::from_str(url)) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}

/// A backend for tests and `ss preview`: fixed state, no system changes.
/// Presets are saved to settings and report `failed`.
pub struct FakeSetup {
    pub state: RefCell<SetupState>,
    pub failed: Vec<ShortcutAction>,
}

impl FakeSetup {
    pub fn new(screen_access: ScreenAccess) -> Self {
        Self {
            state: RefCell::new(SetupState {
                screen_access,
                system_screenshot_shortcuts: true,
                launch_at_login: system::LaunchAtLogin::default(),
                permission_host: Some("T3 Code".into()),
            }),
            failed: Vec::new(),
        }
    }
}

impl SetupBackend for FakeSetup {
    fn state(&self, _: &mut App) -> Task<SetupState> {
        Task::ready(self.state.borrow().clone())
    }

    fn request_screen_access(&self, _: &mut App) -> Task<SetupState> {
        Task::ready(self.state.borrow().clone())
    }

    fn apply_preset(
        &self,
        preset: ShortcutPreset,
        cx: &mut App,
    ) -> Task<Result<PresetOutcome, String>> {
        self.state.borrow_mut().system_screenshot_shortcuts = preset == ShortcutPreset::Alongside;
        let mut outcome = save_preset(preset, cx);
        outcome.failed = self.failed.clone();
        Task::ready(Ok(outcome))
    }

    fn set_launch_at_login(&self, enabled: bool, _: &mut App) -> Task<SetupState> {
        let mut state = self.state.borrow_mut();
        if state.launch_at_login.available {
            state.launch_at_login.enabled = enabled;
        }
        Task::ready(state.clone())
    }

    fn open_login_items(&self, _: &mut App) {}
}
