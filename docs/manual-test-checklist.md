# Native app: manual test checklist

Everything the automated tests and previews could not cover, because it needs real input, a
real permission prompt, a system setting, or a bundled build. Work top to bottom; each item
lists what you should see.

Run the app against a scratch data folder unless an item says otherwise:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
SS_APPLICATION_SUPPORT_DIR=$(mktemp -d) cargo run -- app
```

With an empty folder the setup guide opens first. To skip it, put `{"onboarded":true}` in
`settings.json` inside the folder before launching.

## 1. App basics

- [ ] **No Dock icon.** While ss runs, nothing appears in the Dock or ⌘-Tab, even with a window
      open.
- [ ] **Single instance.** Run a second `cargo run -- app` with the same folder: it logs
      "ss is already running" and exits; the first keeps working.
- [ ] **Closing windows.** ⌘W on Settings, the setup guide or an editor closes it; ss keeps
      running in the menu bar.
- [ ] **⌘H** hides ss's windows. **⌘Q** (any ss window) quits.
- [ ] **Title-bar drag.** Dragging near the very top of a window (traffic-light height) moves it;
      the lower part of the 48 pt header does not (known gap).

## 2. Menu bar item

- [ ] The ss icon (template, adapts to light/dark menu bar) appears; tooltip "ss".
- [ ] **Left and right click** open the menu: Capture Area, Capture Screen, —, Record Area,
      Record Screen, —, Edit Last Capture, Open Saved Screenshots, Open Recordings, —,
      Setup Guide…, Settings… (⌘,), Quit ss (⌘Q). Capture/record rows show your shortcuts.
- [ ] Change a shortcut in Settings: the menu shows the new key equivalent next time it opens.
- [ ] **Open Saved Screenshots / Open Recordings** open `~/Pictures/ss` / `~/Movies/ss` (or the
      folders chosen in Settings) in Finder, creating them if needed.
- [ ] **Settings…** opens one Settings window; choosing it again focuses the same window.
- [ ] **Quit ss** quits; the icon disappears.

## 3. Global shortcuts

- [ ] With the "keep macOS" preset (default), ⌃⇧4 starts an area capture, ⌃⇧3 captures the
      screen under the pointer, ⌃⌥⇧4 opens the last capture in the editor.
- [ ] Pressing the capture shortcut again while the crosshair is up does nothing (one capture at
      a time).
- [ ] After **Quit**, the shortcuts are free: another app can use ⌃⇧4.
- [ ] A shortcut another app already owns shows **in use elsewhere** in red in Settings.

## 4. Capture to clipboard

- [ ] **Area:** ⌃⇧4 shows the native crosshair; drag an area (or Space for a window). The image
      is on the clipboard as PNG (paste into Preview or Slack).
- [ ] **Esc** during the crosshair cancels with no error and no notification.
- [ ] **Screen:** ⌃⇧3 captures the whole display under the pointer (try both displays).
- [ ] "After capture" = **Just copy**: only the clipboard changes, nothing opens.
- [ ] Without Screen Recording permission, a capture opens the setup guide instead of failing
      silently.
- [ ] History keeps the newest 60 PNGs (and their `.ocr.json`) in `captures/`.

## 5. Preview card

"After capture" = **Show preview** (default).

- [ ] After a capture, the card slides in at the bottom-right of the **work area** of the display
      under the pointer (above the Dock), without taking focus from the frontmost app.
- [ ] It appears above full-screen apps and follows you to other Spaces.
- [ ] The chip reads "Copied". The card hides itself after 6 s; hovering keeps it, and leaving
      restarts the 6 s.
- [ ] Hover: ✕ dismisses, **Edit** opens the editor, **Copy text** copies the recognised lines and
      the chip reads "Text copied" (or "No text found" on a capture without text).
- [ ] Clicking the thumbnail opens the editor and the card slides out.
- [ ] A new capture while the card shows hides it at once; the new one slides in.
- [ ] **Drag-out:** drag the thumbnail into Finder, Slack and Mail. The file copies; a 160 px
      image follows the pointer and snaps back if you cancel. The card behaves normally after
      the drop.

## 6. Settings window

- [ ] **Grant Access…** shows the macOS prompt (first time) or opens System Settings → Screen
      Recording; switching back to Settings refreshes the System card.
- [ ] **Use for ss / Give back to macOS** really toggles the macOS ⇧⌘3/4/5 shortcuts (check
      System Settings → Keyboard → Keyboard Shortcuts → Screenshots) and switches ss's shortcuts
      to the matching preset.
- [ ] **Shortcut recorder:** click a row ("Press keys…"), press a combination: it is saved and
      registered. Esc cancels, ⌫ clears ("—"). ⌥⇧4 shows the red "Include ⌘ or ⌃ …" hint and keeps
      recording. While recording, your existing shortcut does not trigger a capture. ⌘W is
      recorded rather than closing the window.
- [ ] **After capture** segments save immediately (check `settings.json`).
- [ ] **Change…** for each folder opens a directory picker (with New Folder); the chosen path
      shows with `~`.
- [ ] The body scrolls to the Folders section and the `ss 0.1.0` footer.
- [ ] **Open Setup Guide** opens the setup guide (one window).

## 7. Setup guide

- [ ] Opens by itself on first launch (empty `settings.json`), from the tray's **Setup Guide…**,
      from Settings, and when a capture fails for lack of permission.
- [ ] Enter triggers the primary button; Back is hidden on the first and last steps.
- [ ] **Permissions:** Grant Access → the prompt or System Settings; the button then reads
      "Open Settings" and the page updates within ~1.5 s after you switch ss (or the hosting
      terminal) on.
- [ ] **Shortcuts:** choosing "Replace macOS shortcuts" and **Use These Shortcuts** turns the
      macOS ones off and registers ⇧⌘3/4/5; a shortcut taken by another app shows
      "— taken by another app" in red and the guide stays on this step.
- [ ] **Done:** Start Using ss saves `onboarded: true` and closes the guide.

## 8. Editor

- [ ] Opens from the preview, from **Edit Last Capture** (⌃⌥⇧4) and with "After capture" =
      Open editor; size min(1240, work area − 80) × min(800, work area − 80), centred on the
      display under the pointer; can't be resized below 880 × 560. Reopening the same capture
      focuses its window.
- [ ] **Sliders** drag and click-to-set; the knob grows while dragged; the stage follows without
      flicker; the chosen style is used by the next editor.
- [ ] **Backgrounds:** 1–0 and the swatches switch; clicking **Custom** while selected opens the
      colour panel and dragging in it recolours the stage live.
- [ ] **R** cycles ratios, **[** / **]** change padding, **H** highlights recognised text.
- [ ] **Text:** drag across words to select them, double-click a word, triple-click a line; ⌘C
      copies the selected text ("Text copied"). Without a selection ⌘C copies the image
      ("Image copied"). ⇧⌘C copies all text.
- [ ] **Esc** clears a selection, then closes. Double-clicking the stage backdrop (not the image)
      copies and closes.
- [ ] **Save (⌘S)** writes `~/Pictures/ss/ss <date> at <time>.png` (or your folder) and toasts
      "Saved to ~/…". **Save As… (⇧⌘S)** opens a save panel; a name without `.png` gets it added.
- [ ] Opening a capture whose PNG was deleted shows "This capture is no longer available."

## 9. Recording

- [ ] **Record area** (⌃⇧5): a dimmed overlay covers the display under the pointer (including the
      menu bar) with the hint card. Drag a rectangle: dimming outside, `W × H` under it. Release
      to start; a tiny drag resets. **Enter** records the whole display; **Esc** or clicking
      another app cancels.
- [ ] While recording, the menu bar shows `● 0:01`, `● 0:02`, …; the tooltip reads "Click to
      stop recording".
- [ ] **Stop** by clicking the menu bar item (left or right click; see known gaps), or by
      pressing the record shortcut again.
- [ ] After stopping: `~/Movies/ss/Recording <date> at <time>.mov` exists and plays; pasting in
      Finder or Slack pastes the file; the preview shows its poster frame with a play glyph and
      "Recording copied"; hover shows **Show in Finder**.
- [ ] **Record screen** (⌃⌥⇧5) records the display under the pointer without the overlay.
- [ ] **Quit while recording:** ⌘Q / Quit ss waits for the movie to finalise (a few seconds at
      most), then quits; the `.mov` plays.

## 10. Bundled build only (after packaging, milestone 9)

- [ ] **Launch at login** switch in Settings and on the setup guide's last step registers ss
      under System Settings → General → Login Items; "requires approval" shows a link there.
- [ ] Failure notifications (e.g. a folder that can't be created) appear silently in
      Notification Centre.
