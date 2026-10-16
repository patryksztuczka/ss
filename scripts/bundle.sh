#!/bin/sh
# Builds the release binary and packages it as an ad-hoc signed ss.app and a
# drag-to-Applications DMG. The version comes from Cargo.toml.
#
#   scripts/bundle.sh      # target/release/ss.app, target/release/ss-<version>-arm64.dmg
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$root"
export PATH="$HOME/.cargo/bin:$PATH"

version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
  echo "could not read the version from Cargo.toml" >&2
  exit 1
fi

cargo build --release --locked

out="$root/target/release"
app="$out/ss.app"
dmg="$out/ss-$version-arm64.dmg"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

rm -rf "$app" "$dmg"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$out/ss" "$app/Contents/MacOS/ss"

iconset="$work/icon.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" resources/icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" resources/icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/icon.icns"

cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>ss</string>
  <key>CFBundleExecutable</key>
  <string>ss</string>
  <key>CFBundleIconFile</key>
  <string>icon.icns</string>
  <key>CFBundleIdentifier</key>
  <string>dev.sztuczka.ss</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>ss</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>$version</string>
  <key>CFBundleVersion</key>
  <string>$version</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.productivity</string>
  <key>LSMinimumSystemVersion</key>
  <string>15.0</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
EOF
plutil -lint "$app/Contents/Info.plist" >/dev/null

# Ad-hoc: no Developer ID yet, so Gatekeeper asks once on first launch.
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"

stage="$work/dmg"
mkdir "$stage"
cp -R "$app" "$stage/ss.app"
ln -s /Applications "$stage/Applications"
hdiutil create -volname ss -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg" >/dev/null

echo "$app"
echo "$dmg"
