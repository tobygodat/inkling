#!/bin/sh
# Deploys only what main holds, so an unmerged branch never reaches production (TOB-573).
git fetch -q origin main || { echo "deploy refused: could not fetch origin/main" >&2; exit 1; }
[ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "deploy refused: HEAD is not origin/main" >&2; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "deploy refused: the working tree has uncommitted changes" >&2; exit 1; }
