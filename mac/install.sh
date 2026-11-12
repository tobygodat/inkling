#!/bin/bash
# Builds Inkling.app for the Mac in release, signs it with the Developer ID identity and installs it
# in /Applications. It never opens the app. Usage: install.sh [destination folder]
#
# The build number is the commit count. It goes into the binary (INKLING_BUILD, read by
# core/build.rs) and into CFBundleVersion, which the running app watches to offer Restart
# (mac/src/main.rs reads the exact CFBundleVersion line written below).
set -euo pipefail
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cd "$(dirname "$0")/.."
root="$(pwd)"
dest="${1:-/Applications}"
identity="Developer ID Application: Toby Godat (Q6NAWA42Y2)"
version="$(cargo metadata --no-deps --format-version 1 | python3 -I -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="inkling-core"))')"
build="$(git rev-list --count HEAD)"

INKLING_BUILD="$build" cargo build --release -p inkling-mac
app="$root/target/release/Inkling.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/inkling "$app/Contents/MacOS/Inkling"
cp icon/Inkling.icns "$app/Contents/Resources/Inkling.icns"
cat >"$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>com.tobygodat.inkling</string>
  <key>CFBundleName</key><string>Inkling</string>
  <key>CFBundleDisplayName</key><string>Inkling</string>
  <key>CFBundleExecutable</key><string>Inkling</string>
  <key>CFBundleIconFile</key><string>Inkling</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$build</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.productivity</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSSupportsAutomaticGraphicsSwitching</key><true/>
</dict>
</plist>
EOF
codesign --force --options runtime --timestamp --sign "$identity" "$app"
codesign --verify --strict --deep "$app"

# Staged, so the running app's bundle is never half-copied and is missing only between the two
# renames. A crash leaves at most a .new or .old, which the next run removes first.
installed="$dest/Inkling.app"
rm -rf "$installed.new" "$installed.old"
ditto "$app" "$installed.new"
[ ! -e "$installed" ] || mv "$installed" "$installed.old"
mv "$installed.new" "$installed"
rm -rf "$installed.old"
echo "installed $dest/Inkling.app $version ($build)"
