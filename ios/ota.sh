#!/bin/bash
# Archives Inkling Dev (com.tobygodat.inkling.dev) for a real iPhone in Release, signs it for
# development with the "Inkling Dev Development" profile (team Q6NAWA42Y2, the registered iPhones),
# uploads it to inkling-server and prints the install link, good for 24 hours.
# Usage: ota.sh [build number]
# The build number defaults to the UTC time, as in testflight.sh. Signing is manual, so no Xcode
# account is needed: the profile must be installed and the Apple Development identity in the login
# Keychain. The upload token is read from the login Keychain (service inkling-ota, account
# upload-token) and goes to curl on stdin, so it is never in a file, an argument or the log.
# INKLING_SERVER overrides the Worker's address.
set -euo pipefail
cd "$(dirname "$0")"
root="$(pwd)"
build="${1:-$(date -u +%Y%m%d.%H%M)}"
server="${INKLING_SERVER:-https://inkling-server.tobygodat.workers.dev}"
archive="$root/target/Inkling-ota.xcarchive"
export_dir="$root/target/ota"
bundle_id="com.tobygodat.inkling.dev"
display_name="Inkling Dev"
profile="Inkling Dev Development"

security find-generic-password -s inkling-ota -a upload-token >/dev/null 2>&1 \
  || { echo "error: no upload-token in the login Keychain under inkling-ota; see ios/README.md" >&2; exit 1; }

# Own bundle ID and display name so OTA installs sit beside TestFlight (com.tobygodat.inkling).
./fetch-mobile.sh
PLATFORM_NAME=iphoneos CONFIGURATION=Release ./cargo-ios.sh
xcodegen generate --spec app/project.yml --project app --quiet
rm -rf "$archive" "$export_dir"
mkdir -p "$export_dir"
xcodebuild -project app/Inkling.xcodeproj -scheme Inkling -configuration Release \
  -destination 'generic/platform=iOS' -archivePath "$archive" -derivedDataPath "$root/target/xcode-ota" \
  CURRENT_PROJECT_VERSION="$build" \
  PRODUCT_BUNDLE_IDENTIFIER="$bundle_id" \
  CODE_SIGN_STYLE=Manual CODE_SIGN_IDENTITY="Apple Development" \
  PROVISIONING_PROFILE_SPECIFIER="$profile" \
  INFOPLIST_KEY_CFBundleDisplayName="$display_name" \
  -quiet archive

cat >"$export_dir/ExportOptions.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>debugging</string>
  <key>destination</key><string>export</string>
  <key>teamID</key><string>Q6NAWA42Y2</string>
  <key>signingStyle</key><string>manual</string>
  <key>signingCertificate</key><string>Apple Development</string>
  <key>provisioningProfiles</key>
  <dict><key>$bundle_id</key><string>$profile</string></dict>
  <key>thinning</key><string>&lt;none&gt;</string>
</dict>
</plist>
EOF
xcodebuild -exportArchive -archivePath "$archive" -exportOptionsPlist "$export_dir/ExportOptions.plist" \
  -exportPath "$export_dir"

version="$(/usr/libexec/PlistBuddy -c 'Print :ApplicationProperties:CFBundleShortVersionString' "$archive/Info.plist")"
response="$(security find-generic-password -s inkling-ota -a upload-token -w \
  | sed 's/^/Authorization: Bearer /' \
  | curl --silent --show-error --fail-with-body -H @- -H 'content-type: application/octet-stream' \
    --data-binary @"$export_dir/Inkling.ipa" "$server/builds?version=$version&build=$build")"
echo "uploaded $display_name $version ($build). Open this on the iPhone within 24 hours:"
plutil -extract url raw -o - - <<<"$response"
