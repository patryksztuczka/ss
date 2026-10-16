// ss-helper: small native companion for things Electron can't do well.
//
//   ss-helper ocr <image>                     Vision text recognition → JSON
//   ss-helper screen-access                   "granted" | "denied" (never prompts)
//   ss-helper request-screen-access           shows the macOS prompt once, then prints the status
//   ss-helper screenshot-shortcuts            "enabled" | "disabled" (macOS ⇧⌘3/4/5)
//   ss-helper screenshot-shortcuts <on|off>   toggles them and applies without logging out
//
// Running these in a child process matters: macOS caches Screen Recording access per process,
// so a fresh helper sees a newly granted permission without restarting ss.
import CoreGraphics
import Foundation
import ImageIO
import Vision

func fail(_ message: String) -> Never {
  FileHandle.standardError.write(Data((message + "\n").utf8))
  exit(1)
}

func output(_ text: String) {
  FileHandle.standardOutput.write(Data(text.utf8))
}

// MARK: - OCR

struct Box: Encodable {
  let text: String
  let x: Double
  let y: Double
  let w: Double
  let h: Double
}

struct Line: Encodable {
  let text: String
  let x: Double
  let y: Double
  let w: Double
  let h: Double
  let words: [Box]
}

struct Layout: Encodable {
  let width: Int
  let height: Int
  let lines: [Line]
}

/// Vision uses a bottom-left origin; the renderer wants top-left.
func flip(_ rect: CGRect) -> (Double, Double, Double, Double) {
  (Double(rect.minX), Double(1 - rect.maxY), Double(rect.width), Double(rect.height))
}

func recognize(_ path: String) {
  let url = URL(fileURLWithPath: path)
  guard let source = CGImageSourceCreateWithURL(url as CFURL, nil),
    let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
  else { fail("cannot read image") }

  let request = VNRecognizeTextRequest()
  request.recognitionLevel = .accurate
  request.usesLanguageCorrection = true
  request.automaticallyDetectsLanguage = true

  do {
    try VNImageRequestHandler(cgImage: image, options: [:]).perform([request])
  } catch {
    fail("recognition failed: \(error.localizedDescription)")
  }

  var lines: [Line] = []
  for observation in request.results ?? [] {
    guard let candidate = observation.topCandidates(1).first else { continue }
    let text = candidate.string
    var words: [Box] = []
    text.enumerateSubstrings(in: text.startIndex..<text.endIndex, options: .byWords) { word, range, _, _ in
      guard let word, let box = try? candidate.boundingBox(for: range) else { return }
      let (x, y, w, h) = flip(box.boundingBox)
      words.append(Box(text: word, x: x, y: y, w: w, h: h))
    }
    let (x, y, w, h) = flip(observation.boundingBox)
    lines.append(Line(text: text, x: x, y: y, w: w, h: h, words: words))
  }

  guard let data = try? JSONEncoder().encode(Layout(width: image.width, height: image.height, lines: lines))
  else { fail("cannot encode output") }
  FileHandle.standardOutput.write(data)
}

// MARK: - macOS screenshot shortcuts (System Settings → Keyboard → Keyboard Shortcuts → Screenshots)

let hotkeysDomain = "com.apple.symbolichotkeys" as CFString
let hotkeysKey = "AppleSymbolicHotKeys" as CFString

/// Symbolic hotkey id → default (character, key code, modifier flags).
let screenshotHotkeys: [(id: Int, parameters: [Int])] = [
  (28, [51, 20, 1_179_648]),  // ⇧⌘3 save screen
  (29, [51, 20, 1_441_792]),  // ⌃⇧⌘3 copy screen
  (30, [52, 21, 1_179_648]),  // ⇧⌘4 save area
  (31, [52, 21, 1_441_792]),  // ⌃⇧⌘4 copy area
  (184, [53, 23, 1_179_648]),  // ⇧⌘5 screenshot & recording options
]

func readHotkeys() -> [String: Any] {
  CFPreferencesCopyAppValue(hotkeysKey, hotkeysDomain) as? [String: Any] ?? [:]
}

func screenshotShortcutsEnabled() -> Bool {
  let hotkeys = readHotkeys()
  // A missing entry means "default", which is enabled.
  return screenshotHotkeys.contains { hotkey in
    let entry = hotkeys[String(hotkey.id)] as? [String: Any]
    return (entry?["enabled"] as? Bool) ?? true
  }
}

func setScreenshotShortcuts(enabled: Bool) {
  var hotkeys = readHotkeys()
  for hotkey in screenshotHotkeys {
    var entry = hotkeys[String(hotkey.id)] as? [String: Any] ?? [:]
    entry["enabled"] = enabled
    if entry["value"] == nil {
      entry["value"] = ["parameters": hotkey.parameters, "type": "standard"] as [String: Any]
    }
    hotkeys[String(hotkey.id)] = entry
  }
  CFPreferencesSetAppValue(hotkeysKey, hotkeys as CFDictionary, hotkeysDomain)
  guard CFPreferencesAppSynchronize(hotkeysDomain) else { fail("cannot write keyboard preferences") }

  // Ask the system to reload keyboard shortcuts now instead of at next login.
  let activate = Process()
  activate.executableURL = URL(
    fileURLWithPath: "/System/Library/PrivateFrameworks/SystemAdministration.framework/Resources/activateSettings")
  activate.arguments = ["-u"]
  try? activate.run()
  activate.waitUntilExit()
}

// MARK: - Entry

let arguments = Array(CommandLine.arguments.dropFirst())
switch arguments.first {
case "ocr":
  guard arguments.count > 1 else { fail("usage: ss-helper ocr <image>") }
  recognize(arguments[1])
case "screen-access":
  output(CGPreflightScreenCaptureAccess() ? "granted" : "denied")
case "request-screen-access":
  output(CGRequestScreenCaptureAccess() ? "granted" : "denied")
case "screenshot-shortcuts":
  switch arguments.dropFirst().first {
  case "on": setScreenshotShortcuts(enabled: true)
  case "off": setScreenshotShortcuts(enabled: false)
  case nil: break
  default: fail("usage: ss-helper screenshot-shortcuts [on|off]")
  }
  output(screenshotShortcutsEnabled() ? "enabled" : "disabled")
default:
  fail("usage: ss-helper <ocr|screen-access|request-screen-access|screenshot-shortcuts>")
}
