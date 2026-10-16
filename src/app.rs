use color_eyre::eyre::{Result, WrapErr};
use futures::StreamExt;
use futures::channel::mpsc;
use gpui::{App, Application, KeyBinding, actions};
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;

use crate::actions::{Action, Actions};
use crate::capture_store::CaptureStore;
use crate::hotkeys::Hotkeys;
use crate::paths::Paths;
use crate::settings::model::SettingsModel;

impl gpui::Global for Hotkeys {}

// GPUI has no implicit app menu, so these replace Electron's default ⌘W/⌘Q/⌘H.
actions!(ss, [CloseWindow, QuitApplication, HideApplication]);

/// Runs the menu bar app until it quits.
pub fn run(paths: Paths) -> Result<()> {
    let store = CaptureStore::open(paths.captures.clone())
        .wrap_err_with(|| format!("could not open {}", paths.captures.display()))?;
    Application::new().run(move |cx: &mut App| {
        install_basics(cx);

        let settings = SettingsModel::init(paths.settings_file.clone(), cx);
        let onboarded = settings.read(cx).settings().onboarded;
        cx.set_global(Actions::new(paths.clone(), store));
        crate::setup::Setup::install(crate::setup::SystemSetup, cx);

        // Hotkeys and the tray only hand actions off; GPUI performs them here.
        let (sender, mut receiver) = mpsc::unbounded::<Action>();
        cx.spawn(async move |cx| {
            while let Some(action) = receiver.next().await {
                if cx.update(|cx| crate::actions::perform(action, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();

        let shortcuts = settings.read(cx).settings().shortcuts.clone();
        let hotkey_sender = sender.clone();
        match Hotkeys::install(move |action| {
            let _ = hotkey_sender.unbounded_send(Action::from(action));
        }) {
            Ok(mut hotkeys) => {
                hotkeys.apply(&shortcuts);
                cx.set_global(hotkeys);
            }
            Err(error) => tracing::error!(%error, "could not install global shortcuts"),
        }
        if let Err(error) = crate::tray::install(sender, &shortcuts) {
            tracing::error!(%error, "could not install the menu bar item");
        }

        cx.observe(&settings, |settings, cx| {
            let shortcuts = settings.read(cx).settings().shortcuts.clone();
            let changed = cx
                .try_global::<Hotkeys>()
                .is_none_or(|hotkeys| *hotkeys.shortcuts() != shortcuts);
            if !changed {
                return;
            }
            if cx.has_global::<Hotkeys>() {
                cx.global_mut::<Hotkeys>().apply(&shortcuts);
            }
            crate::tray::update_menu(&shortcuts);
        })
        .detach();
        cx.on_app_quit(|cx| {
            release_hotkeys(cx);
            async {}
        })
        .detach();

        tracing::info!(data = %paths.data.display(), "ss started");
        if !onboarded {
            crate::windows::onboarding::open_or_focus(cx);
        }
    });
    Ok(())
}

/// Accessory policy and the window key bindings every ss process needs.
pub fn install_basics(cx: &mut App) {
    // GPUI switches to Regular when launching finishes, just before this runs.
    set_accessory_activation_policy();
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-q", QuitApplication, None),
        KeyBinding::new("cmd-h", HideApplication, None),
    ]);
    cx.on_action(|_: &QuitApplication, cx| quit(cx));
    cx.on_action(|_: &HideApplication, cx| cx.hide());
}

/// Quits after finishing any recording (plan R7: synchronously, at most
/// 5 s + 2 s, because `on_app_quit` gets too little time), releasing global
/// shortcuts and removing the menu bar item.
pub fn quit(cx: &mut App) {
    if let Some(actions) = cx.try_global::<Actions>() {
        actions.stop_recording_blocking();
    }
    release_hotkeys(cx);
    crate::tray::remove();
    cx.quit();
}

fn release_hotkeys(cx: &mut App) {
    if cx.has_global::<Hotkeys>() {
        cx.global_mut::<Hotkeys>().unregister_all();
    }
}

/// No Dock icon, like `LSUIElement` + `app.dock.hide()` in the Electron build.
fn set_accessory_activation_policy() {
    let Some(marker) = MainThreadMarker::new() else {
        tracing::warn!("activation policy must be set on the main thread");
        return;
    };
    let application = NSApplication::sharedApplication(marker);
    if !application.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
        tracing::warn!("could not switch to the accessory activation policy");
    }
}
