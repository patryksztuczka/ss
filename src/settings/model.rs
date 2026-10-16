//! The GPUI side of settings: one shared entity that windows observe. The
//! model in the parent module stays free of GPUI.

use std::path::PathBuf;

use color_eyre::eyre::Result;
use gpui::{App, AppContext, Context, Entity, Global};

use super::{Settings, SettingsPatch};

pub struct SettingsModel {
    path: PathBuf,
    settings: Settings,
}

struct GlobalSettings(Entity<SettingsModel>);

impl Global for GlobalSettings {}

impl SettingsModel {
    /// Loads settings (persisting shortcut repairs) and installs the global entity.
    pub fn init(path: PathBuf, cx: &mut App) -> Entity<Self> {
        let settings = super::load(&path);
        let entity = cx.new(|_| Self { path, settings });
        cx.set_global(GlobalSettings(entity.clone()));
        entity
    }

    pub fn global(cx: &App) -> Entity<Self> {
        cx.global::<GlobalSettings>().0.clone()
    }

    pub fn try_global(cx: &App) -> Option<Entity<Self>> {
        cx.try_global::<GlobalSettings>()
            .map(|global| global.0.clone())
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Applies the patch, notifies observers, then writes the file. Observers
    /// see the new value even if the write fails, as in the Electron store.
    pub fn update(&mut self, patch: SettingsPatch, cx: &mut Context<Self>) -> Result<()> {
        self.settings.apply(patch);
        cx.notify();
        super::save(&self.path, &self.settings)
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui::TestAppContext;

    use super::*;
    use crate::settings::AfterCapture;

    #[gpui::test]
    fn update_notifies_observers_and_persists(cx: &mut TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let model = cx.update(|cx| SettingsModel::init(path.clone(), cx));
        let notified = Rc::new(Cell::new(0));
        let _subscription = cx.update(|cx| {
            let notified = notified.clone();
            cx.observe(&model, move |_, _| notified.set(notified.get() + 1))
        });

        model
            .update(cx, |model, cx| {
                model.update(
                    SettingsPatch {
                        after_capture: Some(AfterCapture::Editor),
                        ..SettingsPatch::default()
                    },
                    cx,
                )
            })
            .unwrap();
        cx.run_until_parked();

        assert_eq!(notified.get(), 1);
        assert_eq!(
            crate::settings::read(&path).settings.after_capture,
            AfterCapture::Editor
        );
        cx.update(|cx| {
            let global = SettingsModel::global(cx);
            assert_eq!(
                global.read(cx).settings().after_capture,
                AfterCapture::Editor
            );
        });
    }
}
