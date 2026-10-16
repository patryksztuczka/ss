//! The menu bar item (port of `Tray.ts`, structured like HEX `status_item`).
//! Menu actions go through a channel that GPUI drains on the foreground.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};

use color_eyre::eyre::{Result, eyre};
use futures::channel::mpsc::UnboundedSender;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol};
use objc2::{AllocAnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBitmapImageRep, NSEventMask, NSEventModifierFlags, NSImage, NSMenu, NSMenuItem, NSStatusBar,
    NSStatusItem,
};
use objc2_foundation::{MainThreadMarker, NSData, NSObject, NSSize, NSString};

use crate::accelerator;
use crate::actions::{Action, Folder};
use crate::settings::{ShortcutAction, Shortcuts};

const ICON_1X: &[u8] = include_bytes!("../resources/trayTemplate.png");
const ICON_2X: &[u8] = include_bytes!("../resources/trayTemplate@2x.png");
const ICON_POINTS: f64 = 18.0;
const VARIABLE_LENGTH: f64 = -1.0;

/// Set by the recorder (M7): a left click then stops instead of opening the menu.
static RECORDING: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuEntry {
    Item {
        title: &'static str,
        action: Action,
        /// Display-only key equivalent; the global hotkeys do the work.
        accelerator: Option<String>,
    },
    Separator,
}

/// The menu from `Tray.ts`, with the user's accelerators.
pub fn menu_entries(shortcuts: &Shortcuts) -> Vec<MenuEntry> {
    let item = |title, action, accelerator: Option<&str>| MenuEntry::Item {
        title,
        action,
        accelerator: accelerator.map(str::to_owned),
    };
    let shortcut = |action| Some(shortcuts.get(action));
    vec![
        item(
            "Capture Area",
            Action::CaptureArea,
            shortcut(ShortcutAction::CaptureArea),
        ),
        item(
            "Capture Screen",
            Action::CaptureScreen,
            shortcut(ShortcutAction::CaptureScreen),
        ),
        MenuEntry::Separator,
        item(
            "Record Area",
            Action::RecordArea,
            shortcut(ShortcutAction::RecordArea),
        ),
        item(
            "Record Screen",
            Action::RecordScreen,
            shortcut(ShortcutAction::RecordScreen),
        ),
        MenuEntry::Separator,
        item(
            "Edit Last Capture",
            Action::OpenLast,
            shortcut(ShortcutAction::OpenLast),
        ),
        item(
            "Open Saved Screenshots",
            Action::OpenFolder(Folder::Images),
            None,
        ),
        item(
            "Open Recordings",
            Action::OpenFolder(Folder::Recordings),
            None,
        ),
        MenuEntry::Separator,
        item("Setup Guide…", Action::OpenOnboarding, None),
        item("Settings…", Action::OpenSettings, Some("Command+,")),
        item("Quit ss", Action::Quit, Some("Command+Q")),
    ]
}

#[derive(Debug)]
struct TargetIvars {
    actions: UnboundedSender<Action>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetIvars]
    struct StatusItemTarget;

    unsafe impl NSObjectProtocol for StatusItemTarget {}

    impl StatusItemTarget {
        #[unsafe(method(menuItemSelected:))]
        fn menu_item_selected(&self, sender: &NSMenuItem) {
            if let Some(action) = action_for_tag(sender.tag()) {
                self.send(action);
            }
        }

        /// Only reached while recording: otherwise the menu is attached and
        /// AppKit opens it without sending the action. A right click stops
        /// too: menus popped up by the app are not drawn on macOS 26, where
        /// Control Centre hosts status items.
        #[unsafe(method(statusItemClicked:))]
        fn status_item_clicked(&self, _sender: &AnyObject) {
            if RECORDING.load(Ordering::Acquire) {
                self.send(Action::StopRecording);
            }
        }
    }
);

impl StatusItemTarget {
    fn new(actions: UnboundedSender<Action>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars { actions });
        unsafe { msg_send![super(this), init] }
    }

    fn send(&self, action: Action) {
        let _ = self.ivars().actions.unbounded_send(action);
    }
}

struct Tray {
    item: Retained<NSStatusItem>,
    menu: Retained<NSMenu>,
    target: Retained<StatusItemTarget>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Adds the status item. Menu choices are sent to `actions`.
pub fn install(actions: UnboundedSender<Action>, shortcuts: &Shortcuts) -> Result<()> {
    let mtm = MainThreadMarker::new().ok_or_else(|| eyre!("the tray needs the main thread"))?;
    let target = StatusItemTarget::new(actions, mtm);
    let menu = build_menu(shortcuts, &target, mtm);
    let item = NSStatusBar::systemStatusBar().statusItemWithLength(VARIABLE_LENGTH);
    let button = item
        .button(mtm)
        .ok_or_else(|| eyre!("status item button is unavailable"))?;
    let image = icon()?;
    button.setImage(Some(&*image));
    button.setToolTip(Some(&NSString::from_str("ss")));
    // The menu stays attached (as in HEX; on macOS 26 Control Centre draws
    // status items and only shows attached menus). While recording it is
    // detached so a left click reaches `statusItemClicked:` and stops.
    item.setMenu(Some(&menu));
    unsafe {
        button.setTarget(Some(&target));
        button.setAction(Some(sel!(statusItemClicked:)));
    }
    button.sendActionOn(NSEventMask::LeftMouseUp | NSEventMask::RightMouseUp);
    TRAY.with(|tray| *tray.borrow_mut() = Some(Tray { item, menu, target }));
    Ok(())
}

/// Rebuilds the menu so its key equivalents match `shortcuts`.
pub fn update_menu(shortcuts: &Shortcuts) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    TRAY.with(|tray| {
        if let Some(tray) = tray.borrow_mut().as_mut() {
            tray.menu = build_menu(shortcuts, &tray.target, mtm);
            if !RECORDING.load(Ordering::Acquire) {
                tray.item.setMenu(Some(&tray.menu));
            }
        }
    });
}

/// While recording the menu is detached, so any click on the item stops
/// (left click as in Electron; see `status_item_clicked`), and the
/// tooltip changes. The title is set by [`set_title`].
pub fn set_recording(recording: bool) {
    RECORDING.store(recording, Ordering::Release);
    TRAY.with(|tray| {
        let tray = tray.borrow();
        let Some(tray) = tray.as_ref() else {
            return;
        };
        tray.item.setMenu((!recording).then_some(&*tray.menu));
        if let Some(button) = MainThreadMarker::new().and_then(|mtm| tray.item.button(mtm)) {
            let tooltip = if recording {
                "Click to stop recording"
            } else {
                "ss"
            };
            button.setToolTip(Some(&NSString::from_str(tooltip)));
            if !recording {
                button.setTitle(&NSString::new());
            }
        }
    });
}

/// The text beside the icon, e.g. `● 0:42`.
pub fn set_title(title: &str) {
    TRAY.with(|tray| {
        if let Some(button) = tray
            .borrow()
            .as_ref()
            .and_then(|tray| MainThreadMarker::new().and_then(|mtm| tray.item.button(mtm)))
        {
            button.setTitle(&NSString::from_str(title));
        }
    });
}

/// `m:ss`, like `formatElapsed` in `Tray.ts`.
pub fn format_elapsed(elapsed: std::time::Duration) -> String {
    let total = elapsed.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}

/// Takes the item out of the menu bar (on quit).
pub fn remove() {
    if let Some(tray) = TRAY.with(|tray| tray.borrow_mut().take()) {
        NSStatusBar::systemStatusBar().removeStatusItem(&tray.item);
    }
}

fn build_menu(
    shortcuts: &Shortcuts,
    target: &StatusItemTarget,
    mtm: MainThreadMarker,
) -> Retained<NSMenu> {
    let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str("ss"));
    for entry in menu_entries(shortcuts) {
        let MenuEntry::Item {
            title,
            action,
            accelerator,
        } = entry
        else {
            menu.addItem(&NSMenuItem::separatorItem(mtm));
            continue;
        };
        let parsed = accelerator
            .as_deref()
            .and_then(|text| accelerator::parse(text).ok());
        let key = parsed.as_ref().map_or_else(NSString::new, |parsed| {
            NSString::from_str(&parsed.key_equivalent)
        });
        let item = unsafe {
            NSMenuItem::initWithTitle_action_keyEquivalent(
                NSMenuItem::alloc(mtm),
                &NSString::from_str(title),
                Some(sel!(menuItemSelected:)),
                &key,
            )
        };
        if let Some(parsed) = parsed {
            item.setKeyEquivalentModifierMask(modifier_flags(parsed.modifiers));
        }
        item.setTag(tag_for_action(action));
        unsafe { item.setTarget(Some(target)) };
        menu.addItem(&item);
    }
    menu
}

fn modifier_flags(modifiers: accelerator::Modifiers) -> NSEventModifierFlags {
    let mut flags = NSEventModifierFlags::empty();
    if modifiers.command {
        flags |= NSEventModifierFlags::Command;
    }
    if modifiers.control {
        flags |= NSEventModifierFlags::Control;
    }
    if modifiers.option {
        flags |= NSEventModifierFlags::Option;
    }
    if modifiers.shift {
        flags |= NSEventModifierFlags::Shift;
    }
    flags
}

/// Menu item tags index this table.
const TAGGED_ACTIONS: [Action; 11] = [
    Action::CaptureArea,
    Action::CaptureScreen,
    Action::RecordArea,
    Action::RecordScreen,
    Action::OpenLast,
    Action::OpenFolder(Folder::Images),
    Action::OpenFolder(Folder::Recordings),
    Action::OpenOnboarding,
    Action::OpenSettings,
    Action::Quit,
    Action::StopRecording,
];

fn tag_for_action(action: Action) -> isize {
    TAGGED_ACTIONS
        .iter()
        .position(|candidate| *candidate == action)
        .expect("every menu action is tagged") as isize
}

fn action_for_tag(tag: isize) -> Option<Action> {
    TAGGED_ACTIONS.get(usize::try_from(tag).ok()?).copied()
}

/// The 18 pt template image, with the @2x bitmap for Retina displays.
fn icon() -> Result<Retained<NSImage>> {
    let size = NSSize::new(ICON_POINTS, ICON_POINTS);
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(ICON_1X))
        .ok_or_else(|| eyre!("tray icon is not a valid image"))?;
    let retina = NSBitmapImageRep::imageRepWithData(&NSData::with_bytes(ICON_2X))
        .ok_or_else(|| eyre!("@2x tray icon is not a valid image"))?;
    retina.setSize(size);
    image.addRepresentation(&retina);
    image.setSize(size);
    image.setTemplate(true);
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ShortcutPreset;

    #[test]
    fn menu_matches_tray_ts() {
        let entries = menu_entries(&ShortcutPreset::System.shortcuts());
        let layout: Vec<_> = entries
            .iter()
            .map(|entry| match entry {
                MenuEntry::Item {
                    title, accelerator, ..
                } => format!("{title} {}", accelerator.as_deref().unwrap_or("")),
                MenuEntry::Separator => "---".into(),
            })
            .collect();
        assert_eq!(
            layout,
            [
                "Capture Area Shift+Command+4",
                "Capture Screen Shift+Command+3",
                "---",
                "Record Area Shift+Command+5",
                "Record Screen Alt+Shift+Command+5",
                "---",
                "Edit Last Capture Alt+Shift+Command+4",
                "Open Saved Screenshots ",
                "Open Recordings ",
                "---",
                "Setup Guide… ",
                "Settings… Command+,",
                "Quit ss Command+Q",
            ]
        );
    }

    #[test]
    fn elapsed_time_reads_minutes_and_seconds() {
        use std::time::Duration;
        assert_eq!(format_elapsed(Duration::from_millis(999)), "0:00");
        assert_eq!(format_elapsed(Duration::from_secs(42)), "0:42");
        assert_eq!(format_elapsed(Duration::from_secs(61)), "1:01");
        assert_eq!(format_elapsed(Duration::from_secs(3600)), "60:00");
    }

    #[test]
    fn every_menu_action_has_a_tag() {
        for entry in menu_entries(&Shortcuts::default()) {
            if let MenuEntry::Item { action, .. } = entry {
                assert_eq!(action_for_tag(tag_for_action(action)), Some(action));
            }
        }
        assert_eq!(action_for_tag(-1), None);
        assert_eq!(action_for_tag(99), None);
    }

    #[test]
    fn unassigned_shortcuts_have_no_key_equivalent() {
        let mut shortcuts = Shortcuts::default();
        shortcuts.set(ShortcutAction::OpenLast, "");
        let entries = menu_entries(&shortcuts);
        let edit_last = entries
            .iter()
            .find(|entry| {
                matches!(
                    entry,
                    MenuEntry::Item {
                        title: "Edit Last Capture",
                        ..
                    }
                )
            })
            .unwrap();
        let MenuEntry::Item { accelerator, .. } = edit_last else {
            unreachable!()
        };
        assert!(
            accelerator
                .as_deref()
                .and_then(|text| accelerator::parse(text).ok())
                .is_none()
        );
    }
}
