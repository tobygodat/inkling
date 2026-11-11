#!/bin/bash
# Checks out gpui-mobile at the pinned commit into target/gpui-mobile and moves it from
# gpui-pre 0.3.10 to 0.3.12, the version GPUI Kit 0.9.1 is built on. Idempotent.
set -euo pipefail
cd "$(dirname "$0")"
rev=7548ebc4b24f00494af9976af8852c7163ded9ce
dir=target/gpui-mobile
if [ ! -d "$dir/.git" ]; then
  mkdir -p target
  git clone -q https://github.com/longbridge/gpui-mobile "$dir"
fi
git -C "$dir" checkout -q --force "$rev"
sed -i '' -e 's/version = "=0\.3\.10"/version = "=0.3.12"/g' -e '/^\[patch\.crates-io\]/,$d' -e '/^offscreen = /d' "$dir/Cargo.toml"
# gpui-pre-wgpu 0.3.12 takes the surface config when it picks an adapter.
sed -i '' 's/WgpuContext::new(metal_instance, &surface, None)/WgpuContext::new(metal_instance, \&surface, \&config, None)/' "$dir/src/ios/window.rs"
# gpui-pre 0.3.12 asks for the keyboard through PlatformWindow::show_soft_keyboard when a text
# input is tapped. gpui-mobile predates that method, so its default no-op would swallow the request.
perl -0pi -e 's/(impl PlatformWindow for IosWindow \{\n)/$1    fn show_soft_keyboard(&self) {\n        self.show_keyboard_with_type(crate::KeyboardType::Default);\n    }\n\n    fn hide_soft_keyboard(&self) {\n        self.hide_keyboard();\n    }\n\n/' "$dir/src/ios/window.rs"
