#!/bin/sh
# Downloads the real books inkling-books' tests read into target/fixtures (or $INKLING_FIXTURES).
set -eu
cd "$(dirname "$0")/.."
dir="${INKLING_FIXTURES:-target/fixtures}"
mkdir -p "$dir"
fetch() {
  [ -s "$dir/$1" ] || curl -fsSL -o "$dir/$1" "$2"
  echo "$dir/$1"
}
fetch pride-and-prejudice.epub https://www.gutenberg.org/ebooks/1342.epub3.images
fetch rfc2616.pdf https://www.rfc-editor.org/rfc/rfc2616.pdf
# A small EPUB with italics, a picture and a footnote, written from books/examples/walden.rs.
cargo run -q -p inkling-books --example walden -- "$dir/walden.epub"
echo "$dir/walden.epub"
