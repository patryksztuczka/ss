//! Permissions and OS integration (port of `System.ts` and the `ss-helper`
//! subcommands): Screen Recording access, the macOS screenshot shortcuts,
//! launch at login, and the permission host shown in development.
//! Everything here blocks; call it from the background executor.

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use color_eyre::eyre::{Result, WrapErr, eyre};
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_core_foundation::{
    CFPreferencesAppSynchronize, CFPreferencesCopyAppValue, CFPropertyList, CFRetained, CFString,
};
use objc2_foundation::{
    NSBundle, NSData, NSJSONReadingOptions, NSJSONSerialization, NSJSONWritingOptions,
};
use serde_json::{Map, Value, json};

use crate::login_item;

pub const BUNDLE_ID: &str = "dev.sztuczka.ss";
pub const SCREEN_RECORDING_SETTINGS: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
const ACTIVATE_SETTINGS: &str =
    "/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings";
const HOTKEYS_DOMAIN: &str = "com.apple.symbolichotkeys";
const HOTKEYS_KEY: &str = "AppleSymbolicHotKeys";
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Symbolic hotkey id → default (character, key code, modifier flags), as in `helper.swift`.
const SCREENSHOT_HOTKEYS: [(u32, [i64; 3]); 5] = [
    (28, [51, 20, 1_179_648]),  // ⇧⌘3 save screen
    (29, [51, 20, 1_441_792]),  // ⌃⇧⌘3 copy screen
    (30, [52, 21, 1_179_648]),  // ⇧⌘4 save area
    (31, [52, 21, 1_441_792]),  // ⌃⇧⌘4 copy area
    (184, [53, 23, 1_179_648]), // ⇧⌘5 screenshot & recording options
];

/// Development host apps macOS attributes ss's permission requests to.
const TERMINALS: [(&str, &str); 8] = [
    ("com.apple.Terminal", "Terminal"),
    ("com.googlecode.iterm2", "iTerm"),
    ("com.mitchellh.ghostty", "Ghostty"),
    ("com.microsoft.VSCode", "Visual Studio Code"),
    ("com.todesktop.230313mzl4w4u92", "Cursor"),
    ("dev.warp.Warp-Stable", "Warp"),
    ("dev.zed.Zed", "Zed"),
    ("com.t3tools.t3code", "T3 Code"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenAccess {
    Granted,
    Denied,
}

impl ScreenAccess {
    fn parse(text: &str) -> Self {
        if text.trim() == "granted" {
            Self::Granted
        } else {
            Self::Denied
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Denied => "denied",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LaunchAtLogin {
    /// Only the installed, bundled app can register a login item.
    pub available: bool,
    pub enabled: bool,
    /// macOS wants the user to approve it in System Settings → Login Items.
    pub requires_approval: bool,
}

/// Everything the setup guide and Settings show about permissions and integration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupState {
    pub screen_access: ScreenAccess,
    /// Whether macOS still owns ⇧⌘3 / ⇧⌘4 / ⇧⌘5.
    pub system_screenshot_shortcuts: bool,
    pub launch_at_login: LaunchAtLogin,
    /// In development the terminal running ss is the app macOS asks about.
    pub permission_host: Option<String>,
}

pub fn setup_state() -> SetupState {
    let bundled = is_bundled();
    SetupState {
        screen_access: screen_access(),
        system_screenshot_shortcuts: screenshot_shortcuts_enabled().unwrap_or_else(|error| {
            tracing::warn!(%error, "could not read the macOS screenshot shortcuts");
            true
        }),
        launch_at_login: launch_at_login(bundled),
        permission_host: (!bundled).then(|| {
            permission_host_from(
                std::env::var("__CFBundleIdentifier").ok().as_deref(),
                std::env::var("TERM_PROGRAM").ok().as_deref(),
            )
        }),
    }
}

fn launch_at_login(bundled: bool) -> LaunchAtLogin {
    if !bundled {
        return LaunchAtLogin::default();
    }
    match login_item::status() {
        Ok(status) => LaunchAtLogin {
            available: true,
            enabled: status == login_item::Status::Enabled,
            requires_approval: status == login_item::Status::RequiresApproval,
        },
        Err(error) => {
            tracing::warn!(%error, "could not read the login item status");
            LaunchAtLogin {
                available: true,
                ..LaunchAtLogin::default()
            }
        }
    }
}

/// `app.isPackaged`: running from the installed `dev.sztuczka.ss` bundle.
pub fn is_bundled() -> bool {
    NSBundle::mainBundle()
        .bundleIdentifier()
        .is_some_and(|identifier| identifier.to_string() == BUNDLE_ID)
}

fn permission_host_from(bundle_id: Option<&str>, term_program: Option<&str>) -> String {
    if let Some(bundle_id) = bundle_id {
        return TERMINALS
            .iter()
            .find(|(id, _)| *id == bundle_id)
            .map_or(bundle_id, |(_, name)| name)
            .to_string();
    }
    term_program.unwrap_or("your terminal app").to_string()
}

/// Screen Recording access as a fresh process sees it: macOS caches the
/// answer per process, so a child notices a newly granted permission.
pub fn screen_access() -> ScreenAccess {
    run_probe(false, PROBE_TIMEOUT).unwrap_or_else(|| {
        tracing::debug!("screen access probe failed; checking in-process");
        probe(false)
    })
}

/// Shows the macOS prompt (first time only). A timeout counts as denied;
/// the caller then opens System Settings.
pub fn request_screen_access() -> ScreenAccess {
    run_probe(true, REQUEST_TIMEOUT).unwrap_or(ScreenAccess::Denied)
}

/// The in-process check behind `ss internal screen-access [--request]`.
pub fn probe(request: bool) -> ScreenAccess {
    let granted = if request {
        objc2_core_graphics::CGRequestScreenCaptureAccess()
    } else {
        objc2_core_graphics::CGPreflightScreenCaptureAccess()
    };
    if granted {
        ScreenAccess::Granted
    } else {
        ScreenAccess::Denied
    }
}

fn run_probe(request: bool, timeout: Duration) -> Option<ScreenAccess> {
    let executable = std::env::current_exe().ok()?;
    let mut command = Command::new(executable);
    command.args(["internal", "screen-access"]);
    if request {
        command.arg("--request");
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut output = String::new();
    child.stdout.take()?.read_to_string(&mut output).ok()?;
    Some(ScreenAccess::parse(&output))
}

/// Whether any of the macOS screenshot shortcuts is still on.
pub fn screenshot_shortcuts_enabled() -> Result<bool> {
    Ok(shortcuts_enabled_in(&read_hotkeys()?))
}

/// Switches the macOS screenshot shortcuts and reloads them without a
/// logout. Returns the state read back afterwards.
pub fn set_screenshot_shortcuts(enabled: bool) -> Result<bool> {
    let updated = with_screenshot_shortcuts(read_hotkeys()?, enabled);
    write_hotkeys(&updated)?;
    let status = Command::new(ACTIVATE_SETTINGS)
        .arg("-u")
        .status()
        .wrap_err("could not run activateSettings")?;
    if !status.success() {
        tracing::warn!(%status, "activateSettings failed");
    }
    tracing::info!(on = enabled, "macOS screenshot shortcuts");
    screenshot_shortcuts_enabled()
}

/// A missing entry means the default, which is enabled.
fn shortcuts_enabled_in(hotkeys: &Value) -> bool {
    SCREENSHOT_HOTKEYS.iter().any(|(id, _)| {
        hotkeys
            .get(id.to_string())
            .and_then(|entry| entry.get("enabled"))
            .and_then(Value::as_bool)
            .unwrap_or(true)
    })
}

/// Sets `enabled` on every screenshot hotkey, filling in the default key
/// when an entry has never been customised. Other hotkeys are untouched.
fn with_screenshot_shortcuts(hotkeys: Value, enabled: bool) -> Value {
    let mut hotkeys = match hotkeys {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    for (id, parameters) in SCREENSHOT_HOTKEYS {
        let entry = hotkeys
            .entry(id.to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        if !entry.is_object() {
            *entry = Value::Object(Map::new());
        }
        let entry = entry.as_object_mut().expect("entry is an object");
        entry.insert("enabled".into(), Value::Bool(enabled));
        entry
            .entry("value")
            .or_insert_with(|| json!({ "parameters": parameters, "type": "standard" }));
    }
    Value::Object(hotkeys)
}

fn read_hotkeys() -> Result<Value> {
    let key = CFString::from_str(HOTKEYS_KEY);
    let domain = CFString::from_str(HOTKEYS_DOMAIN);
    let Some(list) = CFPreferencesCopyAppValue(&key, &domain) else {
        return Ok(Value::Object(Map::new()));
    };
    // SAFETY: CFPropertyList objects are toll-free bridged to Foundation.
    let object: Retained<AnyObject> =
        unsafe { Retained::retain(CFRetained::as_ptr(&list).as_ptr().cast()) }
            .ok_or_else(|| eyre!("unexpected null preference value"))?;
    object_to_value(&object)
}

fn write_hotkeys(hotkeys: &Value) -> Result<()> {
    let object = value_to_object(hotkeys)?;
    let key = CFString::from_str(HOTKEYS_KEY);
    let domain = CFString::from_str(HOTKEYS_DOMAIN);
    // SAFETY: Foundation property-list objects are toll-free bridged to CF.
    let list = unsafe { &*(Retained::as_ptr(&object) as *const CFPropertyList) };
    unsafe { objc2_core_foundation::CFPreferencesSetAppValue(&key, Some(list), &domain) };
    if CFPreferencesAppSynchronize(&domain) {
        Ok(())
    } else {
        Err(eyre!("cannot write keyboard preferences"))
    }
}

/// Property list → JSON via `NSJSONSerialization`; the symbolic hotkeys hold
/// only dictionaries, arrays, numbers, booleans and strings.
fn object_to_value(object: &AnyObject) -> Result<Value> {
    // SAFETY: `object` is a property list, which is valid JSON input here.
    let data = unsafe {
        NSJSONSerialization::dataWithJSONObject_options_error(object, NSJSONWritingOptions::empty())
    }
    .map_err(|error| eyre!("cannot read keyboard preferences: {error}"))?;
    Ok(serde_json::from_slice(&data.to_vec())?)
}

fn value_to_object(value: &Value) -> Result<Retained<AnyObject>> {
    let data = NSData::with_bytes(&serde_json::to_vec(value)?);
    NSJSONSerialization::JSONObjectWithData_options_error(&data, NSJSONReadingOptions::empty())
        .map_err(|error| eyre!("cannot encode keyboard preferences: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_host_names_known_terminals() {
        assert_eq!(
            permission_host_from(Some("com.apple.Terminal"), None),
            "Terminal"
        );
        assert_eq!(
            permission_host_from(Some("com.t3tools.t3code"), Some("x")),
            "T3 Code"
        );
        assert_eq!(
            permission_host_from(Some("org.example.Shell"), None),
            "org.example.Shell"
        );
        assert_eq!(permission_host_from(None, Some("tmux")), "tmux");
        assert_eq!(permission_host_from(None, None), "your terminal app");
    }

    #[test]
    fn missing_entries_count_as_enabled() {
        assert!(shortcuts_enabled_in(&json!({})));
        let all_off: Map<String, Value> = SCREENSHOT_HOTKEYS
            .iter()
            .map(|(id, _)| (id.to_string(), json!({ "enabled": false })))
            .collect();
        assert!(!shortcuts_enabled_in(&Value::Object(all_off.clone())));
        let mut one_default = all_off;
        one_default.remove("184");
        assert!(shortcuts_enabled_in(&Value::Object(one_default)));
    }

    #[test]
    fn turning_shortcuts_off_fills_in_defaults_and_keeps_other_hotkeys() {
        let before = json!({
            "28": { "enabled": true, "value": { "parameters": [51, 20, 1_179_648], "type": "standard" } },
            "64": { "enabled": false, "value": { "parameters": [32, 49, 1_048_576], "type": "standard" } }
        });
        let after = with_screenshot_shortcuts(before, false);
        assert!(!shortcuts_enabled_in(&after));
        assert_eq!(
            after["64"],
            json!({ "enabled": false, "value": { "parameters": [32, 49, 1_048_576], "type": "standard" } })
        );
        assert_eq!(
            after["28"]["value"]["parameters"],
            json!([51, 20, 1_179_648])
        );
        assert_eq!(
            after["31"],
            json!({ "enabled": false, "value": { "parameters": [52, 21, 1_441_792], "type": "standard" } })
        );
        assert_eq!(
            after["184"]["value"]["parameters"],
            json!([53, 23, 1_179_648])
        );

        let restored = with_screenshot_shortcuts(after, true);
        assert!(shortcuts_enabled_in(&restored));
        for (id, _) in SCREENSHOT_HOTKEYS {
            assert_eq!(restored[id.to_string()]["enabled"], json!(true));
        }
    }

    #[test]
    fn keeps_customised_screenshot_keys() {
        let custom = json!({ "30": { "enabled": true, "value": { "parameters": [97, 0, 262_144], "type": "standard" } } });
        let after = with_screenshot_shortcuts(custom, false);
        assert_eq!(after["30"]["value"]["parameters"], json!([97, 0, 262_144]));
        assert_eq!(after["30"]["enabled"], json!(false));
    }

    #[test]
    fn non_object_preferences_are_rebuilt() {
        let after = with_screenshot_shortcuts(json!("garbage"), true);
        assert_eq!(after.as_object().unwrap().len(), SCREENSHOT_HOTKEYS.len());
    }

    #[test]
    fn foundation_bridge_round_trips_property_lists_in_memory() {
        let value = json!({
            "28": { "enabled": false, "value": { "parameters": [51, 20, 1_179_648], "type": "standard" } }
        });
        let object = value_to_object(&value).unwrap();
        assert_eq!(object_to_value(&object).unwrap(), value);
    }

    #[test]
    fn screen_access_output_parses() {
        assert_eq!(ScreenAccess::parse("granted\n"), ScreenAccess::Granted);
        assert_eq!(ScreenAccess::parse("denied"), ScreenAccess::Denied);
        assert_eq!(ScreenAccess::parse(""), ScreenAccess::Denied);
    }
}
