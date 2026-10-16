//! Electron accelerator strings: display formatting (port of
//! `renderer/src/lib/accelerator.ts`), parsing into Carbon key codes for
//! global hotkeys, NSMenu key equivalents, and keystroke capture.
//!
//! Key codes are US ANSI `kVK_*` virtual keys, as Electron uses on macOS.

use std::fmt;

/// macOS menu order: ⌃⌥⇧⌘.
const MODIFIER_ORDER: [&str; 6] = [
    "Control",
    "Alt",
    "Option",
    "Shift",
    "Command",
    "CommandOrControl",
];

fn symbol(part: &str) -> Option<&'static str> {
    Some(match part {
        "Command" | "CommandOrControl" => "⌘",
        "Control" => "⌃",
        "Alt" | "Option" => "⌥",
        "Shift" => "⇧",
        "Space" => "Space",
        "Return" | "Enter" => "↩",
        "Backspace" => "⌫",
        "Delete" => "⌦",
        "Escape" => "⎋",
        "Up" => "↑",
        "Down" => "↓",
        "Left" => "←",
        "Right" => "→",
        "Plus" => "+",
        _ => return None,
    })
}

/// `"Alt+Shift+4"` → `["⌥", "⇧", "4"]`.
pub fn accelerator_parts(accelerator: &str) -> Vec<String> {
    if accelerator.is_empty() {
        return Vec::new();
    }
    let mut parts: Vec<&str> = accelerator.split('+').collect();
    let key = parts.pop().unwrap_or_default();
    // Unknown modifiers sort first, like `indexOf` returning -1.
    parts.sort_by_key(|modifier| {
        MODIFIER_ORDER
            .iter()
            .position(|known| known == modifier)
            .map_or(-1, |index| index as isize)
    });
    parts
        .into_iter()
        .chain(std::iter::once(key))
        .map(|part| symbol(part).unwrap_or(part).to_string())
        .collect()
}

/// `"Alt+Shift+4"` → `"⌥⇧4"`, `""` → `"—"`.
pub fn format_accelerator(accelerator: &str) -> String {
    if accelerator.is_empty() {
        "—".into()
    } else {
        accelerator_parts(accelerator).concat()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub command: bool,
    pub control: bool,
    pub option: bool,
    pub shift: bool,
}

impl Modifiers {
    // Carbon `Events.h` modifier bits.
    const CARBON_COMMAND: u32 = 1 << 8;
    const CARBON_SHIFT: u32 = 1 << 9;
    const CARBON_OPTION: u32 = 1 << 11;
    const CARBON_CONTROL: u32 = 1 << 12;

    pub fn carbon(self) -> u32 {
        [
            (self.command, Self::CARBON_COMMAND),
            (self.shift, Self::CARBON_SHIFT),
            (self.option, Self::CARBON_OPTION),
            (self.control, Self::CARBON_CONTROL),
        ]
        .into_iter()
        .filter(|(held, _)| *held)
        .fold(0, |mask, (_, bit)| mask | bit)
    }
}

/// A parsed accelerator: what to register with Carbon and show in menus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Accelerator {
    pub modifiers: Modifiers,
    /// Carbon virtual key code (`kVK_*`).
    pub key_code: u16,
    /// The `NSMenuItem.keyEquivalent` string for this key.
    pub key_equivalent: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    UnknownModifier(String),
    UnknownKey(String),
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("empty accelerator"),
            Self::UnknownModifier(modifier) => write!(formatter, "unknown modifier {modifier:?}"),
            Self::UnknownKey(key) => write!(formatter, "unknown key {key:?}"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parses an Electron accelerator such as `"Shift+Command+4"` or `"Command+,"`.
/// Names are case-insensitive, as in Electron.
pub fn parse(accelerator: &str) -> Result<Accelerator, ParseError> {
    if accelerator.is_empty() {
        return Err(ParseError::Empty);
    }
    let mut parts: Vec<&str> = accelerator.split('+').collect();
    let mut key = parts.pop().unwrap_or_default();
    // "Command++" ends with an empty modifier slot before the literal plus.
    if key.is_empty() && parts.last() == Some(&"") {
        parts.pop();
        key = "+";
    }
    let mut modifiers = Modifiers::default();
    for modifier in parts {
        match modifier.to_ascii_lowercase().as_str() {
            "command" | "cmd" | "commandorcontrol" | "cmdorctrl" | "super" | "meta" => {
                modifiers.command = true;
            }
            "control" | "ctrl" => modifiers.control = true,
            "alt" | "option" => modifiers.option = true,
            "shift" => modifiers.shift = true,
            _ => return Err(ParseError::UnknownModifier(modifier.into())),
        }
    }
    let (key_code, key_equivalent) =
        key_spec(key).ok_or_else(|| ParseError::UnknownKey(key.into()))?;
    Ok(Accelerator {
        modifiers,
        key_code,
        key_equivalent,
    })
}

/// The key code and menu key equivalent for an accelerator key name.
fn key_spec(key: &str) -> Option<(u16, String)> {
    let lower = key.to_ascii_lowercase();
    if let [byte] = lower.as_bytes()
        && let Some(code) = character_key_code(*byte)
    {
        return Some((code, lower));
    }
    if let Some(number) = lower
        .strip_prefix('f')
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse::<usize>().ok())
    {
        let code = *FUNCTION_KEY_CODES.get(number.checked_sub(1)?)?;
        // NSF1FunctionKey and up are consecutive.
        let character = char::from_u32(0xF704 + number as u32 - 1)?;
        return Some((code, character.to_string()));
    }
    let (code, character) = match lower.as_str() {
        "plus" => (0x18, '+'),
        "space" => (0x31, ' '),
        "tab" => (0x30, '\t'),
        "return" | "enter" => (0x24, '\r'),
        "backspace" => (0x33, '\u{8}'),
        "delete" => (0x75, '\u{F728}'),
        "escape" | "esc" => (0x35, '\u{1b}'),
        "up" => (0x7E, '\u{F700}'),
        "down" => (0x7D, '\u{F701}'),
        "left" => (0x7B, '\u{F702}'),
        "right" => (0x7C, '\u{F703}'),
        "home" => (0x73, '\u{F729}'),
        "end" => (0x77, '\u{F72B}'),
        "pageup" => (0x74, '\u{F72C}'),
        "pagedown" => (0x79, '\u{F72D}'),
        _ => return None,
    };
    Some((code, character.to_string()))
}

/// `kVK_ANSI_*` for single printable characters (lowercase letters).
fn character_key_code(character: u8) -> Option<u16> {
    Some(match character {
        b'a' => 0x00,
        b's' => 0x01,
        b'd' => 0x02,
        b'f' => 0x03,
        b'h' => 0x04,
        b'g' => 0x05,
        b'z' => 0x06,
        b'x' => 0x07,
        b'c' => 0x08,
        b'v' => 0x09,
        b'b' => 0x0B,
        b'q' => 0x0C,
        b'w' => 0x0D,
        b'e' => 0x0E,
        b'r' => 0x0F,
        b'y' => 0x10,
        b't' => 0x11,
        b'1' => 0x12,
        b'2' => 0x13,
        b'3' => 0x14,
        b'4' => 0x15,
        b'6' => 0x16,
        b'5' => 0x17,
        b'=' | b'+' => 0x18,
        b'9' => 0x19,
        b'7' => 0x1A,
        b'-' => 0x1B,
        b'8' => 0x1C,
        b'0' => 0x1D,
        b']' => 0x1E,
        b'o' => 0x1F,
        b'u' => 0x20,
        b'[' => 0x21,
        b'i' => 0x22,
        b'p' => 0x23,
        b'l' => 0x25,
        b'j' => 0x26,
        b'\'' => 0x27,
        b'k' => 0x28,
        b';' => 0x29,
        b'\\' => 0x2A,
        b',' => 0x2B,
        b'/' => 0x2C,
        b'n' => 0x2D,
        b'm' => 0x2E,
        b'.' => 0x2F,
        b'`' => 0x32,
        _ => return None,
    })
}

/// `kVK_F1` … `kVK_F20`.
const FUNCTION_KEY_CODES: [u16; 20] = [
    0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F, 0x69, 0x6B, 0x71, 0x6A,
    0x40, 0x4F, 0x50, 0x5A,
];

/// GPUI reports Shift+digit/punctuation as the shifted character with
/// `shift` cleared. Map it back to the physical US key, like `event.code`.
#[allow(dead_code, reason = "used by the Settings shortcut recorder (M4)")]
fn unshifted(key: &str) -> Option<&'static str> {
    Some(match key {
        "!" => "1",
        "@" => "2",
        "#" => "3",
        "$" => "4",
        "%" => "5",
        "^" => "6",
        "&" => "7",
        "*" => "8",
        "(" => "9",
        ")" => "0",
        "_" => "-",
        "+" => "=",
        "{" => "[",
        "}" => "]",
        "|" => "\\",
        ":" => ";",
        "\"" => "'",
        "<" => ",",
        ">" => ".",
        "?" => "/",
        "~" => "`",
        _ => return None,
    })
}

/// Port of `acceleratorFromEvent`: builds an accelerator from a key press.
/// Returns `None` while only modifiers are held, and for bare keys other
/// than function keys (they would hijack typing everywhere).
#[cfg(target_os = "macos")]
#[allow(dead_code, reason = "used by the Settings shortcut recorder (M4)")]
pub fn accelerator_from_keystroke(keystroke: &gpui::Keystroke) -> Option<String> {
    let mut shift = keystroke.modifiers.shift;
    let raw = keystroke.key.as_str();
    let raw = match unshifted(raw) {
        Some(physical) => {
            shift = true;
            physical
        }
        None => raw,
    };
    let key = match raw {
        "space" => "Space".to_string(),
        "enter" => "Return".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Delete".to_string(),
        "tab" => "Tab".to_string(),
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        "-" | "=" | "[" | "]" | "\\" | ";" | "'" | "," | "." | "/" | "`" => raw.to_string(),
        _ if raw.len() == 1 && raw.as_bytes()[0].is_ascii_alphanumeric() => {
            raw.to_ascii_uppercase()
        }
        _ if is_function_key_name(raw) => raw.to_ascii_uppercase(),
        _ => return None,
    };
    let modifiers = &keystroke.modifiers;
    let held: Vec<&str> = [
        (modifiers.control, "Control"),
        (modifiers.alt, "Alt"),
        (shift, "Shift"),
        (modifiers.platform, "Command"),
    ]
    .into_iter()
    .filter_map(|(down, name)| down.then_some(name))
    .collect();
    if held.is_empty() && !key.starts_with('F') {
        return None;
    }
    Some(
        held.into_iter()
            .chain([key.as_str()])
            .collect::<Vec<_>>()
            .join("+"),
    )
}

/// `f1` … `f99` (`/^F\d{1,2}$/` on the physical key).
#[allow(dead_code, reason = "used by the Settings shortcut recorder (M4)")]
fn is_function_key_name(key: &str) -> bool {
    key.strip_prefix('f').is_some_and(|digits| {
        (1..=2).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{ShortcutAction, ShortcutPreset};

    #[test]
    fn parts_follow_the_macos_menu_order() {
        assert_eq!(accelerator_parts("Alt+Shift+4"), ["⌥", "⇧", "4"]);
        assert_eq!(accelerator_parts("Shift+Command+4"), ["⇧", "⌘", "4"]);
        assert_eq!(
            accelerator_parts("Alt+Shift+Command+5"),
            ["⌥", "⇧", "⌘", "5"]
        );
        assert_eq!(
            accelerator_parts("Command+Shift+Alt+Control+K"),
            ["⌃", "⌥", "⇧", "⌘", "K"]
        );
        assert_eq!(
            accelerator_parts("Super+Shift+Space"),
            ["Super", "⇧", "Space"]
        );
        assert_eq!(accelerator_parts("F5"), ["F5"]);
        assert!(accelerator_parts("").is_empty());
    }

    #[test]
    fn named_keys_use_their_symbols() {
        assert_eq!(format_accelerator("Control+Return"), "⌃↩");
        assert_eq!(format_accelerator("Command+Backspace"), "⌘⌫");
        assert_eq!(format_accelerator("Command+Escape"), "⌘⎋");
        assert_eq!(format_accelerator("Command+Plus"), "⌘+");
        assert_eq!(format_accelerator("Option+Up"), "⌥↑");
    }

    #[test]
    fn formats_settings_defaults() {
        assert_eq!(format_accelerator("Control+Shift+4"), "⌃⇧4");
        assert_eq!(format_accelerator("Control+Alt+Shift+5"), "⌃⌥⇧5");
        assert_eq!(format_accelerator(""), "—");
    }

    fn modifiers(command: bool, control: bool, option: bool, shift: bool) -> Modifiers {
        Modifiers {
            command,
            control,
            option,
            shift,
        }
    }

    #[test]
    fn parses_presets_into_carbon_codes() {
        let parsed = parse("Shift+Command+4").unwrap();
        assert_eq!(parsed.modifiers, modifiers(true, false, false, true));
        assert_eq!(parsed.modifiers.carbon(), 256 | 512);
        assert_eq!(parsed.key_code, 0x15);
        assert_eq!(parsed.key_equivalent, "4");

        let parsed = parse("Control+Alt+Shift+5").unwrap();
        assert_eq!(parsed.modifiers, modifiers(false, true, true, true));
        assert_eq!(parsed.modifiers.carbon(), 4096 | 2048 | 512);
        assert_eq!(parsed.key_code, 0x17);

        let parsed = parse("Command+,").unwrap();
        assert_eq!(parsed.key_code, 0x2B);
        assert_eq!(parsed.key_equivalent, ",");

        let parsed = parse("F5").unwrap();
        assert_eq!(parsed.modifiers, Modifiers::default());
        assert_eq!(parsed.key_code, 0x60);
        assert_eq!(parsed.key_equivalent, "\u{F708}");
    }

    #[test]
    fn modifier_aliases_and_case_match_electron() {
        for command in [
            "Command",
            "Cmd",
            "CommandOrControl",
            "CmdOrCtrl",
            "Super",
            "Meta",
            "cmd",
        ] {
            let parsed = parse(&format!("{command}+K")).unwrap();
            assert_eq!(
                parsed.modifiers,
                modifiers(true, false, false, false),
                "{command}"
            );
            assert_eq!(parsed.key_code, 0x28);
            assert_eq!(parsed.key_equivalent, "k");
        }
        assert!(parse("Ctrl+K").unwrap().modifiers.control);
        assert!(parse("Control+K").unwrap().modifiers.control);
        assert!(parse("Option+K").unwrap().modifiers.option);
        assert!(parse("Alt+K").unwrap().modifiers.option);
        assert_eq!(parse("Command+a"), parse("Command+A"));
    }

    #[test]
    fn parses_named_keys() {
        let cases = [
            ("Command+Space", 0x31),
            ("Command+Return", 0x24),
            ("Command+Enter", 0x24),
            ("Command+Backspace", 0x33),
            ("Command+Delete", 0x75),
            ("Command+Tab", 0x30),
            ("Command+Escape", 0x35),
            ("Command+Up", 0x7E),
            ("Command+Down", 0x7D),
            ("Command+Left", 0x7B),
            ("Command+Right", 0x7C),
            ("Command+Plus", 0x18),
            ("Command++", 0x18),
            ("Command+-", 0x1B),
            ("Command+=", 0x18),
            ("Command+[", 0x21),
            ("Command+]", 0x1E),
            ("Command+\\", 0x2A),
            ("Command+;", 0x29),
            ("Command+'", 0x27),
            ("Command+.", 0x2F),
            ("Command+/", 0x2C),
            ("Command+`", 0x32),
            ("F1", 0x7A),
            ("F12", 0x6F),
            ("F20", 0x5A),
        ];
        for (accelerator, code) in cases {
            assert_eq!(parse(accelerator).unwrap().key_code, code, "{accelerator}");
        }
        assert_eq!(parse("Command+Up").unwrap().key_equivalent, "\u{F700}");
        assert_eq!(parse("F1").unwrap().key_equivalent, "\u{F704}");
    }

    #[test]
    fn letters_and_digits_have_distinct_codes() {
        let mut codes: Vec<u16> = ('A'..='Z')
            .chain('0'..='9')
            .map(|key| parse(&format!("Command+{key}")).unwrap().key_code)
            .collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), 36);
    }

    #[test]
    fn rejects_invalid_accelerators() {
        assert_eq!(parse(""), Err(ParseError::Empty));
        assert_eq!(
            parse("Hyper+K"),
            Err(ParseError::UnknownModifier("Hyper".into()))
        );
        assert_eq!(
            parse("Command+Banana"),
            Err(ParseError::UnknownKey("Banana".into()))
        );
        assert!(parse("Command+").is_err());
        assert!(parse("F21").is_err());
        assert!(parse("F0").is_err());
    }

    #[test]
    fn every_preset_parses() {
        for preset in [ShortcutPreset::System, ShortcutPreset::Alongside] {
            let shortcuts = preset.shortcuts();
            for action in ShortcutAction::ALL {
                let accelerator = shortcuts.get(action);
                assert!(parse(accelerator).is_ok(), "{accelerator}");
            }
        }
    }

    #[cfg(target_os = "macos")]
    mod keystrokes {
        use super::*;
        use gpui::Keystroke;

        fn accelerator(keystroke: &str) -> Option<String> {
            accelerator_from_keystroke(&Keystroke::parse(keystroke).unwrap())
        }

        #[test]
        fn builds_accelerators_in_electron_order() {
            assert_eq!(
                accelerator("cmd-shift-a").as_deref(),
                Some("Shift+Command+A")
            );
            assert_eq!(
                accelerator("ctrl-alt-shift-cmd-k").as_deref(),
                Some("Control+Alt+Shift+Command+K")
            );
            assert_eq!(accelerator("cmd-,").as_deref(), Some("Command+,"));
            assert_eq!(accelerator("ctrl-space").as_deref(), Some("Control+Space"));
            assert_eq!(accelerator("cmd-enter").as_deref(), Some("Command+Return"));
            assert_eq!(accelerator("cmd-up").as_deref(), Some("Command+Up"));
        }

        #[test]
        fn shifted_characters_map_back_to_their_physical_key() {
            // GPUI delivers ⇧⌘4 as "$" with shift cleared.
            assert_eq!(accelerator("cmd-$").as_deref(), Some("Shift+Command+4"));
            assert_eq!(
                accelerator("ctrl-alt-%").as_deref(),
                Some("Control+Alt+Shift+5")
            );
            assert_eq!(accelerator("cmd-?").as_deref(), Some("Shift+Command+/"));
        }

        #[test]
        fn bare_keys_need_a_modifier_unless_they_are_function_keys() {
            assert_eq!(accelerator("a"), None);
            assert_eq!(accelerator("4"), None);
            assert_eq!(accelerator("f5").as_deref(), Some("F5"));
            assert_eq!(accelerator("shift-f12").as_deref(), Some("Shift+F12"));
            assert_eq!(accelerator("cmd-home"), None);
            assert_eq!(accelerator("cmd-escape"), None);
        }

        #[test]
        fn modifier_only_presses_produce_nothing() {
            let mut keystroke = Keystroke::parse("cmd-a").unwrap();
            keystroke.key = "shift".into();
            assert_eq!(accelerator_from_keystroke(&keystroke), None);
            keystroke.key = String::new();
            assert_eq!(accelerator_from_keystroke(&keystroke), None);
        }

        #[test]
        fn every_preset_round_trips_through_a_keystroke() {
            for preset in [ShortcutPreset::System, ShortcutPreset::Alongside] {
                let shortcuts = preset.shortcuts();
                for action in ShortcutAction::ALL {
                    let accelerator_text = shortcuts.get(action);
                    let parsed = parse(accelerator_text).unwrap();
                    let mut keystroke = Keystroke::parse(&parsed.key_equivalent).unwrap();
                    keystroke.modifiers.platform = parsed.modifiers.command;
                    keystroke.modifiers.control = parsed.modifiers.control;
                    keystroke.modifiers.alt = parsed.modifiers.option;
                    keystroke.modifiers.shift = parsed.modifiers.shift;
                    let rebuilt = accelerator_from_keystroke(&keystroke).unwrap();
                    assert_eq!(parse(&rebuilt).unwrap(), parsed, "{accelerator_text}");
                    assert_eq!(
                        format_accelerator(&rebuilt),
                        format_accelerator(accelerator_text)
                    );
                }
            }
        }
    }
}
