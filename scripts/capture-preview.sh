#!/bin/sh
# Builds ss, opens one `ss preview` window with fixture data, captures only
# that window, then quits. The preview never takes focus and uses its own
# temp data folder.
#
#   scripts/capture-preview.sh OUTPUT.png settings [--scroll bottom]
#   scripts/capture-preview.sh OUTPUT.png onboarding --step shortcuts --failed
#   SS_PREVIEW_MIN_WIDTH=200 scripts/capture-preview.sh OUTPUT.png preview --hover
set -eu

if [ "$#" -lt 2 ]; then
  echo "usage: $0 OUTPUT TARGET [PREVIEW OPTIONS...]" >&2
  exit 2
fi

output=$1
shift
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"
if [ "${SS_PREVIEW_SKIP_BUILD:-0}" != "1" ]; then
  cargo build --quiet
fi

data=$(mktemp -d)
SS_APPLICATION_SUPPORT_DIR=$data "$root/target/debug/ss" preview "$@" &
pid=$!
cleanup() {
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -rf "$data"
}
trap cleanup EXIT INT TERM

window_id=$(swift - "$pid" "${SS_PREVIEW_MIN_WIDTH:-500}" <<'SWIFT'
import CoreGraphics
import Foundation

let pid = Int(CommandLine.arguments[1])!
let minimumWidth = CGFloat(Double(CommandLine.arguments[2])!)
for _ in 0..<200 {
    let windows = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as! [[String: Any]]
    let match = windows.first { window in
        let bounds = window[kCGWindowBounds as String] as? [String: CGFloat]
        return (window[kCGWindowOwnerPID as String] as? Int) == pid
            && (bounds?["Width"] ?? 0) >= minimumWidth
    }
    if let match {
        print(match[kCGWindowNumber as String] as! Int)
        exit(0)
    }
    Thread.sleep(forTimeInterval: 0.05)
}
exit(1)
SWIFT
)

# Let entrance animations and any scripted scrolling finish.
sleep "${SS_PREVIEW_SETTLE:-1}"
mkdir -p "$(dirname -- "$output")"
screencapture -x -o -l "$window_id" "$output"
echo "$output"
