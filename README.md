# ss

A small, shortcut-driven replacement for the macOS screenshot tool. It lives in the menu bar, has no Dock icon, and never writes to the Desktop.

- Every capture goes straight to the clipboard. A floating preview slides in: click it to edit, drag it into any app, or hover for **Copy text**.
- Capture an area or window (native macOS selection: drag, or press Space to pick a window), or the whole screen under the cursor.
- Record an area or the whole screen. While recording, the menu bar shows `● 0:42`. Click it, or press the shortcut again, to stop. The `.mov` file lands in `~/Movies/ss` and a reference to it goes on the clipboard.
- The editor adds a background (presets, transparent, or a custom color), padding, an inset filled with the screenshot's edge color, corner radius, shadow, and aspect ratio. The look you pick is remembered for next time.
- Text in screenshots is selectable. On-device OCR (Apple Vision) puts an invisible text layer over the image, so you select words with the mouse and press ⌘C.

## Setup guide

On first launch (and any time from the menu bar → **Setup Guide…**), ss walks through:

1. **Screen Recording permission.** One click shows the macOS prompt and opens System Settings, and the guide updates by itself once you switch ss on. No restart needed: captures run in child processes, which see the new permission straight away.
2. **Shortcuts.** Either **replace the macOS ones** (ss switches off the system ⇧⌘3/4/5 shortcuts for you and claims them), or **keep them** and use ⌃⇧3/4/5. You can switch either way later in Settings → System.
3. **Launch at login** (installed app only).

## Shortcuts

| Action                | Replace macOS | Keep macOS (default) |
| --------------------- | ------------- | -------------------- |
| Capture area / window | ⇧⌘4           | ⌃⇧4                  |
| Capture screen        | ⇧⌘3           | ⌃⇧3                  |
| Record area · stop    | ⇧⌘5           | ⌃⇧5                  |
| Record screen · stop  | ⌥⇧⌘5          | ⌃⌥⇧5                 |
| Edit last capture     | ⌥⇧⌘4          | ⌃⌥⇧4                 |

Every shortcut has to include ⌘ or ⌃. Since macOS 15, global shortcuts that use only ⌥ / ⌥⇧ register without error but never fire, so the Settings recorder rejects them.

| Editor                         |                                    |
| ------------------------------ | ---------------------------------- |
| ⌘C                             | Copy the image, or selected text   |
| ⇧⌘C                            | Copy all recognized text           |
| ⌘S / ⇧⌘S                       | Save to `~/Pictures/ss` / Save As… |
| 1 – 0                          | Background presets                 |
| R                              | Cycle aspect ratio                 |
| [ / ]                          | Less / more padding                |
| H                              | Highlight recognized text          |
| Esc, ⌘W, double-click backdrop | Close (double-click also copies)   |

## Where files go

- Screenshot history: `~/Library/Application Support/ss/captures` (the last 60 are kept, so "edit last capture" survives restarts)
- ⌘S in the editor: `~/Pictures/ss`
- Recordings: `~/Movies/ss`

Both folders can be changed in Settings.

## Development

Requires macOS, Node ≥ 22, pnpm, and the Xcode Command Line Tools (`swiftc` builds the OCR helper).

```sh
pnpm install
pnpm dev         # renderer dev server + Electron
pnpm typecheck   # TypeScript 7
pnpm lint
pnpm dist        # unsigned .dmg/.zip in apps/desktop/release
```

macOS asks for **Screen & System Audio Recording** permission on the first capture. In development, the terminal running `pnpm dev` is the app that needs it.

## Native app (Rust, in progress)

A native rewrite in Rust + [GPUI](https://www.gpui.rs) lives at the repo root (`Cargo.toml`,
`src/`). It reads the same data folder and settings as the Electron app. The plan, status and
known gaps are in [`docs/rewrite-plan.md`](docs/rewrite-plan.md); the hands-on checklist is
[`docs/manual-test-checklist.md`](docs/manual-test-checklist.md). Contributor rules are in
[`AGENTS.md`](AGENTS.md).

Requires Rust stable (`~/.cargo/bin` on `PATH`) and the Xcode Command Line Tools; GPUI compiles
its Metal shaders at runtime, so full Xcode isn't needed.

```sh
# Develop against a scratch data folder, never your real one:
SS_APPLICATION_SUPPORT_DIR=$(mktemp -d) cargo run -- app

cargo run -- status        # data folder, settings, capture count (read-only)

# Debug builds only: one window with fixture data, for UI work and screenshots
cargo run -- preview settings --scroll bottom
cargo run -- preview onboarding --step shortcuts
cargo run -- preview preview --hover
cargo run -- preview editor --highlight
cargo run -- preview region --drag 380,260,640,360
scripts/capture-preview.sh /tmp/settings.png settings

# Debug builds only: headless tools
cargo run -- debug displays
cargo run -- debug setup-state
cargo run -- debug ocr capture.png --compare capture.ocr.json
cargo run -- debug compose capture.png --out /tmp/export.png
cargo run -- debug record-screen --seconds 3 --out /tmp/test.mov

cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

`SS_APPLICATION_SUPPORT_DIR` points the app at another data folder; use it for every `app` run
during development. `status` and the `debug` commands only read the real folder.

## How it's built

- `apps/desktop/src/main`: Electron main process, written with Effect 4 services and layers (`Screencapture`, `Recorder`, `Ocr`, `CaptureStore`, `SettingsStore`, `Clipboard`, `Windows`, `Shortcuts`, `Actions`, `Tray`, `Ipc`). Pixels come from macOS's own `screencapture` binary, which gives the native selection UI and hardware-encoded recordings.
- `apps/desktop/src/shared`: Effect `Schema` definitions for settings, OCR output, and the IPC contract. Both processes validate every message against them.
- `apps/desktop/src/renderer`: React 19 + Tailwind 4. The floating preview, editor (canvas compositor + OCR text layer), recording-region overlay, and settings.
- `apps/desktop/native/helper.swift`: a small native CLI (`ss-helper`), compiled at build time. It handles Vision OCR, fresh Screen Recording checks and prompts, and toggling the macOS screenshot shortcuts.
