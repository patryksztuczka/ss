//! Global shortcuts via Carbon `RegisterEventHotKey`, the API Electron's
//! `globalShortcut` uses on macOS. It needs no permission, and it shares
//! Electron's semantics: ⌥-only chords register but never fire, and a chord
//! another app owns fails with `eventHotKeyExistsErr`.

use std::ffi::c_void;
use std::ptr;

use color_eyre::eyre::{Result, eyre};

use crate::accelerator::{self, Accelerator};
use crate::settings::{ShortcutAction, Shortcuts, is_usable_accelerator};

type OsStatus = i32;
type EventTargetRef = *mut c_void;
type EventHandlerRef = *mut c_void;
type EventHandlerCallRef = *mut c_void;
type EventRef = *mut c_void;
type EventHotKeyRef = *mut c_void;
type EventHandlerProc = extern "C" fn(EventHandlerCallRef, EventRef, *mut c_void) -> OsStatus;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EventHotKeyId {
    signature: u32,
    id: u32,
}

#[repr(C)]
struct EventTypeSpec {
    event_class: u32,
    event_kind: u32,
}

const fn four_char_code(code: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*code)
}

const NO_ERR: OsStatus = 0;
const EVENT_NOT_HANDLED_ERR: OsStatus = -9874;
const EVENT_HOT_KEY_EXISTS_ERR: OsStatus = -9878;
const EVENT_CLASS_KEYBOARD: u32 = four_char_code(b"keyb");
const EVENT_HOT_KEY_PRESSED: u32 = 5;
const EVENT_PARAM_DIRECT_OBJECT: u32 = four_char_code(b"----");
const TYPE_EVENT_HOT_KEY_ID: u32 = four_char_code(b"hkid");
const SIGNATURE: u32 = four_char_code(b"ssHK");

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn GetApplicationEventTarget() -> EventTargetRef;
    fn InstallEventHandler(
        target: EventTargetRef,
        handler: EventHandlerProc,
        type_count: usize,
        types: *const EventTypeSpec,
        user_data: *mut c_void,
        handler_ref: *mut EventHandlerRef,
    ) -> OsStatus;
    fn RemoveEventHandler(handler: EventHandlerRef) -> OsStatus;
    fn RegisterEventHotKey(
        key_code: u32,
        modifiers: u32,
        id: EventHotKeyId,
        target: EventTargetRef,
        options: u32,
        hot_key: *mut EventHotKeyRef,
    ) -> OsStatus;
    fn UnregisterEventHotKey(hot_key: EventHotKeyRef) -> OsStatus;
    fn GetEventParameter(
        event: EventRef,
        name: u32,
        desired_type: u32,
        actual_type: *mut u32,
        buffer_size: usize,
        actual_size: *mut usize,
        data: *mut c_void,
    ) -> OsStatus;
}

type PressHandler = Box<dyn Fn(ShortcutAction)>;

/// Owns the registered hotkeys and the Carbon handler. Main thread only;
/// dropping it unregisters everything.
pub struct Hotkeys {
    handler: EventHandlerRef,
    on_press: *mut PressHandler,
    registered: Vec<EventHotKeyRef>,
    shortcuts: Shortcuts,
    failed: Vec<ShortcutAction>,
}

impl Hotkeys {
    /// Installs the hotkey handler. `on_press` runs on the main thread from
    /// the Carbon callback, so it should only hand the action off.
    pub fn install(on_press: impl Fn(ShortcutAction) + 'static) -> Result<Self> {
        let on_press: *mut PressHandler = Box::into_raw(Box::new(Box::new(on_press)));
        let spec = EventTypeSpec {
            event_class: EVENT_CLASS_KEYBOARD,
            event_kind: EVENT_HOT_KEY_PRESSED,
        };
        let mut handler = ptr::null_mut();
        // SAFETY: `on_press` stays alive until `Drop` removes the handler.
        let status = unsafe {
            InstallEventHandler(
                GetApplicationEventTarget(),
                handle_hot_key,
                1,
                &spec,
                on_press.cast(),
                &mut handler,
            )
        };
        if status != NO_ERR {
            // SAFETY: Carbon did not keep the pointer.
            drop(unsafe { Box::from_raw(on_press) });
            return Err(eyre!("InstallEventHandler failed ({status})"));
        }
        Ok(Self {
            handler,
            on_press,
            registered: Vec::new(),
            shortcuts: Shortcuts::default(),
            failed: Vec::new(),
        })
    }

    /// Registers `shortcuts` in place of the current set and returns the
    /// actions that could not be registered.
    pub fn apply(&mut self, shortcuts: &Shortcuts) -> Vec<ShortcutAction> {
        self.shortcuts = shortcuts.clone();
        self.unregister_all();
        let (planned, mut failed) = plan(shortcuts);
        for (action, accelerator) in planned {
            match register(action, &accelerator) {
                Ok(hot_key) => self.registered.push(hot_key),
                Err(status) => {
                    if status != EVENT_HOT_KEY_EXISTS_ERR {
                        tracing::warn!(?action, status, "RegisterEventHotKey failed");
                    }
                    failed.push(action);
                }
            }
        }
        // Keep the settings order regardless of which step failed.
        failed.sort_by_key(|action| ShortcutAction::ALL.iter().position(|a| a == action));
        if !failed.is_empty() {
            tracing::warn!(?failed, "shortcuts not registered");
        }
        self.failed = failed.clone();
        failed
    }

    /// Releases every hotkey while Settings records a new one; resuming
    /// registers the last applied shortcuts again.
    #[allow(dead_code, reason = "called by the Settings shortcut recorder (M4)")]
    pub fn pause(&mut self, paused: bool) {
        if paused {
            self.unregister_all();
        } else {
            let shortcuts = self.shortcuts.clone();
            self.apply(&shortcuts);
        }
    }

    pub fn shortcuts(&self) -> &Shortcuts {
        &self.shortcuts
    }

    #[allow(dead_code, reason = "shown as \"in use elsewhere\" by Settings (M4)")]
    pub fn failed(&self) -> &[ShortcutAction] {
        &self.failed
    }

    pub fn unregister_all(&mut self) {
        for hot_key in self.registered.drain(..) {
            // SAFETY: each ref came from a successful RegisterEventHotKey.
            unsafe { UnregisterEventHotKey(hot_key) };
        }
    }
}

impl Drop for Hotkeys {
    fn drop(&mut self) {
        self.unregister_all();
        // SAFETY: the handler is removed before the closure it points to is freed.
        unsafe {
            RemoveEventHandler(self.handler);
            drop(Box::from_raw(self.on_press));
        }
    }
}

/// Splits shortcuts into what to register and what fails up front, as
/// `Shortcuts.ts` does: empty strings are skipped, ⌥-only or unparseable
/// accelerators fail.
fn plan(shortcuts: &Shortcuts) -> (Vec<(ShortcutAction, Accelerator)>, Vec<ShortcutAction>) {
    let mut planned = Vec::new();
    let mut failed = Vec::new();
    for action in ShortcutAction::ALL {
        let text = shortcuts.get(action);
        if text.is_empty() {
            continue;
        }
        match accelerator::parse(text) {
            Ok(parsed) if is_usable_accelerator(text) => planned.push((action, parsed)),
            _ => failed.push(action),
        }
    }
    (planned, failed)
}

fn action_id(action: ShortcutAction) -> u32 {
    ShortcutAction::ALL
        .iter()
        .position(|candidate| *candidate == action)
        .expect("every action is listed") as u32
}

fn register(action: ShortcutAction, accelerator: &Accelerator) -> Result<EventHotKeyRef, OsStatus> {
    let id = EventHotKeyId {
        signature: SIGNATURE,
        id: action_id(action),
    };
    let mut hot_key = ptr::null_mut();
    // SAFETY: plain value arguments and a valid out-pointer.
    let status = unsafe {
        RegisterEventHotKey(
            u32::from(accelerator.key_code),
            accelerator.modifiers.carbon(),
            id,
            GetApplicationEventTarget(),
            0,
            &mut hot_key,
        )
    };
    if status == NO_ERR {
        Ok(hot_key)
    } else {
        Err(status)
    }
}

extern "C" fn handle_hot_key(
    _: EventHandlerCallRef,
    event: EventRef,
    user_data: *mut c_void,
) -> OsStatus {
    let mut id = EventHotKeyId::default();
    // SAFETY: reads a fixed-size EventHotKeyID into a matching buffer.
    let status = unsafe {
        GetEventParameter(
            event,
            EVENT_PARAM_DIRECT_OBJECT,
            TYPE_EVENT_HOT_KEY_ID,
            ptr::null_mut(),
            size_of::<EventHotKeyId>(),
            ptr::null_mut(),
            (&raw mut id).cast(),
        )
    };
    if status != NO_ERR || id.signature != SIGNATURE {
        return EVENT_NOT_HANDLED_ERR;
    }
    let Some(&action) = ShortcutAction::ALL.get(id.id as usize) else {
        return EVENT_NOT_HANDLED_ERR;
    };
    // SAFETY: `user_data` is the `PressHandler` owned by the live `Hotkeys`.
    let on_press = unsafe { &*(user_data as *const PressHandler) };
    on_press(action);
    NO_ERR
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ShortcutPreset;

    #[test]
    fn presets_plan_every_action() {
        for preset in [ShortcutPreset::System, ShortcutPreset::Alongside] {
            let (planned, failed) = plan(&preset.shortcuts());
            assert!(failed.is_empty());
            let actions: Vec<_> = planned.iter().map(|(action, _)| *action).collect();
            assert_eq!(actions, ShortcutAction::ALL);
        }
        let (planned, _) = plan(&ShortcutPreset::System.shortcuts());
        let (_, capture_area) = &planned[0];
        assert_eq!(capture_area.key_code, 0x15);
        assert_eq!(capture_area.modifiers.carbon(), 256 | 512);
    }

    #[test]
    fn empty_shortcuts_are_skipped_and_bad_ones_fail() {
        let shortcuts = Shortcuts {
            capture_area: String::new(),
            capture_screen: "Alt+Shift+3".into(),
            record_area: "Command+Banana".into(),
            record_screen: "Control+Shift+5".into(),
            open_last: "F5".into(),
        };
        let (planned, failed) = plan(&shortcuts);
        let actions: Vec<_> = planned.iter().map(|(action, _)| *action).collect();
        assert_eq!(
            actions,
            [ShortcutAction::RecordScreen, ShortcutAction::OpenLast]
        );
        assert_eq!(
            failed,
            [ShortcutAction::CaptureScreen, ShortcutAction::RecordArea]
        );
    }

    #[test]
    fn hot_key_ids_round_trip_to_actions() {
        for action in ShortcutAction::ALL {
            assert_eq!(ShortcutAction::ALL[action_id(action) as usize], action);
        }
        assert_eq!(SIGNATURE, 0x7373_484B);
    }
}
