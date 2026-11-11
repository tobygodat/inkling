#!/bin/bash
# Rewrites the last patch in this folder from target/gpui-mobile: the difference between the
# pinned commit with every earlier patch applied and the working tree. Edit gpui-mobile in place,
# run this, then build.
set -euo pipefail
cd "$(dirname "$0")/.."
rev=7548ebc4b24f00494af9976af8852c7163ded9ce
dir=target/gpui-mobile
patches=(patches/*.patch)
last="${patches[${#patches[@]}-1]}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir "$tmp/a" "$tmp/b"
git -C "$dir" archive "$rev" src Cargo.toml | tar -x -C "$tmp/a"
for patch in "${patches[@]}"; do
  [ "$patch" = "$last" ] || (cd "$tmp/a" && git apply --whitespace=nowarn "$OLDPWD/$patch")
done
cp -R "$dir/src" "$dir/Cargo.toml" "$tmp/b/"
(cd "$tmp" && diff -ruN a b | perl -pe 's/^((?:---|\+\+\+) \S+)\t.*/$1/') > "$last" || true
echo "$last: $(grep -c '^[-+][^-+]' "$last") changed lines"
