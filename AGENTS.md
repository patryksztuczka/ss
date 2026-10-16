# ss Agent Guide

## Purpose

ss is a menu-bar-only macOS screenshot and screen-recording tool. This crate is
the native Rust + GPUI rewrite of the Electron app in `apps/desktop`, which stays
in the repo as the parity reference until cut-over.

Targets are macOS 15+ on Apple silicon only. Do not add Linux code, platform
traits or cross-platform seams. Updates (Sparkle) and release packaging are
deferred; do not add them until asked.

## Architecture

One binary crate at the repo root. Modules marked *planned* do not exist yet.

- `main`: clap CLI. `app` (default, so Finder launches work), `status`, `debug`.
- `paths`: the data folder (`~/Library/Application Support/ss`, or
  `SS_APPLICATION_SUPPORT_DIR`), `captures/`, `settings.json`, `logs/ss.log`,
  `ss.lock`, and the default `~/Pictures/ss` / `~/Movies/ss` folders.
- `logging`: tracing to stderr plus `logs/ss.log`; `RUST_LOG` or `ss=info`.
- `instance`: the exclusive `ss.lock`; a second app instance logs and exits 0.
- `settings`: the pure settings model ported from `shared/settings.ts` (serde,
  defaults, presets, `is_usable_accelerator`, `repair_shortcuts`, lenient
  per-field decoding, atomic save). `settings::model` is the only GPUI part:
  the shared `SettingsModel` entity that windows observe.
- `accelerator`: Electron accelerator strings: display formatting, parsing to
  Carbon key codes and NSMenu key equivalents, and keystroke → accelerator.
- `hotkeys`: Carbon `RegisterEventHotKey` global shortcuts with pause; presses
  are handed to GPUI through a channel.
- `capture_store`: capture ids, history rebuilt from `captures/*.png`,
  pruning to 60, timestamp names.
- `screencapture`: the `/usr/sbin/screencapture` wrapper and
  permission-error detection (blocking; background executor only).
- `displays`: NSScreen → top-left global points (Electron's `bounds` /
  `workArea`), display under the cursor.
- `clipboard`: exact PNG bytes as `public.png`, text via arboard, file URLs.
- `actions`: what shortcuts, the tray and windows trigger; one capture at a
  time.
- `tray`: the NSStatusItem and its menu (menu attached except while
  recording; macOS 26 only shows attached status-item menus).
- `app`: GPUI bootstrap, accessory activation policy, global actions and key
  bindings (⌘W, ⌘Q, ⌘H), the action channel, quit.
- `system`: Screen Recording probe (a fresh `ss internal screen-access`
  child), the macOS screenshot shortcuts in `com.apple.symbolichotkeys`,
  permission host, setup state. Blocking.
- `login_item`: `SMAppService.mainAppService`; bundled builds only.
- `notify`: silent `UNUserNotificationCenter` posts; unbundled runs only log.
- `setup`: what Settings and onboarding do to the system, behind the
  `SetupBackend` global (`SystemSetup`; `FakeSetup` for tests and previews).
- `ocr`: Vision text recognition (objc2-vision; `boundingBoxForRange:error:`
  is sent with `msg_send!` as it has no binding), the reading-order sort, the
  `captures/<id>.ocr.json` cache. In-memory dedupe lives in `actions`.
- `js_sort`: V8's `Array.prototype.sort` (TimSort), so OCR line order matches
  Electron with its non-transitive comparator.
- `thumbnail`: preview thumbnails and the drag image (`image` crate) as GPUI
  `RenderImage`s; video posters via QuickLook.
- `drag`: dragging a capture file out via an AppKit `NSDraggingSource`.
- `backgrounds`, `compose`: the editor compositor on tiny-skia (layout,
  gradients, rounded card, Gaussian-approximated shadow, edge colour, PNG
  export). Sources are converted from their ICC profile to sRGB (qcms), as
  Chromium does. Pure; checked against PNGs rendered by the Electron app's
  real `compose.ts` (`tests/fixtures/compose`).
- `text_selection`: word-granular selection over the OCR glyphs.
- `color_panel`: `NSColorPanel` for the Custom background.
- `desktop_ui`: the light-palette tokens, easings and shared controls.
- `windows::settings`, `windows::onboarding`, `windows::preview`,
  `windows::editor`: the Settings window, the setup guide, the floating
  capture preview and the editor (one window per capture).
- `preview`: `ss preview settings|onboarding`, fixture windows for UI work.
- `debug`: `ss debug …` tools, debug builds only.
- `recorder`: one recording at a time over `screencapture -v` (stdin held
  open; SIGINT, then SIGTERM), with a single finalisation path.
- `windows::region`: the record-area overlay.
- *planned*: `migrate` (Chromium-cache cleanup, deferred).

Keep modules deep. Callers should not coordinate file formats, child processes
or AppKit details. Add a seam only when a second real adapter exists or a
current test needs substitution.

Core modules never import `gpui`. The exception is `settings::model`, which
stays separate from the pure model in `settings`.

## Invariants

- The data folder is shared with the Electron build and read in place. Never
  delete anything in the real `~/Library/Application Support/ss`. Tests and
  development runs use temp dirs via `SS_APPLICATION_SUPPORT_DIR`. `status`
  and other read-only commands must not write to the data folder.
- `settings.json` keeps Electron's field names, defaults, key order and
  2-space pretty JSON (whole numbers without `.0`) so either app can read it.
  Decoding is lenient per field: an invalid or missing field falls back to its
  default. `repair_shortcuts` runs on every load and persists only when it
  changed something. Saves are atomic (temp file + rename).
- ss is an accessory app at all times: no Dock icon, even with a window open.
  Closing the last window keeps the process running; ⌘Q quits.
- Apart from deliberate behaviour changes, everything (shortcut defaults and
  presets, accelerator rules, file names and locations, pruning, afterCapture
  modes, window sizes, timings, and copy) must match the Electron app.
- Blocking work (child processes, OCR, image encode/decode, compositing) runs
  on GPUI's background executor, never on the main thread. Small settings
  writes may stay synchronous.
- Debug subcommands are gated by `cfg!(debug_assertions)`, except hidden
  internal commands the app spawns itself.
- Development must not change system state: never write the screenshot
  hotkeys, register login items, call `CGRequestScreenCaptureAccess` or open
  System Settings outside a deliberate manual test. Previews and tests use
  `FakeSetup`.
- Native callbacks (Carbon hotkeys, status-item targets) only send an
  `Action` into the channel; GPUI performs it on the foreground.

## UI conventions

- Every window builds from `desktop_ui`: `window_frame`, `titlebar`, `section`,
  `card` / `divided_card`, `settings_row` / `compact_row`, `button`, `kbd`,
  `key_caps`, `segmented`, `switch_toggle`, `status_pill`, `hint`. Do not add
  ad-hoc headers, rows, colours or widths in a window; extend `desktop_ui`.
- Use the ss light palette from `apps/desktop/src/renderer/src/index.css`
  (canvas, stage, panel, line, ink, ink-soft, muted, accent, accent-soft),
  not HEX's dark one. Port visuals from `components/ui.tsx`.
- Standard windows use `desktop_ui::window_options`: transparent titlebar,
  traffic lights at (18, 18), canvas background. The root element tracks a
  focus handle and handles `CloseWindow` with `window.remove_window()`.
- Single-instance windows are tracked in a GPUI global as
  `Option<WindowHandle<T>>`; open-or-focus activates the existing window.
- Any `.truncate()` element also sets `.overflow_x_hidden()` and `w_full`, and
  must not sit in a flex column: GPUI caches a nowrap text's first
  measurement, so an intrinsic-sizing pass leaves it as "…". Text beside a
  fixed control gets `flex_1` and `min_w_0` so it wraps instead of pushing the
  control.
- Don't use `ml_auto`/`mr_auto` in a row that sits in a flex column: GPUI's
  layout then drops that row's `gap`. Push items apart with a `flex_1` spacer.
- GPUI asserts eased deltas stay within 0..=1. Overshooting curves
  (`ease_spring`) are applied inside the animator, never via `with_easing`.
- GPUI stops drawing occluded windows; `ss preview` floats its window without
  activating so screenshots render.
- One direct line of copy per control. Reuse the Electron copy verbatim.
- Motion uses `desktop_ui::ease_out_soft` / `ease_spring` and the duration
  constants there.
- Layout behaviour that matters gets a `#[gpui::test]` with `debug_selector`.

## Commands

All cargo commands need `export PATH="$HOME/.cargo/bin:$PATH"`.

```sh
cargo run -- status
SS_APPLICATION_SUPPORT_DIR=$(mktemp -d) cargo run -- app
cargo run -- debug displays
cargo run -- debug capture-screen
cargo run -- debug parse-accelerator "Shift+Command+4"
cargo run -- debug setup-state
scripts/capture-preview.sh /tmp/ss-settings.png settings --scroll bottom
scripts/capture-preview.sh /tmp/ss-onboarding.png onboarding --step shortcuts --failed
SS_PREVIEW_MIN_WIDTH=200 scripts/capture-preview.sh /tmp/ss-preview.png preview --hover
cargo run -- debug ocr capture.png --compare capture.ocr.json
cargo run -- debug compose capture.png --out /tmp/export.png
cargo run -- debug record-screen --seconds 3 --out /tmp/test.mov   # delete afterwards
cargo run -- debug poster /tmp/test.mov --out /tmp/poster.png
scripts/capture-preview.sh /tmp/ss-editor.png editor --highlight
node scripts/fixtures/compose-reference/run.mjs   # regenerate compose references
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
git diff --check
```

Dependencies are optimised in the dev profile (`[profile.dev.package."*"]`):
image work is unusably slow unoptimised. The `gpui` dependency enables `runtime_shaders`, so building does not need
Xcode's Metal compiler; the Command Line Tools are enough.

## Release identity

- Bundle identifier `dev.sztuczka.ss`, product name `ss`, executable `ss`.
- `LSUIElement` (no Dock icon) and `LSMinimumSystemVersion` 15.0, arm64 only.
- The data folder name stays `ss`.
- Signing, notarization, DMG and Sparkle are deferred; do not add a release
  script until asked.
