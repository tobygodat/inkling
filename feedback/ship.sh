#!/bin/bash
# Ships one feedback from the standing checkout on main: checks, record, commit, install, push.
# The feedback agent runs it after it has made the change, or decided not to.
#
#   feedback/ship.sh done     <id> "<what changed, in the writer's words>"
#   feedback/ship.sh declined <id> "<why not>"
#
# <id> comes from the webhook body, such as 1762899330-k7q2m3xd. The line is what the writer reads
# in Feedback & fixes. INKLING_INSTALL_DIR sets where the app goes (default /Applications), and
# INKLING_SHIP_COAUTHOR, when set, adds a Co-Authored-By trailer to the commit.
#
# Exit: 0 shipped or already shipped. 1 a stage failed (its log's tail is printed; fix it and
# rerun the same command). 2 bad usage, or a checkout ship.sh won't touch. 3 installed, not
# pushed. 75 another ship.sh holds the lock.
#
# Safe to rerun after a crash at any point: each stage reads the state that exists. A record in
# HEAD skips to install, a bundle already at HEAD's build skips the install, and a HEAD that
# origin/main already has skips the push. A committed record is never rewritten.
set -euo pipefail
export PATH=/opt/homebrew/opt/rustup/bin:$PATH
self="$(cd "$(dirname "$0")" && pwd)/$(basename "$0")"
cd "$(dirname "$self")/.."

usage() {
  echo "usage: feedback/ship.sh done|declined <id> \"<one line>\"" >&2
  exit 2
}
[ $# -eq 3 ] || usage
outcome="$1" id="$2" line="$3"
case "$outcome" in done | declined) ;; *) usage ;; esac
# Only for path safety; the record writer's parser is the authority on ids.
[[ "$id" =~ ^[0-9]{1,20}-[a-z2-7]{8}$ ]] || { echo "ship.sh: \"$id\" is not a feedback id" >&2; exit 2; }

# One ship at a time per checkout: target/, the checkout and the install folder are shared. The
# kernel drops the lock when the process dies, so a crash leaves no stale lock.
if [ -z "${INKLING_SHIP_LOCKED:-}" ]; then
  status=0
  INKLING_SHIP_LOCKED=1 /usr/bin/lockf -s -t 0 "$(git rev-parse --git-common-dir)/inkling-ship.lock" "$self" "$@" || status=$?
  [ "$status" -ne 75 ] || echo "ship.sh: another ship.sh is running in this checkout; rerun when it ends" >&2
  exit "$status"
fi

dest="${INKLING_INSTALL_DIR:-/Applications}"
record="feedback/$id.json"
logs="target/ship"
mkdir -p "$logs"
now() { perl -MTime::HiRes=time -e 'printf "%.2f", time'; }
since() { perl -e 'printf "%.1f", $ARGV[1] - $ARGV[0]' "$1" "$(now)"; }
started="$(now)"
refuse() { echo "ship.sh: $*" >&2; exit 2; }

# Runs one stage with its output in target/ship/<name>.log and prints its seconds. On failure it
# prints the log's tail and exits 1, or with the stage's own code when that is 2 (bad input).
stage() {
  local name="$1" t status=0
  shift
  t="$(now)"
  "$@" >"$logs/$name.log" 2>&1 || status=$?
  if [ "$status" -ne 0 ]; then
    tail -25 "$logs/$name.log" >&2
    echo "ship.sh: $name failed after $(since "$t") s (log: $logs/$name.log). Fix it and rerun feedback/ship.sh $outcome $id \"…\"" >&2
    [ "$status" -eq 2 ] && exit 2
    exit 1
  fi
  echo "$name $(since "$t") s"
}

preflight() {
  [ "$(git symbolic-ref --short -q HEAD)" = main ] || refuse "the checkout is not on main"
  for f in MERGE_HEAD rebase-merge rebase-apply CHERRY_PICK_HEAD; do
    [ ! -e "$(git rev-parse --git-path "$f")" ] || refuse "a merge or rebase is in progress"
  done
  git fetch -q origin main || echo "ship.sh: couldn't fetch origin; going on with the last known origin/main" >&2
  git merge-base --is-ancestor origin/main HEAD ||
    refuse "origin/main has commits this checkout doesn't. Run git pull --rebase origin main, then rerun"
}

others() {
  local other
  other="$(git status --porcelain --untracked-files=all -- feedback/ | sed 's/^...//' |
    grep -E '^feedback/[^/]+\.json(\.tmp)?$' | grep -vE "^feedback/$id\.json(\.tmp)?$" || true)"
  [ -z "$other" ] || refuse "$other is from another ship; finish or delete it first"
}

commit() {
  local trailers="Feedback: $id"
  [ -z "${INKLING_SHIP_COAUTHOR:-}" ] || trailers="$trailers
Co-Authored-By: $INKLING_SHIP_COAUTHOR"
  git add -A
  if [ "$outcome" = "done" ]; then
    printf '%s (feedback %s)\n\n%s\n' "$line" "$id" "$trailers" | git commit -q -F -
  else
    printf 'Decline feedback %s\n\n%s\n\n%s\n' "$id" "$line" "$trailers" | git commit -q -F -
  fi
}

installed_build() {
  /usr/libexec/PlistBuddy -c "Print CFBundleVersion" "$dest/Inkling.app/Contents/Info.plist" 2>/dev/null || true
}

t="$(now)"
preflight
echo "preflight $(since "$t") s"
if git cat-file -e "HEAD:$record" 2>/dev/null; then
  echo "$record is already committed; the committed record stands"
else
  others
  stage tests cargo test --workspace
  stage clippy cargo clippy --workspace --all-targets --all-features -- -D warnings
  stage snap mac/snap.sh
  stage record cargo run -q -p inkling-core --example feedback-record -- "$outcome" "$id" "$line"
  stage core cargo build -p inkling-core
  stage commit commit
fi

build="$(git rev-list --count HEAD)"
if [ "$(installed_build)" = "$build" ]; then
  echo "build $build is already installed in $dest"
else
  stage install mac/install.sh "$dest"
fi

if git merge-base --is-ancestor HEAD origin/main 2>/dev/null; then
  echo "origin/main already has $(git rev-parse --short HEAD)"
elif ! git push -q origin main >"$logs/push.log" 2>&1; then
  cat "$logs/push.log" >&2
  echo "ship.sh: build $build is installed but not pushed. Fix the push, then rerun" >&2
  exit 3
else
  echo "pushed $(git rev-parse --short HEAD)"
fi
echo "shipped $id in $(git log -1 --format='%h "%s"' -- "$record"); installed build $build; $(since "$started") s"
