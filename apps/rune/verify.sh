#!/bin/sh
# Drives the headless app through what screenshots can't show and checks the element tree:
# a paste, mouse selection and a relaunch reading the autosaved draft.
set -eu
cd "$(dirname "$0")"
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
cargo build -q -p inkling-mac
bin=$PWD/target/debug/inkling
work=$(mktemp -d)
fail=0

run() { # name, data dir, script json
  printf '%s' "$3" > "$work/$1.json"
  INKLING_FAKE_AI=1 INKLING_DATA_DIR="$2" "$bin" --script "$work/$1.json" --tree "$work/$1.tree.json" --at mark:end >/dev/null 2>&1
}

check() { # name, python expression over `doc` (the editor's tree node), description
  if python3 - "$work/$1.tree.json" "$2" <<'PY'
import json, sys
def find(n):
    if isinstance(n, dict):
        if n.get("key") == "doc" and "value" in n: return n
        for v in n.values():
            r = find(v)
            if r: return r
    if isinstance(n, list):
        for v in n:
            r = find(v)
            if r: return r
doc = find(json.load(open(sys.argv[1])))
ok = eval(sys.argv[2], {"doc": doc})
print(("ok   " if ok else "FAIL ") + repr(doc.get("value")) + " selection " + repr(doc.get("selection")))
sys.exit(0 if ok else 1)
PY
  then echo "  $3"; else echo "  $3"; fail=1; fi
}

d=$(mktemp -d)
run paste "$d" '[{"t":0.1,"type":"type","text":"ab","every":0.02},{"t":0.3,"type":"paste","text":" pasted"},{"t":0.6,"type":"mark","name":"end"}]'
check paste 'doc["value"] == "ab pasted"' "a paste lands at the caret"

d=$(mktemp -d)
run drag "$d" '[{"t":0.1,"type":"type","text":"select these words","every":0.02},{"t":0.8,"type":"drag","x":145,"y":86,"dx":120,"dy":0,"dur":0.3},{"t":1.5,"type":"mark","name":"end"}]'
check drag 'doc["selection"][1] != doc["selection"][3]' "a mouse drag selects text"

d=$(mktemp -d)
run first "$d" '[{"t":0.1,"type":"type","text":"kept across launches","every":0.02},{"t":1.4,"type":"mark","name":"end"}]'
run relaunch "$d" '[{"t":0.2,"type":"mark","name":"end"}]'
check relaunch 'doc["value"] == "kept across launches"' "a relaunch shows the autosaved draft"

exit $fail
