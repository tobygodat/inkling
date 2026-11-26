#!/bin/bash
# Checks out gpui-mobile at the pinned commit into target/gpui-mobile and applies patches/ in order.
# Idempotent: it discards any local edits there first, so run patches/refresh.sh before it if you
# changed gpui-mobile by hand.
set -euo pipefail
cd "$(dirname "$0")"
rev=7548ebc4b24f00494af9976af8852c7163ded9ce
dir=target/gpui-mobile
clone() {
  rm -rf "$dir"
  mkdir -p target
  git clone -q https://github.com/longbridge/gpui-mobile "$dir"
}
[ -d "$dir/.git" ] || clone
# CI's cache of target/ can restore the checkout without objects the pin needs. Clone it afresh then.
if ! git -C "$dir" checkout -q --force "$rev" 2>/dev/null; then
  echo "target/gpui-mobile cannot check out $rev; cloning it again" >&2
  clone
  git -C "$dir" checkout -q --force "$rev"
fi
git -C "$dir" clean -fdq
for patch in patches/*.patch; do
  git -C "$dir" apply --whitespace=nowarn "$PWD/$patch"
done
