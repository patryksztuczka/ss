//! Launch at login via `SMAppService.mainAppService` (from HEX). macOS owns
//! the state; ss polls it rather than persisting a flag. Only meaningful for
//! the signed, installed bundle. Blocking; call from the background executor.

use color_eyre::eyre::{Result, eyre};
use objc2_service_management::{SMAppService, SMAppServiceStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Disabled,
    Enabled,
    RequiresApproval,
}

pub fn status() -> Result<Status> {
    objc2::rc::autoreleasepool(|_| {
        // SAFETY: mainAppService and status do not retain caller-owned pointers.
        let status = unsafe { SMAppService::mainAppService().status() };
        match status {
            SMAppServiceStatus::NotRegistered | SMAppServiceStatus::NotFound => {
                Ok(Status::Disabled)
            }
            SMAppServiceStatus::Enabled => Ok(Status::Enabled),
            SMAppServiceStatus::RequiresApproval => Ok(Status::RequiresApproval),
            _ => Err(eyre!("macOS returned an unknown login item status")),
        }
    })
}

pub fn set_enabled(enabled: bool) -> Result<Status> {
    objc2::rc::autoreleasepool(|_| {
        // SAFETY: SMAppService owns the NSError produced by these synchronous APIs.
        let service = unsafe { SMAppService::mainAppService() };
        let current = status()?;
        if enabled && current == Status::Disabled {
            unsafe { service.registerAndReturnError() }
                .map_err(|error| eyre!(error.to_string()))?;
        } else if !enabled && current != Status::Disabled {
            unsafe { service.unregisterAndReturnError() }
                .map_err(|error| eyre!(error.to_string()))?;
        }
        status()
    })
}

/// Opens System Settings → General → Login Items.
pub fn open_settings() {
    // SAFETY: asks macOS to open its Login Items pane.
    unsafe { SMAppService::openSystemSettingsLoginItems() };
}
