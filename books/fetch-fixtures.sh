#!/bin/sh
# Downloads the real books inkling-books' tests read into target/fixtures (or $INKLING_FIXTURES).
set -eu
cd "$(dirname "$0")/.."
dir="${INKLING_FIXTURES:-target/fixtures}"
mkdir -p "$dir"
# fetch <name> <url>…: tries each address in turn, retrying a 503 or a dropped connection with
# backoff. The book lands in $dir only once it is complete, so a failed run leaves no partial file.
fetch() {
  name=$1
  shift
  for url in "$@"; do
    [ -s "$dir/$name" ] && break
    if curl -fsSL --retry 4 --retry-all-errors --max-time 300 -o "$dir/$name.part" "$url"; then
      mv "$dir/$name.part" "$dir/$name"
    fi
  done
  rm -f "$dir/$name.part"
  [ -s "$dir/$name" ] || { echo "could not download $name" >&2; exit 1; }
  echo "$dir/$name"
}
# The second address of each is a fixed copy on the Wayback Machine, for when the first site is down.
fetch pride-and-prejudice.epub https://www.gutenberg.org/ebooks/1342.epub3.images \
  https://web.archive.org/web/20261115011423id_/https://www.gutenberg.org/cache/epub/1342/pg1342-images-3.epub
fetch rfc2616.pdf https://www.rfc-editor.org/rfc/rfc2616.pdf \
  https://web.archive.org/web/20261114194643id_/https://www.rfc-editor.org/rfc/rfc2616.pdf
# A small EPUB with italics, a picture and a footnote, written from books/examples/walden.rs.
cargo run -q -p inkling-books --example walden -- "$dir/walden.epub"
echo "$dir/walden.epub"
