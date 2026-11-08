#!/usr/bin/env bash
# What SolidCraft changed since the version carried in vendor/solidcraft: its commits, and the
# command ids that are new (the ones to wire into crates/aurafx-cad/src/kernel/brep.rs and the
# CAD ribbon).
#
#   scripts/cad/upstream-diff.sh [tag-or-branch]      # default: the newest release tag
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$root"
url=https://github.com/tangency-labs/solidcraft
ref="${1:-$(git ls-remote --tags --refs "$url" | grep -v nightly | sed 's#.*refs/tags/##' | sort -V | tail -1)}"
# The upstream commit the carried tree came from: recorded by git subtree in its squash commit.
have=$(git log --grep='^git-subtree-dir: vendor/solidcraft$' -1 --format=%B | sed -n 's/^git-subtree-split: //p' | head -1)
[ -n "$have" ] || { echo "no git-subtree-split record for vendor/solidcraft" >&2; exit 1; }
echo "carried: $have   new: $ref"
git fetch -q "$url" "$ref"
new=$(git rev-parse FETCH_HEAD)
echo; echo "== upstream commits since the carried version"
git log --oneline "$have..$new" 2>/dev/null | head -200 || echo "(history not fetched: git fetch $url --unshallow)"
ids() { sed -nE 's/^\s*id: "([a-z_]+(\.[a-z_0-9]+)+)".*/\1/p' | sort -u; }
git show-ref >/dev/null 2>&1 || true
old_ids=$(cat vendor/solidcraft/crates/engine/src/cmd/*.rs | ids)
new_ids=$(for f in $(git ls-tree -r --name-only "$new" crates/engine/src/cmd | grep '\.rs$'); do git show "$new:$f"; done | ids)
echo; echo "== command ids that are new"
comm -13 <(echo "$old_ids") <(echo "$new_ids")
echo; echo "== command ids that went away"
comm -23 <(echo "$old_ids") <(echo "$new_ids")
