#!/bin/bash
# Archives Inkling for a real iPhone in Release, signs it for the App Store with Xcode's signed-in
# account (team Q6NAWA42Y2) and uploads it to TestFlight. Usage: testflight.sh [build number]
# The build number defaults to the UTC time, so each upload is higher than the last.
# Needs the app record for com.tobygodat.inkling in App Store Connect.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"
build="${1:-$(date -u +%Y%m%d.%H%M)}"
archive="$root/target/Inkling.xcarchive"
export_dir="$root/target/testflight"

./fetch-mobile.sh
xcodegen generate --spec app/project.yml --project app --quiet
rm -rf "$archive" "$export_dir"
mkdir -p "$export_dir"
xcodebuild -project app/Inkling.xcodeproj -scheme Inkling -configuration Release \
  -destination 'generic/platform=iOS' -archivePath "$archive" -derivedDataPath "$root/target/xcode-release" \
  -allowProvisioningUpdates CURRENT_PROJECT_VERSION="$build" -quiet archive

# App Store Connect rejects private API use (ITMS-90338). Refuse to upload when the binary
# imports a framework C symbol that no public iPhoneOS SDK header declares.
sdk="$(xcrun --sdk iphoneos --show-sdk-path)"
find "$sdk/System/Library/Frameworks" -name '*.h' >"$export_dir/headers.txt"
private=()
while read -r sym; do
  xargs grep -lw -m1 "$sym" <"$export_dir/headers.txt" >/dev/null 2>&1 || private+=("$sym")
done < <(nm -u "$archive/Products/Applications/Inkling.app/Inkling" | sed 's/^_//' \
  | grep -E '^(k?CG|k?CT|k?CF|CA|UI|NS|MTL|k?Sec|SC|AV|CM|Audio)' | sort -u)
if [ ${#private[@]} -gt 0 ]; then
  echo "error: not in any public SDK header, so App Store Connect would reject the build:" >&2
  printf '  %s\n' "${private[@]}" >&2
  exit 1
fi

cat >"$export_dir/ExportOptions.plist" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>app-store-connect</string>
  <key>destination</key><string>upload</string>
  <key>teamID</key><string>Q6NAWA42Y2</string>
  <key>signingStyle</key><string>automatic</string>
  <key>manageAppVersionAndBuildNumber</key><false/>
  <key>uploadSymbols</key><true/>
</dict>
</plist>
EOF
xcodebuild -exportArchive -archivePath "$archive" -exportOptionsPlist "$export_dir/ExportOptions.plist" \
  -exportPath "$export_dir" -allowProvisioningUpdates
echo "uploaded Inkling $(/usr/libexec/PlistBuddy -c 'Print :ApplicationProperties:CFBundleShortVersionString' "$archive/Info.plist") ($build)"
