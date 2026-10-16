//! Port of `apps/desktop/src/shared/settings.ts` plus the load/save half of
//! `SettingsStore.ts`. Field names, defaults and the 2-space JSON layout match
//! the Electron build so either app can read the other's file.
//!
//! Decoding is lenient per field: a missing or invalid value falls back to its
//! default instead of resetting every setting.

use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use color_eyre::eyre::{Result, WrapErr, eyre};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

#[cfg(target_os = "macos")]
pub mod model;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BackgroundId {
    Desktop,
    Cool,
    Nice,
    Morning,
    Bright,
    Love,
    Rain,
    Sky,
    None,
    Custom,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AspectRatio {
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "16:9")]
    R16x9,
    #[serde(rename = "4:3")]
    R4x3,
    #[serde(rename = "3:2")]
    R3x2,
    #[serde(rename = "1:1")]
    R1x1,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AfterCapture {
    #[default]
    Preview,
    Editor,
    Nothing,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    CaptureArea,
    CaptureScreen,
    RecordArea,
    RecordScreen,
    OpenLast,
}

impl ShortcutAction {
    pub const ALL: [Self; 5] = [
        Self::CaptureArea,
        Self::CaptureScreen,
        Self::RecordArea,
        Self::RecordScreen,
        Self::OpenLast,
    ];
}

/// Editor style, each number a percentage in `0..=100`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorStyle {
    #[serde(serialize_with = "serialize_number")]
    pub padding: f64,
    #[serde(serialize_with = "serialize_number")]
    pub inset: f64,
    #[serde(serialize_with = "serialize_number")]
    pub radius: f64,
    #[serde(serialize_with = "serialize_number")]
    pub shadow: f64,
    pub background: BackgroundId,
    pub custom_color: String,
    pub ratio: AspectRatio,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            padding: 36.0,
            inset: 0.0,
            radius: 30.0,
            shadow: 45.0,
            background: BackgroundId::Sky,
            custom_color: "#e8e4df".into(),
            ratio: AspectRatio::Auto,
        }
    }
}

impl EditorStyle {
    fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        let default = Self::default();
        Self {
            padding: percent(object, "padding", default.padding),
            inset: percent(object, "inset", default.inset),
            radius: percent(object, "radius", default.radius),
            shadow: percent(object, "shadow", default.shadow),
            background: field(object, "background", default.background),
            custom_color: field(object, "customColor", default.custom_color),
            ratio: field(object, "ratio", default.ratio),
        }
    }
}

/// Electron accelerator strings, `""` meaning unassigned.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shortcuts {
    pub capture_area: String,
    pub capture_screen: String,
    pub record_area: String,
    pub record_screen: String,
    pub open_last: String,
}

impl Default for Shortcuts {
    fn default() -> Self {
        ShortcutPreset::Alongside.shortcuts()
    }
}

impl Shortcuts {
    pub fn get(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::CaptureArea => &self.capture_area,
            ShortcutAction::CaptureScreen => &self.capture_screen,
            ShortcutAction::RecordArea => &self.record_area,
            ShortcutAction::RecordScreen => &self.record_screen,
            ShortcutAction::OpenLast => &self.open_last,
        }
    }

    pub fn set(&mut self, action: ShortcutAction, accelerator: impl Into<String>) {
        let slot = match action {
            ShortcutAction::CaptureArea => &mut self.capture_area,
            ShortcutAction::CaptureScreen => &mut self.capture_screen,
            ShortcutAction::RecordArea => &mut self.record_area,
            ShortcutAction::RecordScreen => &mut self.record_screen,
            ShortcutAction::OpenLast => &mut self.open_last,
        };
        *slot = accelerator.into();
    }

    fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        let default = Self::default();
        Self {
            capture_area: field(object, "captureArea", default.capture_area),
            capture_screen: field(object, "captureScreen", default.capture_screen),
            record_area: field(object, "recordArea", default.record_area),
            record_screen: field(object, "recordScreen", default.record_screen),
            open_last: field(object, "openLast", default.open_last),
        }
    }
}

/// Shortcut sets offered during onboarding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutPreset {
    /// Takes over the macOS screenshot keys (requires turning the system ones off).
    #[allow(
        dead_code,
        reason = "applied by onboarding and Settings in milestone 4"
    )]
    System,
    /// Lives next to the macOS ones, which keep working.
    Alongside,
}

impl ShortcutPreset {
    pub fn shortcuts(self) -> Shortcuts {
        let [
            capture_screen,
            capture_area,
            record_area,
            record_screen,
            open_last,
        ] = match self {
            Self::System => [
                "Shift+Command+3",
                "Shift+Command+4",
                "Shift+Command+5",
                "Alt+Shift+Command+5",
                "Alt+Shift+Command+4",
            ],
            Self::Alongside => [
                "Control+Shift+3",
                "Control+Shift+4",
                "Control+Shift+5",
                "Control+Alt+Shift+5",
                "Control+Alt+Shift+4",
            ],
        };
        Shortcuts {
            capture_area: capture_area.into(),
            capture_screen: capture_screen.into(),
            record_area: record_area.into(),
            record_screen: record_screen.into(),
            open_last: open_last.into(),
        }
    }
}

/// Since macOS 15, global hotkeys whose only modifiers are ⌥ / ⌥⇧ register
/// fine but never fire. Anything else needs ⌘ or ⌃ (function keys may stand alone).
pub fn is_usable_accelerator(accelerator: &str) -> bool {
    if accelerator.is_empty() {
        return true;
    }
    let parts: Vec<&str> = accelerator.split('+').collect();
    let (key, modifiers) = parts.split_last().expect("split yields at least one part");
    if is_function_key(key) {
        return true;
    }
    modifiers.iter().any(|modifier| {
        matches!(
            *modifier,
            "Command"
                | "Cmd"
                | "CommandOrControl"
                | "CmdOrCtrl"
                | "Control"
                | "Ctrl"
                | "Super"
                | "Meta"
        )
    })
}

/// `/^F\d{1,2}$/`
fn is_function_key(key: &str) -> bool {
    key.strip_prefix('F').is_some_and(|digits| {
        (1..=2).contains(&digits.len()) && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// Replaces shortcuts macOS would silently ignore (e.g. the old ⌥⇧4 defaults).
pub fn repair_shortcuts(shortcuts: &Shortcuts) -> Shortcuts {
    let defaults = Shortcuts::default();
    let mut repaired = shortcuts.clone();
    for action in ShortcutAction::ALL {
        if !is_usable_accelerator(repaired.get(action)) {
            repaired.set(action, defaults.get(action));
        }
    }
    repaired
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub shortcuts: Shortcuts,
    pub editor: EditorStyle,
    pub after_capture: AfterCapture,
    /// Where ⌘S in the editor writes images. `None` = `~/Pictures/ss`.
    pub images_directory: Option<String>,
    /// Where recordings are written. `None` = `~/Movies/ss`.
    pub recordings_directory: Option<String>,
    /// Set once the setup guide has been completed (or skipped).
    pub onboarded: bool,
}

impl Settings {
    /// Lenient decode: anything that is not a valid value for its field
    /// (including a non-object document) falls back to the default.
    pub fn from_value(value: &Value) -> Self {
        let object = value.as_object();
        let default = Self::default();
        Self {
            shortcuts: object
                .and_then(|object| object.get("shortcuts"))
                .map(Shortcuts::from_value)
                .unwrap_or(default.shortcuts),
            editor: object
                .and_then(|object| object.get("editor"))
                .map(EditorStyle::from_value)
                .unwrap_or(default.editor),
            after_capture: field(object, "afterCapture", default.after_capture),
            images_directory: field(object, "imagesDirectory", default.images_directory),
            recordings_directory: field(
                object,
                "recordingsDirectory",
                default.recordings_directory,
            ),
            onboarded: field(object, "onboarded", default.onboarded),
        }
    }

    pub fn from_json(text: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(text)?;
        Ok(Self::from_value(&value))
    }

    /// The exact bytes Electron writes: `JSON.stringify(settings, null, 2)`.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("settings always serialize")
    }

    /// Shallow merge, like `{ ...current, ...patch }`.
    pub fn apply(&mut self, patch: SettingsPatch) {
        let SettingsPatch {
            shortcuts,
            editor,
            after_capture,
            images_directory,
            recordings_directory,
            onboarded,
        } = patch;
        if let Some(shortcuts) = shortcuts {
            self.shortcuts = shortcuts;
        }
        if let Some(editor) = editor {
            self.editor = editor;
        }
        if let Some(after_capture) = after_capture {
            self.after_capture = after_capture;
        }
        if let Some(images_directory) = images_directory {
            self.images_directory = images_directory;
        }
        if let Some(recordings_directory) = recordings_directory {
            self.recordings_directory = recordings_directory;
        }
        if let Some(onboarded) = onboarded {
            self.onboarded = onboarded;
        }
    }
}

impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(Self::from_value(&Value::deserialize(deserializer)?))
    }
}

/// Each `Some` replaces the whole field. For the directories, `Some(None)`
/// resets to the default folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsPatch {
    pub shortcuts: Option<Shortcuts>,
    pub editor: Option<EditorStyle>,
    pub after_capture: Option<AfterCapture>,
    pub images_directory: Option<Option<String>>,
    pub recordings_directory: Option<Option<String>>,
    pub onboarded: Option<bool>,
}

/// Settings read from disk, with ⌥-only shortcuts already repaired.
#[derive(Clone, Debug, PartialEq)]
pub struct Loaded {
    pub settings: Settings,
    /// The file had shortcuts macOS ignores and should be rewritten.
    pub repaired: bool,
}

/// Reads settings without writing anything. A missing, unreadable or
/// malformed file yields the defaults.
pub fn read(path: &Path) -> Loaded {
    let initial = match fs::read_to_string(path) {
        Ok(text) => Settings::from_json(&text).unwrap_or_else(|error| {
            tracing::warn!(path = %path.display(), %error, "using default settings");
            Settings::default()
        }),
        Err(error) if error.kind() == ErrorKind::NotFound => Settings::default(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "using default settings");
            Settings::default()
        }
    };
    let shortcuts = repair_shortcuts(&initial.shortcuts);
    let repaired = shortcuts != initial.shortcuts;
    Loaded {
        settings: Settings {
            shortcuts,
            ..initial
        },
        repaired,
    }
}

/// [`read`], then persists the repaired shortcuts, as `SettingsStore` does on startup.
pub fn load(path: &Path) -> Settings {
    let Loaded { settings, repaired } = read(path);
    if repaired {
        tracing::info!("replaced shortcuts macOS ignores (⌥/⌥⇧ only)");
        if let Err(error) = save(path, &settings) {
            tracing::warn!(%error, "could not persist repaired shortcuts");
        }
    }
    settings
}

/// Atomic write: a sibling temp file renamed over the target.
pub fn save(path: &Path, settings: &Settings) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| eyre!("settings path has no parent"))?;
    fs::create_dir_all(parent)
        .wrap_err_with(|| format!("could not create {}", parent.display()))?;
    let temporary = temporary_path(path);
    fs::write(&temporary, settings.to_json())
        .wrap_err_with(|| format!("could not write {}", temporary.display()))?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).wrap_err_with(|| format!("could not replace {}", path.display()));
    }
    Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    path.with_extension(format!("json.{}.{sequence}.tmp", std::process::id()))
}

fn field<T: DeserializeOwned>(object: Option<&Map<String, Value>>, key: &str, fallback: T) -> T {
    object
        .and_then(|object| object.get(key))
        .and_then(|value| T::deserialize(value).ok())
        .unwrap_or(fallback)
}

fn percent(object: Option<&Map<String, Value>>, key: &str, fallback: f64) -> f64 {
    let value = field(object, key, fallback);
    if (0.0..=100.0).contains(&value) {
        value
    } else {
        fallback
    }
}

/// JavaScript prints whole numbers without a fraction (`42`, not `42.0`).
pub(crate) fn serialize_number<S: Serializer>(
    value: &f64,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        serializer.serialize_i64(*value as i64)
    } else {
        serializer.serialize_f64(*value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const REAL_SETTINGS: &str = include_str!("../tests/fixtures/settings-2026-10-12.json");

    #[test]
    fn defaults_match_the_typescript_schema() {
        let expected = json!({
            "shortcuts": {
                "captureArea": "Control+Shift+4",
                "captureScreen": "Control+Shift+3",
                "recordArea": "Control+Shift+5",
                "recordScreen": "Control+Alt+Shift+5",
                "openLast": "Control+Alt+Shift+4"
            },
            "editor": {
                "padding": 36,
                "inset": 0,
                "radius": 30,
                "shadow": 45,
                "background": "sky",
                "customColor": "#e8e4df",
                "ratio": "auto"
            },
            "afterCapture": "preview",
            "imagesDirectory": null,
            "recordingsDirectory": null,
            "onboarded": false
        });
        assert_eq!(serde_json::to_value(Settings::default()).unwrap(), expected);
        // `decodeSettings({})` is how TypeScript builds its defaults.
        assert_eq!(Settings::from_json("{}").unwrap(), Settings::default());
    }

    #[test]
    fn presets_match_the_typescript_presets() {
        let system = ShortcutPreset::System.shortcuts();
        assert_eq!(system.capture_screen, "Shift+Command+3");
        assert_eq!(system.capture_area, "Shift+Command+4");
        assert_eq!(system.record_area, "Shift+Command+5");
        assert_eq!(system.record_screen, "Alt+Shift+Command+5");
        assert_eq!(system.open_last, "Alt+Shift+Command+4");
        assert_eq!(Shortcuts::default(), ShortcutPreset::Alongside.shortcuts());
        for preset in [ShortcutPreset::System, ShortcutPreset::Alongside] {
            let shortcuts = preset.shortcuts();
            assert_eq!(repair_shortcuts(&shortcuts), shortcuts);
        }
    }

    #[test]
    fn round_trips_the_real_settings_file() {
        let settings = Settings::from_json(REAL_SETTINGS).unwrap();
        assert_eq!(settings.shortcuts, ShortcutPreset::System.shortcuts());
        assert_eq!(
            settings.editor,
            EditorStyle {
                padding: 42.0,
                inset: 0.0,
                radius: 100.0,
                shadow: 66.0,
                background: BackgroundId::Love,
                custom_color: "#da8216".into(),
                ratio: AspectRatio::R1x1,
            }
        );
        assert_eq!(settings.after_capture, AfterCapture::Preview);
        assert_eq!(settings.images_directory, None);
        assert_eq!(settings.recordings_directory, None);
        assert!(settings.onboarded);

        let original: Value = serde_json::from_str(REAL_SETTINGS).unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&settings.to_json()).unwrap(),
            original
        );
        assert_eq!(
            settings.to_json(),
            r##"{
  "shortcuts": {
    "captureArea": "Shift+Command+4",
    "captureScreen": "Shift+Command+3",
    "recordArea": "Shift+Command+5",
    "recordScreen": "Alt+Shift+Command+5",
    "openLast": "Alt+Shift+Command+4"
  },
  "editor": {
    "padding": 42,
    "inset": 0,
    "radius": 100,
    "shadow": 66,
    "background": "love",
    "customColor": "#da8216",
    "ratio": "1:1"
  },
  "afterCapture": "preview",
  "imagesDirectory": null,
  "recordingsDirectory": null,
  "onboarded": true
}"##
        );
    }

    #[test]
    fn fractional_percentages_survive() {
        let settings = Settings::from_json(r#"{"editor":{"padding":12.5}}"#).unwrap();
        assert_eq!(settings.editor.padding, 12.5);
        assert!(settings.to_json().contains(r#""padding": 12.5,"#));
    }

    #[test]
    fn every_enum_value_round_trips() {
        for (text, ratio) in [
            ("auto", AspectRatio::Auto),
            ("16:9", AspectRatio::R16x9),
            ("4:3", AspectRatio::R4x3),
            ("3:2", AspectRatio::R3x2),
            ("1:1", AspectRatio::R1x1),
        ] {
            assert_eq!(serde_json::to_value(ratio).unwrap(), json!(text));
            assert_eq!(
                serde_json::from_value::<AspectRatio>(json!(text)).unwrap(),
                ratio
            );
        }
        let backgrounds = [
            "desktop", "cool", "nice", "morning", "bright", "love", "rain", "sky", "none", "custom",
        ];
        for text in backgrounds {
            let id: BackgroundId = serde_json::from_value(json!(text)).unwrap();
            assert_eq!(serde_json::to_value(id).unwrap(), json!(text));
        }
        for (text, after) in [
            ("preview", AfterCapture::Preview),
            ("editor", AfterCapture::Editor),
            ("nothing", AfterCapture::Nothing),
        ] {
            assert_eq!(serde_json::to_value(after).unwrap(), json!(text));
        }
    }

    #[test]
    fn invalid_fields_fall_back_individually() {
        let settings = Settings::from_json(
            r##"{
                "shortcuts": { "captureArea": 4, "openLast": "Command+L" },
                "editor": {
                    "padding": 150, "inset": -1, "radius": "big", "shadow": 12,
                    "background": "plaid", "customColor": 7, "ratio": "2:1"
                },
                "afterCapture": "explode",
                "imagesDirectory": 5,
                "recordingsDirectory": "/tmp/rec",
                "onboarded": "yes",
                "unknown": true
            }"##,
        )
        .unwrap();
        let defaults = Settings::default();
        assert_eq!(
            settings.shortcuts.capture_area,
            defaults.shortcuts.capture_area
        );
        assert_eq!(settings.shortcuts.open_last, "Command+L");
        assert_eq!(settings.editor.padding, 36.0);
        assert_eq!(settings.editor.inset, 0.0);
        assert_eq!(settings.editor.radius, 30.0);
        assert_eq!(settings.editor.shadow, 12.0);
        assert_eq!(settings.editor.background, BackgroundId::Sky);
        assert_eq!(settings.editor.custom_color, "#e8e4df");
        assert_eq!(settings.editor.ratio, AspectRatio::Auto);
        assert_eq!(settings.after_capture, AfterCapture::Preview);
        assert_eq!(settings.images_directory, None);
        assert_eq!(settings.recordings_directory.as_deref(), Some("/tmp/rec"));
        assert!(!settings.onboarded);
        assert!(!settings.to_json().contains("unknown"));
    }

    #[test]
    fn wrong_shapes_fall_back_to_defaults() {
        assert_eq!(Settings::from_json("[]").unwrap(), Settings::default());
        assert_eq!(Settings::from_json("null").unwrap(), Settings::default());
        let settings =
            Settings::from_json(r#"{"shortcuts":5,"editor":[],"onboarded":true}"#).unwrap();
        assert_eq!(settings.shortcuts, Shortcuts::default());
        assert_eq!(settings.editor, EditorStyle::default());
        assert!(settings.onboarded);
        assert!(Settings::from_json("{not json").is_err());
    }

    #[test]
    fn boundary_percentages_are_valid() {
        let settings =
            Settings::from_json(r#"{"editor":{"padding":0,"inset":100,"radius":100.0001}}"#)
                .unwrap();
        assert_eq!(settings.editor.padding, 0.0);
        assert_eq!(settings.editor.inset, 100.0);
        assert_eq!(settings.editor.radius, 30.0);
    }

    #[test]
    fn usable_accelerators_follow_the_macos_15_rule() {
        let usable = [
            "",
            "Command+Shift+4",
            "Shift+Command+3",
            "Control+Shift+4",
            "Control+Alt+Shift+5",
            "Alt+Shift+Command+4",
            "Cmd+K",
            "CommandOrControl+K",
            "CmdOrCtrl+K",
            "Ctrl+K",
            "Super+K",
            "Meta+K",
            "F5",
            "F12",
            "Shift+F12",
            "Alt+F1",
        ];
        for accelerator in usable {
            assert!(
                is_usable_accelerator(accelerator),
                "{accelerator:?} should be usable"
            );
        }
        let unusable = [
            "Alt+Shift+4",
            "Alt+4",
            "Option+4",
            "Shift+4",
            "4",
            "A",
            "Command",
            "F123",
            "Fx",
            "F",
            "Shift+F",
            "command+K",
        ];
        for accelerator in unusable {
            assert!(
                !is_usable_accelerator(accelerator),
                "{accelerator:?} should be unusable"
            );
        }
    }

    #[test]
    fn repair_replaces_option_only_accelerators() {
        let shortcuts = Shortcuts {
            capture_area: "Alt+Shift+4".into(),
            capture_screen: "Alt+Shift+3".into(),
            record_area: "Command+Shift+5".into(),
            record_screen: String::new(),
            open_last: "Shift+L".into(),
        };
        let repaired = repair_shortcuts(&shortcuts);
        assert_eq!(repaired.capture_area, "Control+Shift+4");
        assert_eq!(repaired.capture_screen, "Control+Shift+3");
        assert_eq!(repaired.record_area, "Command+Shift+5");
        assert_eq!(repaired.record_screen, "");
        assert_eq!(repaired.open_last, "Control+Alt+Shift+4");
    }

    #[test]
    fn load_persists_repairs_but_read_never_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        let original = r#"{"shortcuts":{"captureArea":"Alt+Shift+4"},"onboarded":true}"#;
        fs::write(&path, original).unwrap();

        let loaded = read(&path);
        assert!(loaded.repaired);
        assert_eq!(loaded.settings.shortcuts.capture_area, "Control+Shift+4");
        assert_eq!(fs::read_to_string(&path).unwrap(), original);

        let settings = load(&path);
        assert_eq!(settings, loaded.settings);
        assert_eq!(fs::read_to_string(&path).unwrap(), settings.to_json());
        assert!(!read(&path).repaired);
    }

    #[test]
    fn missing_or_malformed_files_read_as_defaults() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("settings.json");
        assert_eq!(read(&path).settings, Settings::default());
        assert_eq!(load(&path), Settings::default());
        assert!(!path.exists(), "loading defaults must not create the file");
        fs::write(&path, "{oops").unwrap();
        assert_eq!(read(&path).settings, Settings::default());
    }

    #[test]
    fn save_is_atomic_and_pretty() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("nested").join("settings.json");
        let mut settings = Settings::default();
        settings.apply(SettingsPatch {
            after_capture: Some(AfterCapture::Nothing),
            images_directory: Some(Some("/tmp/images".into())),
            ..SettingsPatch::default()
        });
        save(&path, &settings).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(text, settings.to_json());
        assert!(text.starts_with("{\n  \"shortcuts\": {\n    \"captureArea\""));
        assert_eq!(read(&path).settings, settings);
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name != "settings.json")
            .collect();
        assert!(
            leftovers.is_empty(),
            "temporary files left behind: {leftovers:?}"
        );
    }

    #[test]
    fn patches_merge_shallowly() {
        let mut settings = Settings {
            images_directory: Some("/tmp/a".into()),
            ..Settings::default()
        };
        let mut shortcuts = Shortcuts::default();
        shortcuts.set(ShortcutAction::OpenLast, "");
        settings.apply(SettingsPatch {
            shortcuts: Some(shortcuts.clone()),
            images_directory: Some(None),
            onboarded: Some(true),
            ..SettingsPatch::default()
        });
        assert_eq!(settings.shortcuts, shortcuts);
        assert_eq!(settings.images_directory, None);
        assert!(settings.onboarded);
        assert_eq!(settings.editor, EditorStyle::default());
        assert_eq!(settings.after_capture, AfterCapture::Preview);
    }
}
