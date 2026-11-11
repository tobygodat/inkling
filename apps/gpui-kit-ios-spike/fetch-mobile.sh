#!/bin/bash
# Checks out gpui-mobile at the pinned commit into target/gpui-mobile and applies patches/ in order.
# Idempotent: it discards any local edits there first, so run patches/refresh.sh before it if you
# changed gpui-mobile by hand.
set -euo pipefail
cd "$(dirname "$0")"
rev=7548ebc4b24f00494af9976af8852c7163ded9ce
dir=target/gpui-mobile
if [ ! -d "$dir/.git" ]; then
  mkdir -p target
  git clone -q https://github.com/longbridge/gpui-mobile "$dir"
fi
git -C "$dir" checkout -q --force "$rev"
git -C "$dir" clean -fdq
for patch in patches/*.patch; do
  git -C "$dir" apply --whitespace=nowarn "$PWD/$patch"
done
