# ss: Electron → native Rust/GPUI rewrite plan

Status: **milestones 1–7 implemented; 8–10 deferred.** See [Status](#status) and
[`manual-test-checklist.md`](manual-test-checklist.md).

## Status

| M | Milestone | State | Tests (cumulative) |
|---|-----------|-------|--------------------|
| 1 | Skeleton | done | 30 |
| 2 | Migration + accelerators + hotkeys | done (migration is read-only; cleanup deferred) | 65 with M3 |
| 3 | Tray + capture-to-clipboard | done | 65 |
| 4 | System + Settings + Onboarding | done | 84 |
| 5 | OCR + Preview | done (OCR matches the Swift helper exactly) | 100 |
| 6 | Editor | done (compositor matches Electron: mean ≤ 0.21 levels per channel) | 129 |
| 7 | Recording | done | 140 |
| 8 | Updates (Sparkle) | **deferred** | — |
| 9 | Packaging (signing, notarization, DMG, Info.plist, scripts) | **deferred** | — |
| 10 | Cut-over (delete `apps/desktop`, pnpm/Vite config) | **deferred**; Electron stays the parity reference | — |

Gates at every milestone: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`,
`cargo test`; `cargo build --release` builds too.

Also deferred:
- The one-time Chromium-cache cleanup and `.migrated-from-electron` marker (§6, D3), while the
  Electron app is still in use. The data folder is read in place.
- Re-registering an Electron-era login item (§6); launch at login only works from a signed bundle.
- Notifications (`UNUserNotificationCenter`) and launch at login only run bundled; unbundled
  builds log instead.

### Parity evidence

- Settings: the real `settings.json` round-trips byte for byte; defaults, presets and
  `isUsableAccelerator` are ported with tests.
- OCR: identical lines, words and boxes (≤ 1e-16) against the Electron caches of two real
  captures; a committed fixture compares against the Swift helper. Line order reproduces V8's
  `Array.prototype.sort` (TimSort), checked against 60 cases recorded from node.
- Compositor: six styles rendered by the real `compose.ts` in Electron
  (`scripts/fixtures/compose-reference/`) match within mean 0.00–0.21 / max 1–31 levels;
  layouts and edge colours match exactly (also on two real captures). Sources are
  colour-managed from their ICC profile like Chromium.
- Recording: a real 3 s `screencapture -v` run through the recorder stops on SIGINT in 0.02 s
  and yields a 2.8 s movie; SIGTERM escalation is tested with fake children.

## Known gaps vs Electron

Visual
1. No `backdrop-blur`: the preview chip and buttons, and the region hint card use more opaque
   fills (§7.6).
2. No letter-spacing (`tracking-*`), `tabular-nums`, or `active:scale` press effects; swatches
   don't scale on hover.
3. `SF Mono` is not a public font family, so mono text uses Menlo.
4. Animation approximations: the loading/OCR shimmer is a colour pulse, not a moving gradient;
   the welcome cards rise together (no stagger); preview hover controls fade in but vanish
   instantly; the thumbnail's 1.02 hover zoom is a slightly larger box; the launch-at-login
   switch doesn't spring; the onboarding icon has no drop shadow.
5. The Custom swatch's "+" picks black or white by brightness instead of `mix-blend-difference`.
6. Compositor output differs from Chromium by a few levels at antialiased edges and in the shadow
   (a three-pass box approximation of Skia's blur).
7. OCR glyph boxes are the Vision boxes; Electron sized invisible text at 0.92× height with a
   0.04× offset, so highlights sit slightly differently. Selection is word-granular (§7.7); the
   copied text is the same.

Behaviour
8. The window can only be dragged by AppKit's native ~28 pt titlebar strip, not the full 48 pt
   header (GPUI ignores drag regions on macOS; a manual `performWindowDragWithEvent` risks
   re-entering GPUI).
9. Sliders have no arrow-key control, and a drag that leaves the window stops updating until the
   pointer returns (GPUI only delivers moves to hovered elements).
10. While recording, a **right** click on the menu bar item also stops the recording: on macOS 26
    Control Centre hosts status items, and menus popped up by the app (`popUpMenu…`,
    `setMenu`+`performClick`) are not drawn. Left click stops, as before.
11. The status item of an unbundled build is unnamed (shown by pid) and not exposed to
    Accessibility.
12. Quit blocks the main thread for up to 5 s + 2 s while a recording finalises (plan R7,
    intended).
13. The OCR cache may write tiny numbers in exponent form (`1.2e-6`) where JavaScript writes
    decimals; both apps read either. The in-memory OCR cache keeps 32 results with no 1 h expiry.
14. Recordings go on the clipboard as `public.file-url` (what `osascript … POSIX file` produced),
    written eagerly.
15. VoiceOver support is weaker than Chromium's (§7.9).
16. Debug builds optimise dependencies (`[profile.dev.package."*"]`); image work is too slow
    otherwise.


### Resolved decisions (2026-10-12)

- **D1:** macOS only (15+, Apple silicon). No Linux and no cross-platform seams.
- **Scope cut:** updates (Sparkle, milestone 8) and release (signing, notarization, DMG, appcast, milestone 9) are **deferred**. Milestone 10 (deleting `apps/desktop`) is deferred too, because the Electron app stays as the parity reference.
- **D2:** keep `dev.sztuczka.ss` (recommended default; not yet objected to).
- **D3:** read `~/Library/Application Support/ss` in place. The Chromium-cache cleanup is **deferred** while the Electron app is still in use; only the read-only migration runs now.
- **D4:** not applicable while updates are deferred.

Source of truth for this inventory: `apps/desktop` at the time of writing (Electron 44, Effect 4, React 19,
Tailwind 4, ~5,000 lines of TS/TSX plus a 160-line Swift helper). Reference stack: HEX
(`github.com/anomalyco/hex`): `Cargo.toml`, `AGENTS.md`, `src/desktop_ui.rs`, `scripts/`.

---

## 0. Decisions needed before step 2

The brief left three placeholders open. My recommendation is first in each list.

| # | Question | Recommendation | Why |
|---|----------|----------------|-----|
| D1 | Targets | **macOS 15+, Apple silicon only** | Every feature sits on a macOS-only API: the `screencapture` binary, Vision OCR, `com.apple.symbolichotkeys`, SMAppService, NSStatusItem. A Linux build would be a different app with nothing shared except the compositor. The Electron build currently declares `LSMinimumSystemVersion 13.0`, so this drops macOS 13–14 and Intel (listed in §7). |
| D2 | Bundle ID | **Keep `dev.sztuczka.ss`** | The Electron app has no auto-updater, so there is no update chain to preserve and nothing forces a new ID. Keeping it keeps the same data folder name and the same login-item identity. Screen Recording permission has to be granted again either way, because the code signature changes (ad-hoc → Developer ID). |
| D3 | Existing data | **`~/Library/Application Support/ss`** (checked on this machine: `settings.json`, `captures/` with `*.png` + `*.ocr.json`, plus ~20 Chromium cache entries) | The Rust app reads the same folder in place. See §6. |
| D4 | Where Sparkle updates are hosted (`SUFeedURL`) | **GitHub Releases**: a stable feed at `https://github.com/<owner>/ss/releases/latest/download/appcast.xml`, with the DMG/ZIP as release assets | No extra infrastructure. HEX uses Cloudflare R2 with a GitHub mirror, which only pays off with heavier traffic. Needs the repo owner name and confirmation that the repo will be public. |

---

## 1. What the app is

`ss` is a menu-bar-only (no Dock icon, `LSUIElement`) replacement for the macOS screenshot tool:
global shortcuts → capture/record via macOS's own `screencapture` binary → clipboard → floating preview →
optional editor that adds a background, padding, inset, rounded corners, shadow and aspect ratio, with an
invisible OCR text layer for selecting text.

### 1.1 Windows / screens

| Window | Electron config | Contents and behaviour |
|--------|-----------------|------------------------|
| **Tray** (NSStatusItem) | `Tray` + template PNG 18/36 px, tooltip "ss" | Left click: if recording, stop; otherwise pop up the menu. Right click: menu. While recording the title reads `● m:ss`, ticking every second, with the tooltip "Click to stop recording". Menu: Capture Area, Capture Screen, ─, Record Area, Record Screen, ─, Edit Last Capture, Open Saved Screenshots, Open Recordings, ─, Setup Guide…, Settings… (⌘,), Quit ss (⌘Q). Capture items show the user's accelerators as display-only key equivalents (`registerAccelerator: false`). The menu is rebuilt when settings change. |
| **Preview** | frameless, transparent, no shadow, non-focusable, `showInactive`, always-on-top `floating` level, visible on all Spaces incl. fullscreen; width 248+2×18 pt; height = clamp(248·aspect, 120, 220)+36; bottom-right of the **work area** of the display under the cursor, 6 pt inset | Card with rounded 14 pt corners and a layered shadow; slide-in (520 ms spring) / slide-out (260 ms). Auto-hides after 6 s unless hovered. Status chip ("Copied" / "Recording copied" / "Text copied" / "No text found"). Video: play glyph overlay. Hover reveals a dismiss ✕ and buttons: image → **Edit**, **Copy text**; video → **Show in Finder**. Clicking the thumbnail edits (image) or reveals (video). **Dragging the thumbnail drags the file out** to other apps, using a 160 px thumbnail as the drag icon. A single window is reused. |
| **Editor** (one per capture id; reopening focuses the existing one) | `hiddenInset` titlebar, traffic lights at (18,18), bg `#f7f5f2`; size min(1240, work−80) × min(800, work−80) centred on the display under the cursor; min 880×560 | **Stage**: composition preview scaled to fit with a 56 pt gutter; checkerboard when the background is "none"; header shows `W × H`; footer has Copy (⌘C, primary), Save (⌘S), Save As… (⇧⌘S), hint text and zoom %. Double-clicking the backdrop copies and closes. **Sidebar** (300 pt): Spacing (Padding, Inset sliders; inset hint shows the detected edge colour), Card (Radius, Shadow), Background (10 swatches in a 5-col grid; clicking Custom while it is selected opens the colour picker), Size (segmented Auto/16:9/4:3/3:2/1:1), Text (OCR status with shimmer, Copy all ⇧⌘C, Show/H highlight toggle). **Keys**: ⌘C copies selected text, or the image if nothing is selected; ⇧⌘C; ⌘S/⇧⌘S; ⌘W; Esc clears the selection, then closes; H; R cycles the ratio; `[`/`]` ±4 padding; 1–0 pick backgrounds. Toasts last 2.2 s. The style is saved to settings 400 ms after the last change. Loading state shows a shimmer skeleton; failure state reads "This capture is no longer available." |
| **Settings** | 560×760, not resizable, not minimizable, `hiddenInset` | **System** card: Screen Recording (status pill or "Grant Access…"; dev hint names the host terminal), macOS screenshot shortcuts ("Use for ss" / "Give back to macOS"), Launch at login switch (disabled when unpackaged); "Open Setup Guide" link. **Shortcuts**: 5 rows; click to record, Esc cancels, ⌫ clears, rejects ⌥/⇧-only combos with a red hint; per-row "in use elsewhere". Global shortcuts are **paused while recording**. **After capture** segmented (Show preview / Open editor / Just copy). **Folders**: images and recordings, path shown with `~`, "Change…" opens a directory picker. Footer `ss <version>`. Re-reads setup state on window focus. Live-updates on `settings:changed`. |
| **Onboarding / Setup Guide** | 760×560, fixed, `hiddenInset` | 4 steps with progress dots: **Welcome** (app icon, 3 feature cards), **Permissions** (Grant Access → prompt + System Settings; polls every 1.5 s after asking; dev host hint), **Shortcuts** (two preset cards, "Recommended" badge, key caps, 5-row table with per-row failure), **Done** (big key caps for capture-area, launch-at-login switch). Footer: Back, primary button whose label depends on the step (↩ triggers it). Finish sets `onboarded: true` and closes. Opens automatically on first launch, on capture when permission is missing, and from the tray/Settings. |
| **Region overlay** (record area) | frameless, transparent, covers `display.bounds`, `screen-saver` level, all Spaces | Crosshair cursor, dimmed backdrop (25% black, or 32% outside the selection), white 1 px outline, `W × H` label below the rect, hint card ("Drag to choose what to record · ↩ whole screen · Esc cancel …") with a red dot. Esc cancels, Enter selects the whole display, drags under 8 pt reset. Losing focus cancels. After a selection, waits 180 ms for the fade before recording. |

### 1.2 User flows

1. **First launch** → onboarding.
2. **Capture area/window** (shortcut or tray) → hide preview → `screencapture -i -x -o <captures>/<id>.png` (native crosshair, Space for window, Esc cancels) → register → PNG to clipboard → OCR prefetch in the background → `afterCapture`: preview | editor | nothing. One capture at a time; repeat presses while busy are ignored.
3. **Capture screen** → `screencapture -x -R<display bounds>` for the display under the cursor → same as 2.
4. **Record area** → region overlay → `screencapture -v -x -R… <recordings>/Recording <date> at <time>.mov` → tray timer → stop via shortcut again, tray click, or quit (SIGINT, then SIGTERM after 5 s) → file URL on clipboard → preview.
5. **Record screen** → same without the overlay (whole display under the cursor).
6. **Edit last capture** → latest capture (survives restarts by scanning `captures/`); a video is revealed in Finder instead; "Nothing captured yet" notification if there is none.
7. **Editor export** → full-resolution PNG → clipboard / `~/Pictures/ss/ss <date> at <time>.png` / Save panel.
8. **Copy text** (preview) → OCR → lines joined with `\n` → clipboard → line count shown in the chip.
9. **Missing Screen Recording permission** during a capture (stderr matches `could not create image|not authorized|permission|declined`) → open onboarding. Other failures → log + system notification.
10. **Shortcut presets** "system" (⇧⌘3/4/5…) switches macOS's own screenshot hotkeys **off** (CFPreferences + `activateSettings -u`); "alongside" (⌃⇧3/4/5…) switches them back **on**.
11. **Quit** → stop the recording (finalise the movie), unregister hotkeys, remove the tray item, with a 5 s cap.
12. **Pruning**: only the newest 60 PNGs (and their `.ocr.json`) are kept in `captures/`.

---

## 2. IPC inventory → replacement

All 25 invoke channels and 2 events go away. In Rust, views call core functions directly. Anything that
blocks (child processes, OCR, file I/O, PNG encode, compositing) runs on GPUI's background executor
(`cx.background_spawn`) and returns to the view through the task.

| Channel | Handler today | Rust replacement |
|---------|---------------|------------------|
| `app:info` | version, default dirs | `env!("CARGO_PKG_VERSION")`, `paths::Paths` |
| `settings:get` | SettingsStore.get | read the `Entity<SettingsModel>` (GPUI global) |
| `settings:update` | persist + re-register shortcuts + broadcast | `SettingsModel::update(patch, cx)` → write JSON, `cx.notify()`; hotkeys re-registered by an observer that returns the failed actions |
| `settings:pick-directory` | `dialog.showOpenDialog` | `cx.prompt_for_paths(PathPromptOptions { directories: true, .. })` → update settings |
| `setup:state` | System.state | `system::setup_state()` on the background executor (spawns the permission probe, §3) |
| `setup:request-screen-access` | helper + open Settings URL | `system::request_screen_access()` |
| `setup:apply-shortcuts` | toggle macOS hotkeys + preset + apply | `system::apply_preset(preset)` → returns `SettingsUpdateResult { settings, failed }` |
| `setup:launch-at-login` | `app.setLoginItemSettings` | `login_item::set_enabled()` via SMAppService |
| `setup:open` / `setup:finish` | windows | `windows::open_onboarding(cx)` / settings update + window close |
| `shortcuts:pause` | `globalShortcut.unregisterAll` / re-apply | `Hotkeys::pause(bool)` |
| `capture:load` | read PNG bytes + size | decode PNG on the background executor → `Arc<RenderImage>` + `image::RgbaImage` for compositing |
| `capture:ocr` | Ocr.recognize (cached) | `ocr::recognize(id)` deduped through a `HashMap<CaptureId, Shared<Task<…>>>` |
| `clipboard:write-image` / `write-text` | Electron clipboard | `clipboard::write_png` / `write_text` |
| `image:save` | quick write or Save panel | `export::save_quick` / `cx.prompt_for_new_path` + write |
| `preview:edit` / `reveal` / `copy-text` / `dismiss` | Actions | direct `Actions` methods |
| `preview:drag` | `webContents.startDrag` | **native drag source**, §5 risk R4 |
| `region:submit` / `region:cancel` | resolve a Deferred | the region view resolves a `oneshot::Sender<Option<Rect>>` |
| `window:close` | close sender | `window.remove_window()` |
| event `preview:show` | push payload | `PreviewView::show(payload, cx)` on the preview entity |
| event `settings:changed` | broadcast to all windows | views `cx.observe(&settings, …)` |

Schema validation at the process boundary disappears with the boundary. Settings and OCR JSON are still
validated at the **disk** boundary (serde, §6).

---

## 3. Electron / Node API and dependency inventory → replacement

### 3.1 Electron APIs

| API (file) | Use | Replacement |
|------------|-----|-------------|
| `app.whenReady`, `app.on('window-all-closed')`, `before-quit` (index.ts) | lifecycle, graceful shutdown | `gpui::Application::new().run(…)`. GPUI keeps running with no windows. Quit: `cx.on_app_quit`, but **its timeout is short** (R7), so the recorder is stopped synchronously in the quit path (SIGINT + bounded `waitpid`, ≤5 s) before calling `cx.quit()`. |
| `app.requestSingleInstanceLock` | second instance quits | `fs2::try_lock_exclusive` on `~/Library/Application Support/ss/ss.lock` (as HEX); the second instance exits 0. |
| `app.dock.hide()` + `LSUIElement` | no Dock icon | `NSApplication.setActivationPolicy(.accessory)` (objc2-app-kit) + `LSUIElement` in Info.plist. |
| `app.setName/setPath('userData')` | `~/Library/Application Support/ss` | `dirs::data_dir().join("ss")`. |
| `app.getPath('pictures'/'videos')` | default dirs | `dirs::picture_dir()/ss`, `dirs::video_dir()/ss`. |
| `app.getVersion`, `app.isPackaged` | about, dev checks | `CARGO_PKG_VERSION`; packaged = `NSBundle.mainBundle.bundleIdentifier == Some("dev.sztuczka.ss")`. |
| `app.focus({steal:true})` | bring windows forward from an accessory app | `cx.activate(true)` + `window.activate_window()`. |
| `app.setLoginItemSettings` / `getLoginItemSettings` | launch at login | `objc2-service-management`: `SMAppService.mainAppService` `registerAndReturnError` / `unregisterAndReturnError` / `status`. Only available when bundled (same as today). |
| `BrowserWindow` ×5 kinds | windows | `cx.open_window(WindowOptions{…})`, see §4. |
| `webContents.send`, `ipcMain.handle`, `contextBridge`, preload | IPC | removed (§2). |
| `webContents.setWindowOpenHandler` → `shell.openExternal` | external links | not needed (no web content, no links). |
| `globalShortcut.register/unregisterAll` | 5 global hotkeys | Carbon `RegisterEventHotKey` / `UnregisterEventHotKey` + `InstallEventHandler`. Electron uses the same API on macOS, so the "⌥-only never fires" rule and "in use elsewhere" failures behave the same. This is a small `extern "C"` FFI module, because no objc2 binding exists for Carbon. HEX uses CGEventTap here; §8 explains why we don't. |
| `Tray`, `Menu.buildFromTemplate`, `nativeImage.setTemplateImage`, `tray.setTitle/setToolTip/popUpContextMenu` | menu bar | objc2-app-kit: `NSStatusBar.systemStatusBar.statusItemWithLength(variable)`, `button.setImage` (template `NSImage` from `include_bytes!` PNGs), `button.setTitle`, `setToolTip`, `NSMenu`/`NSMenuItem` with `keyEquivalent` + `keyEquivalentModifierMask` for display. Left/right click: a `define_class!` target; for left click, if recording → stop, else assign the menu and `performClick`. |
| `screen.getCursorScreenPoint`, `getDisplayNearestPoint`, `display.bounds/workArea` | target display | `NSEvent.mouseLocation` + `NSScreen.screens` `frame`/`visibleFrame`, converted from Cocoa's bottom-left coordinates to the top-left global points `screencapture -R` expects. Pure conversion function with unit tests (R8). |
| `nativeImage.createFromPath(...).getSize/resize/toDataURL` | thumbnails, PNG size | `image` crate (PNG decode, `resize` with `FilterType::Triangle`/`Lanczos3`) on the background executor → `gpui::RenderImage`. No data URLs. |
| `nativeImage.createThumbnailFromPath` | video poster frame (QuickLook) | `objc2-quick-look-thumbnailing` (`QLThumbnailGenerator`) → CGImage → RGBA. Fallback: `objc2-av-foundation` `AVAssetImageGenerator`. |
| `Notification` | error/"Nothing captured yet" | `objc2-user-notifications` (`UNUserNotificationCenter`), bundled builds only; unbundled `cargo run` logs instead (UN crashes without a bundle). |
| `shell.openExternal(x-apple.systempreferences:…Privacy_ScreenCapture)` | open System Settings | `NSWorkspace.sharedWorkspace.openURL`. |
| `shell.showItemInFolder` | reveal | `NSWorkspace.activateFileViewerSelectingURLs`. |
| `shell.openPath(dir)` | open folder | `NSWorkspace.openURL(fileURL)`. |
| `systemPreferences.getMediaAccessStatus('screen')` | fallback permission check | `CGPreflightScreenCaptureAccess` in-process (objc2-core-graphics). |
| `dialog.showOpenDialog` (directory, createDirectory) | folder picker | `cx.prompt_for_paths`. |
| `dialog.showSaveDialog` (default path, `.png` filter) | Save As… | `cx.prompt_for_new_path(dir, suggested_name)`; if GPUI's panel can't take the `png` filter, force the `.png` extension on the returned path (R9). |
| `clipboard.write([ClipboardItem image/png])` | image to clipboard | **NSPasteboard `setData:forType: public.png`** with the exact PNG bytes. `arboard::set_image` takes RGBA and re-encodes as TIFF, which changes what other apps receive. `arboard` is used for text. |
| `clipboard.writeText` | text | `arboard::Clipboard::set_text`. |
| `osascript … set the clipboard to POSIX file` | file reference for recordings | NSPasteboard `writeObjects:[NSURL fileURLWithPath:]` (no osascript child process). |
| `event.sender.startDrag({file, icon})` | drag a file out | native `NSDraggingSource`, R4. |
| Default Electron app menu (implicit) | ⌘W close, ⌘Q quit, ⌘H hide, Edit menu while a window is key | GPUI has no implicit menu. Bind `cmd-w` → close window, `cmd-q` → quit, `cmd-h` → hide in a key context active in every window. ⌘C/⌘S etc. are explicit editor actions. |

### 3.2 Node APIs

| API | Use | Replacement |
|-----|-----|-------------|
| `fs` (via Effect `FileSystem`): readFile/writeFile/readDirectory/makeDirectory/remove/exists | settings, captures, OCR cache, exports | `std::fs` on the background executor. |
| `child_process` (via Effect `ChildProcessSpawner`) | `screencapture`, `ss-helper`, `osascript` | `std::process::Command`. The recording child keeps a piped stdin (screencapture treats EOF as stop), is stopped with `libc::kill(pid, SIGINT)`, waited on a background thread, escalating to SIGTERM after 5 s and giving up after 2 s more. |
| `path`, `process.env` (`VITE_DEV_SERVER_URL`, `__CFBundleIdentifier`, `TERM_PROGRAM`), `process.resourcesPath`, `__dirname` | paths, dev permission host | `std::env`; resources are compiled in with `include_bytes!` so no resource path lookup. The dev-host terminal table is kept as-is. |
| `setTimeout/setInterval/requestAnimationFrame/ResizeObserver` (renderer) | debounce, polling, render scheduling, stage size | `cx.spawn` + `Timer::after`; GPUI layout gives sizes each frame (`canvas`/`on_children_prepainted` or a bounds observer). |

### 3.3 npm dependencies

| Package | Role | Fate |
|---------|------|------|
| `electron` 44, `electron-builder` 26 | runtime, packaging | → GPUI binary + `scripts/build-app.sh`, `release-app.sh`, `macos-signing.sh`, `validate-app.sh`. |
| `effect` 4, `@effect/platform-node` | services/layers, Schema, Stream, Cache, Semaphore, Deferred | plain structs in modules; `serde`; GPUI entities and observers for streams; `HashMap<_, Shared<Task>>` for the cache; `AtomicBool` for the busy semaphore; `oneshot` for Deferred. |
| `react`, `react-dom`, `tailwindcss`, `@tailwindcss/vite`, `@vitejs/plugin-react` | UI | GPUI views + `desktop_ui.rs` (tokens from `index.css` `@theme`). |
| `vite-plus`, `typescript`, `@types/*` | tooling | `cargo`, `cargo fmt`, `cargo clippy`. |

There are **no** native npm modules and **no network calls** (no fetch/http/update checks). Sparkle is
the first network feature (§7).

### 3.4 Native helper (`native/helper.swift`, compiled with `swiftc`)

| Subcommand | Rust replacement | Notes |
|------------|------------------|-------|
| `ocr <image>` | `ocr.rs` using **objc2-vision** (`VNRecognizeTextRequest`, `.accurate`, language correction, auto language; `VNRecognizedText.boundingBoxForRange` per word via `NSString.enumerateSubstringsInRange…ByWords`) in-process on a background thread inside an `autoreleasepool`. | If any of these selectors are missing from objc2-vision, fall back to a ~60-line Swift static library linked from `build.rs` (the brief allows bridges "only where no Rust binding exists"). Output JSON and the Vision→top-left flip stay identical. |
| `screen-access` / `request-screen-access` | `CGPreflightScreenCaptureAccess` / `CGRequestScreenCaptureAccess` (objc2-core-graphics), run in a **child process of our own binary** (`ss debug screen-access [--request]`) | macOS caches Screen Recording access per process; a fresh child sees a newly granted permission without restarting. This keeps that behaviour without shipping a second executable. |
| `screenshot-shortcuts [on|off]` | `system.rs`: `CFPreferencesCopyAppValue`/`SetAppValue`/`AppSynchronize` on `com.apple.symbolichotkeys` (objc2-core-foundation), then run `/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings -u` | Same IDs 28/29/30/31/184 and default parameters. Exposed as `ss debug screenshot-shortcuts [on|off]`. |

Result: **no Swift in the shipped app** unless the Vision fallback is needed.

---

## 4. Web-rendering dependencies (risks: GPUI has no browser engine)

| # | What depends on the browser | Severity | Plan |
|---|-----------------------------|----------|------|
| R1 | **Editor compositor** (`compose.ts`, `backgrounds.ts`): Canvas2D linear gradients at CSS angles, radial "blob" gradients with CSS `circle` radius semantics, rounded-rect clip, **drop shadow that follows the card's alpha** (`shadowBlur`/`shadowOffsetY`), high-quality downscale, `canvas.toBlob('image/png')`. | High | Port to **`tiny-skia`** (pure Rust: gradients, paths, clipping, PNG) plus a hand-written separable Gaussian blur for the shadow (Canvas `shadowBlur` = 2σ). Pure functions → pixel-level unit tests (layout numbers; edge colour; gradient endpoints; shadow alpha non-zero below the card). The on-screen preview renders at `min(1, displayScale × scaleFactor)` on the background executor, latest-request-wins, shown as `RenderImage`. Visual parity check: export the same capture + style from both apps and diff (`ss debug compose`). The alternative, CoreGraphics (`CGContextSetShadowWithColor`), is closer to Canvas but macOS-only and harder to test; I'd keep it as a fallback if the blur differs visibly. |
| R2 | **Background swatches** use CSS gradients generated from the same spec. | Low | Render each swatch with the same compositor into a small `RenderImage` (cached per custom colour). This makes the "swatches always match" guarantee structural. The transparency checkerboard is drawn with `canvas()` quads. |
| R3 | **OCR text layer**: invisible `<span>`s with `scaleX` fitting, native browser **text selection**, `::selection` styling, highlight mode, `selectedText()` that joins **whole words** per line. | High | Custom element: hit-test word boxes (normalised → image px → stage px), mouse-drag selection over the reading-order sequence (line, word), selection rects painted as accent-tinted quads, I-beam cursor over words, highlight mode paints all boxes. `selectedText()` already returns whole words for partly selected spans, so **word-granular selection gives the same copied text**. Double-click selects a word, triple-click a line (browser parity). The pure model (`text_selection.rs`) is unit-tested. |
| R4 | **Drag a file out of the preview** (`webContents.startDrag`). | High | GPUI has no external drag source. On mouse-drag past a threshold in the preview, call into AppKit: get the `NSView` from the window (`raw-window-handle`), `beginDraggingSessionWithItems:event:source:` with an `NSDraggingItem` (`NSURL` file) and the 160 px thumbnail as the image; the source is a `define_class!` implementing `NSDraggingSource` (`draggingSession:sourceOperationMaskForDraggingContext:` → copy). Needs the current `NSEvent` (`NSApp.currentEvent`). Spike this early (milestone 5). |
| R5 | **Animations**: slide-in/out (spring cubic-bezier), rise-in, fade-in, toast, shimmer, slider thumb scale, card hover scale, switch knob spring. | Medium | `gpui::Animation` + `with_animation` with custom easing fns (`cubic_bezier(0.22,1,0.36,1)`, spring `(0.34,1.36,0.64,1)`) in `desktop_ui.rs`. The preview's slide is an offset inside the transparent window. Shimmer is an animated gradient position (GPUI `linear_gradient`) or an animated opacity if gradient animation isn't available. |
| R6 | **`backdrop-blur`** on the preview chip, hover buttons and region hint card. | Low | GPUI has no per-element backdrop blur. Use slightly more opaque fills (e.g. `bg-black/55` → `/65`, `white/90` → `/96`). **Intentional visual change.** |
| R10 | `<input type=range>` (keyboard-adjustable), `<input type=color>`, `<kbd>`, `:focus-visible` rings, Tab focus order, screen-reader semantics. | Medium | `desktop_ui::slider` (drag + arrow keys when focused), colour picker = `NSColorPanel` via objc2 (continuous updates through a target/action class), focus rings via GPUI `FocusHandle` + `tab_index`. **GPUI accessibility (VoiceOver) is far weaker than Chromium's**; this is a regression I can't fully close. Listed in §7. |
| R11 | Fonts: `-apple-system`, SF Mono, `tabular-nums`, `text-[10.5px]` etc. | Low | `.SystemUIFont`, `SF Mono`/`Menlo`, `FontFeatures` `tnum`. |
| R12 | App icon `<img>` in onboarding, SVG icons (check, ✕, play, display). | Low | Embedded asset via GPUI `AssetSource` for the icon. SF Symbols via `gpui-symbols` for the glyphs (checkmark, xmark, play.fill, display). |

No embedded web pages, markdown, or charts exist in the app.

### Other platform risks

| # | Risk | Plan |
|---|------|------|
| R7 | GPUI's quit path gives `on_app_quit` futures only a short timeout; a recording needs up to ~5 s to finalise. | Handle quit ourselves: the tray "Quit ss" and ⌘Q call `app::quit(cx)`, which stops the recorder synchronously (bounded) and then `cx.quit()`. Also handle `applicationShouldTerminate` paths (logout/shutdown) if GPUI exposes them; otherwise document it. |
| R8 | Coordinate systems: Cocoa (bottom-left, per-screen) vs Electron/`screencapture -R` (top-left global points) vs GPUI window-local points. | One `displays.rs` with pure conversions and tests for multi-display layouts (secondary display above/left of primary, negative origins). |
| R9 | GPUI window flavours: non-activating floating panel (preview), screen-saver-level overlay (region), all-Spaces collection behaviour. | `WindowKind::PopUp` / `WindowOptions { focus: false, show: true, … }`, then set `level`, `collectionBehavior`, `ignoresMouseEvents=false`, `hasShadow=false` on the underlying `NSWindow` via objc2. Confirm GPUI's mac window flags against HEX and Zed's `gpui/src/platform/mac/window.rs` before coding. |
| R13 | `screencapture -i` blocks for as long as the user is selecting. | Always run on the background executor; the busy flag prevents concurrent captures. |

---

## 5. Target architecture

Single binary crate at the **repo root** (like HEX). `apps/desktop` stays untouched until parity, then
gets deleted along with the pnpm/Vite config (separate commit).

```
Cargo.toml  build.rs (only if a Swift/Carbon link stub is needed)
resources/  icon.png  trayTemplate.png  trayTemplate@2x.png  Info.plist.in  ss.entitlements
scripts/    build-app.sh  release-app.sh  macos-signing.sh  validate-app.sh
src/
  main.rs            clap: `app` (default), `status`, `debug {ocr, screen-access, screenshot-shortcuts, compose, migrate}`
  logging.rs         tracing-subscriber: stderr + <support>/logs/ss.log
  paths.rs           data/captures/settings/default dirs
  settings.rs        model, serde defaults, repair, load/save, SettingsModel entity
  migrate.rs         one-time Electron migration (§6)
  accelerator.rs     parse/format/validate Electron accelerator strings, keystroke → accelerator, → Carbon keycode+mods
  hotkeys.rs         Carbon global hotkeys
  capture_store.rs   ids, history scan, prune(60), timestamp names
  screencapture.rs   `screencapture` wrapper, rect arg, permission-error detection, recording child
  recorder.rs        recording state machine
  ocr.rs             Vision, reading-order sort, on-disk cache, in-memory dedupe
  compose.rs         layout + tiny-skia renderer + edge colour + PNG export
  backgrounds.rs     gradient specs
  text_selection.rs  OCR selection model
  clipboard.rs       NSPasteboard PNG / file URL, arboard text
  system.rs          permission probe, symbolic hotkeys, permission host, setup state
  login_item.rs      SMAppService
  displays.rs        NSScreen + coordinate conversion
  notify.rs          UNUserNotificationCenter
  updater.rs         Sparkle loader
  tray.rs            NSStatusItem + NSMenu
  actions.rs         what shortcuts/tray/windows trigger (old Actions service)
  desktop_ui.rs      tokens + controls
  windows/{preview,editor,settings,onboarding,region}.rs
```

`desktop_ui.rs` tokens come from `index.css`: canvas `#f7f5f2`, stage `#efece8`, panel `#fbfaf8`, line
`rgb(28 25 23 / .07)`, ink `#1c1b1a`, ink-soft `#57534e`, muted `#9a948d`, accent `#7c5cff`, accent-soft
`/0.12`, easings, radii (lg 8, xl 12, preview 14, swatch 10), type scale (10.5–22 px). Controls: `button`
(ghost/primary + shortcut hint), `kbd`, `key_caps(md/lg)`, `slider`, `segmented`, `switch`,
`status_pill`, `section`, `card`, `toast`, `titlebar_drag_region`, `window_options(kind)`.

Core modules never import `gpui`, except `settings.rs`'s entity wrapper, which is kept separate from the
pure model.

---

## 6. Data migration

Location stays `~/Library/Application Support/ss`.

- **settings.json**: read in place. The Rust `Settings` struct uses the same field names, the same
  defaults, and the same 2-space pretty JSON, so the Electron build can still read the file if you roll
  back. `repair_shortcuts` runs on every load, as today.
- **captures/**: read in place. Ids (`<base36 ms>-<3 base36>`), `.png` and `.ocr.json` layout are
  unchanged, so "Edit last capture" and the OCR cache carry over.
- **One-time step** (runs when `.migrated-from-electron` is absent): validate settings, then delete the
  known Chromium artefacts (`Cache`, `Code Cache`, `GPUCache`, `GPUPersistentCache`, `Dawn*Cache`,
  `GraphiteDawnCache`, `Local Storage`, `Session Storage`, `blob_storage`, `Shared Dictionary`,
  `Cookies*`, `DIPS`, `Trust Tokens*`, `Local State`, `Network Persistent State`, `Preferences`,
  `declarative_performance_observer.db*`) by exact name only, then write the marker. `ss debug migrate
  --dry-run` lists what it would do. Unit-tested against a temp dir.
- Login item: an Electron-registered login item belongs to the old binary. On first launch, if the old
  item is detected via `SMAppService.mainAppService.status`, re-register (same bundle ID, D2).

---

## 7. Intentional behaviour changes

1. Minimum macOS 15, Apple silicon only (was macOS 13+, arm64 build). *(D1)*
2. Signed with Developer ID, Hardened Runtime, notarized. Screen Recording must be granted once more
   after upgrading.
3. **New:** Sparkle auto-updates. Adds a "Check for Updates…" tray item and network access to the appcast URL.
4. **New:** CLI (`ss status`, `ss preview <window>`, `ss debug …`) and a log file at
   `~/Library/Application Support/ss/logs/ss.log`.
5. Settings decoding is per-field lenient: an invalid field falls back to its default. Electron's Schema
   reset **all** settings to defaults if any field was invalid.
6. No `backdrop-blur`: translucent chips use more opaque fills. *(R6)*
7. OCR text selection is word-granular visually. The copied text is the same as today. *(R3)*
8. Clipboard file reference for recordings is written via NSPasteboard rather than `osascript`, with the same result.
9. Accessibility: VoiceOver support is reduced compared with Chromium. Keyboard focus and Tab order are
   kept where GPUI supports them. *(R10)*
10. One-time cleanup of Chromium cache folders in the data directory. *(§6)*
11. `ss-helper` (Swift) is removed. Its functions move into the main binary. *(§3.4)*

Everything else (shortcut defaults and presets, accelerator rules, file names and locations, pruning,
afterCapture modes, window sizes, timings, and copy) is meant to stay identical.

---

## 8. HEX conventions to adopt

Studied HEX at `104a8ac` (2026-10-07). This section lists what we copy, and the cases where ss deliberately differs.

**Crate setup**
- `edition = "2024"`, one binary crate. Every macOS dependency sits under `[target.'cfg(target_os = "macos")'.dependencies]`, as in HEX, even though D1 makes macOS the only target.
- Pinned versions, taken from HEX:
  - `gpui = { version = "0.2.2", default-features = false, features = ["font-kit"] }` from crates.io, plus `gpui = { …, features = ["test-support"] }` as a dev-dependency.
  - `gpui-symbols = { version = "0.6.1", default-features = false, features = ["component", "cache"] }`
  - `objc2 0.6.4`, `objc2-app-kit 0.3.2`, `objc2-foundation 0.3.2`, `objc2-core-foundation 0.3.2`, `objc2-service-management 0.3.2` (`SMAppService`), `block2 0.6.2`
  - `arboard 3.6.1` (no default features), `clap 4.6.7` (derive), `color-eyre 0.6.5`, `dirs 7.0.0`, `fs2 0.4.3`, `libc 0.2`, `libloading 0.9.0`, `serde 1.0.229`, `serde_json 1.0.151`, `tracing 0.1.44`, `tracing-subscriber 0.3.23` (env-filter)
  - Any other `objc2-*` framework crates use the same 0.3.2 line.
- `MACOSX_DEPLOYMENT_TARGET=15.0`; arm64 only.

**AGENTS.md**
- Create an `AGENTS.md` for ss, modelled on HEX's:
  - architecture list (one line per module)
  - "keep modules deep; add a seam only when a second real adapter exists"
  - every window uses the `desktop_ui` scaffold, with no ad-hoc headers, rows or widths
  - `.truncate()` always pairs with `.overflow_x_hidden()`, and text beside a fixed column gets `flex_1` + zero min width
  - one direct line of copy per control
  - commands: `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `cargo test`, `git diff --check`
  - release identity invariants
- Skip HEX's `docs/features` tree. The plan and README cover ss's size.

**desktop_ui.rs**
- Same shape as HEX:
  - `u32` hex tokens used with `rgb()`, plus size constants
  - `pub(crate)` builder functions that return `Div`/`AnyElement`
  - a spring-driven `toggle(position)` and `mix_color`
  - `segmented_control` / `sliding_segmented_control`
  - `hotkey_keycaps`, `settings_row`, `settings_panel`, `section_label`
  - layout tests with `#[gpui::test]` + `debug_selector`
- ss keeps its **own light palette** (§5), not HEX's dark one.
- Icons: `gpui_symbols::Icon::new("checkmark")…`.

**Windows**
- Normal windows: `TitlebarOptions { appears_transparent: true, traffic_light_position: Some(point(px(18.), px(18.))), .. }`.
- Root element is `.track_focus(..)`, with `.on_action(|_: &CloseWindow, window, _| window.remove_window())`.
- Single-instance windows are tracked as `Option<WindowHandle<T>>`. Open-or-focus calls `handle.update(cx, |_, w, _| w.activate_window())`, then `cx.activate(true)`.
- `actions!(ss, [CloseWindow, QuitApplication, HideApplication, …])`, bound with `cx.bind_keys([KeyBinding::new("cmd-w", CloseWindow, None), "cmd-q", "cmd-h", …])`. This replaces Electron's implicit menu.
- The preview follows HEX's popup:

  ```rust
  WindowOptions {
      titlebar: None,
      window_background: WindowBackgroundAppearance::Transparent,
      focus: false,
      show: true,
      kind: WindowKind::PopUp,
      is_movable: false,
      is_resizable: false,
      ..
  }
  ```

  It is placed via `display_id`, then gets an objc2 pass for `collectionBehavior` (all Spaces + FullScreenAuxiliary) and `hidesOnDeactivate(false)`.
- The region overlay uses the same popup, plus `setLevel(NSScreenSaverWindowLevel)` and `focus: true`.

**Activation policy**
- ss stays **Accessory at all times**, as the Electron app does today.
- HEX switches to Regular while its window is open; we don't. That would add a Dock icon whenever Settings or the editor is open, which ss never does.

**Tray**
- Copy HEX `status_item.rs`:
  - a `define_class!` `StatusItemTarget` with a `SyncSender<TrayAction>` ivar and one `#[unsafe(method(...:))]` per menu item
  - a `NSStatusBar::systemStatusBar().statusItemWithLength(-2.0)` status item, held in a `thread_local!`
  - a GPUI async loop drains the channel (`try_recv` + `Timer::after(16ms)`)
- Differences from HEX:
  - ss uses its own template PNGs (via `NSImage initWithData`), not an SF Symbol.
  - The ticking title.
  - Left click toggles stop-recording, so the menu isn't permanently attached with `setMenu`.

**Global hotkeys: we diverge.**
- HEX uses `CGEventTap`, which needs Input Monitoring/Accessibility permission. ss only needs fixed chords.
- So `hotkeys.rs` uses Carbon `RegisterEventHotKey` through a small `#[link(name = "Carbon", kind = "framework")] extern "C"` block. It needs **no permission**, it is what Electron uses (same "⌥-only never fires" and "in use elsewhere" semantics), and it keeps onboarding at one permission.

**Launch at login**
- Copy HEX `login_item.rs`: `SMAppService::mainAppService()` called on a worker thread inside `autoreleasepool`.
  - `NotRegistered` and `NotFound` count as disabled.
  - `RequiresApproval` shows a link to `openSystemSettingsLoginItems()`.
- The UI polls OS state rather than persisting a boolean (this matches ss today).

**Sparkle**
- Copy HEX `sparkle.rs`:
  - `libloading` opens `exe_dir/../Frameworks/Sparkle.framework/Versions/B/Sparkle`; if it is absent (CLI/dev), skip silently.
  - `AnyClass::get(c"SPUStandardUpdaterController")`, then `initWithStartingUpdater:NO updaterDelegate: userDriverDelegate:nil`, `startUpdater:` and `checkForUpdatesInBackground`.
  - A manual check activates the app, then calls `checkForUpdates:`.
  - Honour `DisableUpdates`.
- `scripts/setup-sparkle.sh` pins Sparkle 2.9.4 with a sha256 check, as HEX does.

**Settings, paths, logging**
- As HEX:
  - serde with `#[serde(default)]`
  - atomic write (temp file + `rename`)
  - single instance via `fs2::try_lock_exclusive` on `<support>/ss.lock`
  - `color_eyre::install()` first
  - `tracing_subscriber::fmt().with_env_filter(RUST_LOG or "ss=info").with_writer(stderr.and(Mutex<File>))`
  - log at **`~/Library/Application Support/ss/logs/ss.log`** (HEX's location pattern, not `~/Library/Logs`)
- Optional `SS_APPLICATION_SUPPORT_DIR` override, used by tests and `capture-preview.sh`.

**CLI**
- clap derive. With no subcommand, the default is `app`, so Finder launches work.
- Subcommands: `app`, `status`, `preview <target>` (opens one window with fixture data for UI iteration, like HEX's), `debug …`.
- Debug-only subcommands are gated by `cfg!(debug_assertions)`, as with HEX's `DEVELOPER_FEATURES_ENABLED`. The exception is the permission probe the app spawns itself, which is always compiled in (hidden).

**Native bridges**
- If any are needed (the Vision fallback, R4 if objc2 is awkward), follow HEX `build.rs`:
  - `xcrun swiftc -parse-as-library -emit-object -target arm64-apple-macosx15.0` linked with `cargo:rustc-link-arg`
  - or `cc::Build` with `-fobjc-arc` for `.m`
- HEX's `native/permission_guide.m` `HexDragView` (`NSDraggingSource` + `beginDraggingSessionWithItems:event:source:` with an `NSURL` writer, 3 pt threshold, `NSDragOperationCopy`) is the reference for R4. I'll port it to objc2 `define_class!` first and fall back to `.m` only if that fails.

**Images**
- HEX only uses `img(Arc<gpui::Image>)` from PNG bytes. It has no `RenderImage`.
- For the compositor preview I'll check Zed's `ImageSource::Render(Arc<RenderImage>)` in gpui 0.2.2. The fallback is to encode PNG and use `Image::from_bytes`, which is slower: that costs one PNG encode per slider frame at reduced scale.

**Scripts**
- Modelled on HEX: `setup-sparkle.sh`, `macos-signing.sh`, `build-app.sh`, `validate-app.sh`, `release-app.sh [prepare|publish]`, `install-app.sh`, `capture-preview.sh`.
- Environment variables, renamed from HEX's: `SS_TEAM_ID` (required), `SS_CODESIGN_IDENTITY` (optional override), `SS_NOTARY_PROFILE` (notarytool keychain profile), `SS_VERSION` / `SS_BUILD_NUMBER` (default from Cargo metadata, major·10000+minor·100+patch), `SS_RELEASE_BASE_URL`, `SS_RELEASE_NOTES`. The Sparkle EdDSA key stays in the login keychain (`generate_appcast --ed-key-file -`).
- `build-app.sh`:
  - inside-out `codesign --force --timestamp --options runtime`: Sparkle XPC services → Autoupdate → Updater.app → framework → app
  - icon via `sips` + `iconutil` from `resources/icon.png`; we have no Icon Composer file
- Entitlements: none needed beyond Hardened Runtime. Sparkle is re-signed with our team, so library validation passes, and the old `osascript` is gone, so we need no Apple Events. `validate-app.sh` asserts:
  - bundle ID, versions, `LSUIElement`, `LSMinimumSystemVersion`, `SUFeedURL`/`SUPublicEDKey`, arm64-only (`lipo`)
  - `codesign --verify --deep --strict`, TeamIdentifier, Developer ID authority
  - no sandbox
- `release-app.sh` guards:
  - clean tree, HEAD pushed
  - build number greater than the live appcast's `sparkle:version`
  - notarize + staple both the app and the DMG (APFS ULFO, `/Applications` symlink), `spctl --assess`
  - Sparkle ZIP via `ditto -c -k --keepParent`
  - `generate_appcast`, then check for `sparkle:edSignature`
  - publish artifacts first, feed last

---

## 9. Order of work (each milestone ends with `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`)

| M | Milestone | Core (tested) | UI | Done when |
|---|-----------|---------------|----|-----------|
| 1 | **Skeleton** | `Cargo.toml`, `AGENTS.md`, `paths`, `settings` (serde, defaults, repair, atomic write, round-trip with the real file), `logging`, `main` (clap) | GPUI app, accessory policy, single-instance lock, an empty Settings window built with `desktop_ui` | `cargo run -- app` opens a window; `cargo run -- status` prints paths and settings |
| 2 | **Migration + accelerators + hotkeys** | `migrate`, `accelerator` (parse/format/validate/keycodes; tests ported from `isUsableAccelerator`/`acceleratorParts`/`acceleratorFromEvent`) | `hotkeys` registering from settings, with pause | pressing ⌃⇧4 logs the action |
| 3 | **Tray + capture-to-clipboard** | `capture_store` (ids, scan, prune, timestamp), `screencapture` (args, permission regex), `clipboard`, `displays` (conversion tests), `actions` capture paths | `tray` with the full menu; "Just copy" mode works end to end | ⇧⌘4-style capture lands on the clipboard as public.png; history pruned to 60 |
| 4 | **System + Settings + Onboarding** | `system` (probe child, symbolic hotkeys, permission host), `login_item`, `notify` | full Settings and Onboarding windows, shortcut recorder, directory picker, focus refresh, 1.5 s polling | both windows match the Electron screens side by side |
| 5 | **OCR + Preview** | `ocr` (Vision, sort, cache, dedupe; sort tests), thumbnails | preview window: placement, animations, auto-hide/hover, chip, Edit/Copy text/Show in Finder, **drag-out spike (R4)** | afterCapture=preview behaves as today, including drag into Finder/Slack |
| 6 | **Editor** | `backgrounds`, `compose` (layout tests, edge colour, pixel tests, PNG export), `text_selection` | editor window, sidebar controls, swatches, NSColorPanel, OCR layer, toasts, keys, debounced style persist, save/save-as/copy, multi-editor | an exported PNG matches the Electron export closely for 3 reference captures × 4 styles |
| 7 | **Recording** | `recorder` (state machine, SIGINT/SIGTERM escalation, tested with a fake child), video thumbnail | region overlay, tray timer, stop-by-click/shortcut/quit | record area/screen → `.mov` in `~/Movies/ss`, file on clipboard, preview with play glyph |
| 8 | **Updates** | `updater` (dlopen `Sparkle.framework` from `Contents/Frameworks`, `SPUStandardUpdaterController` via objc2 class lookup; no-op when unbundled) | "Check for Updates…" tray item | a test appcast offers an update |
| 9 | **Packaging** | — | — | `build-app.sh` → `.app` (Info.plist: `dev.sztuczka.ss`, `LSUIElement`, `LSMinimumSystemVersion 15.0`, `SUFeedURL`, `SUPublicEDKey`, `NSScreenCaptureUsageDescription`), Developer ID + Hardened Runtime signing (entitlements kept minimal), notarize + staple, signed DMG + Sparkle ZIP, Ed25519-signed `appcast.xml`; `validate-app.sh` checks the bundle ID, version and `codesign --verify --deep --strict` + `spctl -a`. Team ID / notary profile / Sparkle key read from env vars. |
| 10 | **Cut-over** | — | — | remove `apps/desktop`, pnpm/Vite config; update README |

New crates beyond the brief's list:
- Already used in HEX: `fs2`, `libc`, `libloading`, `block2`, `objc2-core-foundation`.
- New to us, each tied to a specific item above:
  - `image` (PNG decode/resize)
  - `tiny-skia` (compositor, R1)
  - `objc2-vision` (OCR)
  - `objc2-core-graphics` (permission checks)
  - `objc2-quick-look-thumbnailing` (video poster)
  - `objc2-user-notifications`
  - `fastrand` (capture id suffix)
  - `jiff` (local-time file names)
  - `futures` (oneshot/Shared)
  - `raw-window-handle` (R4)
- Tests only: `tempfile`.
