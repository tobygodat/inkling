#!/bin/bash
# Archives Inkling for a real iPhone in Release, signs it for the App Store (team Q6NAWA42Y2) and
# uploads it to TestFlight. Usage: testflight.sh [build number]
# The build number defaults to the UTC time, so each upload is higher than the last.
# Signing uses Xcode's signed-in account, which may use the cloud-managed distribution
# certificate. API keys may not, so the key only uploads. The upload uses the App Store Connect
# API key in the login Keychain (service inkling-app-store-connect: key-id, issuer-id and p8, the
# .p8 file in base64), written to a private temp folder for the run and deleted after.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"
build="${1:-$(date -u +%Y%m%d.%H%M)}"
archive="$root/target/Inkling.xcarchive"
export_dir="$root/target/testflight"

svc=inkling-app-store-connect
for account in key-id issuer-id p8; do
  security find-generic-password -s "$svc" -a "$account" >/dev/null 2>&1 \
    || { echo "error: no $account in the login Keychain under $svc; see ios/README.md" >&2; exit 1; }
done

./fetch-mobile.sh
# Build the library before Xcode reads the -force_load input; on a clean target/ the build phase
# runs too late and the archive fails with "Build input file cannot be found".
PLATFORM_NAME=iphoneos CONFIGURATION=Release ./cargo-ios.sh
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
  <key>destination</key><string>export</string>
  <key>teamID</key><string>Q6NAWA42Y2</string>
  <key>signingStyle</key><string>automatic</string>
  <key>manageAppVersionAndBuildNumber</key><false/>
  <key>uploadSymbols</key><true/>
</dict>
</plist>
EOF
xcodebuild -exportArchive -archivePath "$archive" -exportOptionsPlist "$export_dir/ExportOptions.plist" \
  -exportPath "$export_dir" -allowProvisioningUpdates

key_dir="$(mktemp -d -t inkling-asc)"
trap 'rm -rf "$key_dir"' EXIT
key_id="$(security find-generic-password -s "$svc" -a key-id -w)"
security find-generic-password -s "$svc" -a p8 -w | base64 -d >"$key_dir/AuthKey_$key_id.p8"
xcrun altool --upload-app -f "$export_dir/Inkling.ipa" -t ios --apiKey "$key_id" \
  --apiIssuer "$(security find-generic-password -s "$svc" -a issuer-id -w)" \
  --p8-file-path "$key_dir/AuthKey_$key_id.p8"
echo "uploaded Inkling $(/usr/libexec/PlistBuddy -c 'Print :ApplicationProperties:CFBundleShortVersionString' "$archive/Info.plist") ($build)"
