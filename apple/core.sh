#!/bin/bash
# Builds inkling-ffi for the Mac, the iPhone and the Simulator, writes its Swift side with
# uniffi-bindgen, and puts both into apple/InklingCore, the Swift package the native app imports
# (ADR 0007). Both are build output and are not committed. Runs only on a Mac with Xcode.
#
#   apple/core.sh                 release build
#   PROFILE=dev apple/core.sh     debug build, as CI runs it
#
# Then `swift test --package-path apple/InklingCore` runs the package's tests on the Mac.
set -euo pipefail
cd "$(dirname "$0")/.."

profile=${PROFILE:-release}
folder=$([ "$profile" = dev ] && echo debug || echo "$profile")
targets=(aarch64-apple-darwin aarch64-apple-ios aarch64-apple-ios-sim)
package=apple/InklingCore
generated=target/inkling-ffi-swift

for target in "${targets[@]}"; do
  cargo build -p inkling-ffi --lib --profile "$profile" --target "$target"
done

rm -rf "$generated"
mkdir -p "$generated/headers"
cargo run -q -p inkling-ffi --bin uniffi-bindgen -- generate \
  "target/aarch64-apple-darwin/$folder/libinkling_ffi.dylib" \
  --language swift --no-format --out-dir "$generated"
cp "$generated/inkling_ffiFFI.h" "$generated/headers/"
cp "$generated/inkling_ffiFFI.modulemap" "$generated/headers/module.modulemap"

libraries=()
for target in "${targets[@]}"; do
  libraries+=(-library "target/$target/$folder/libinkling_ffi.a" -headers "$generated/headers")
done
rm -rf "$package/InklingFFI.xcframework"
xcodebuild -create-xcframework "${libraries[@]}" -output "$package/InklingFFI.xcframework" >/dev/null
cp "$generated/inkling_ffi.swift" "$package/Sources/InklingCore/InklingFFI.swift"
echo "InklingCore is ready in $package"
