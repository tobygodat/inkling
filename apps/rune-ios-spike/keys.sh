#!/bin/bash
# Types on the Simulator's on-screen keyboard by tapping its keys (iPhone 18 Pro, portrait,
# English QWERTY, points). Usage: keys.sh <agent-device> "<text>" where text uses lowercase
# letters, space, '<' for delete and '/' for return. Tapping keys, not `agent-device type`,
# keeps the software keyboard up and sends input the way a finger does.
set -euo pipefail
ad="$1"
text="$2"
key() {
  case "$1" in
    q) echo "24 618" ;; w) echo "63 618" ;; e) echo "102 618" ;; r) echo "141 618" ;; t) echo "180 618" ;;
    y) echo "220 618" ;; u) echo "259 618" ;; i) echo "299 618" ;; o) echo "339 618" ;; p) echo "378 618" ;;
    a) echo "43 672" ;; s) echo "82 672" ;; d) echo "122 672" ;; f) echo "161 672" ;; g) echo "200 672" ;;
    h) echo "240 672" ;; j) echo "279 672" ;; k) echo "318 672" ;; l) echo "358 672" ;;
    z) echo "82 727" ;; x) echo "122 727" ;; c) echo "161 727" ;; v) echo "200 727" ;; b) echo "240 727" ;;
    n) echo "279 727" ;; m) echo "318 727" ;; '<') echo "372 727" ;;
    ' ') echo "200 780" ;; '/') echo "349 780" ;;
    *) echo "unknown key '$1'" >&2; exit 1 ;;
  esac
}
for ((i = 0; i < ${#text}; i++)); do
  # shellcheck disable=SC2046
  "$ad" press $(key "${text:i:1}") >/dev/null
done
