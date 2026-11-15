#!/usr/bin/env bash
# What SolidCraft changed since the version carried in vendor/solidcraft (vendor/UPSTREAM.toml):
# its commits, and the command ids that are new or gone (the ones to wire into
# crates/aurafx-cad/src/kernel/brep.rs and the CAD ribbon). Reads only; changes nothing.
#
#   scripts/cad/upstream-diff.sh [tag-or-branch]      # default: the newest release tag
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$root"
field() { python3 -I -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))[sys.argv[2]][sys.argv[3]])' vendor/UPSTREAM.toml solidcraft "$1"; }
url="$(field url)"; have="$(field rev)"
ref="${1:-$(git ls-remote --tags --refs "$url" | grep -v nightly | sed 's#.*refs/tags/##' | sort -V | tail -1)}"
work="$(mktemp -d "${TMPDIR:-/tmp}/afx-upstream.XXXXXX")"; trap 'rm -rf "$work"' EXIT
git clone -q --filter=blob:none --no-checkout "$url" "$work/sc"
cd "$work/sc"
new="$(git rev-parse -q --verify "$ref^{commit}" 2>/dev/null || { git fetch -q origin "$ref" && git rev-parse FETCH_HEAD; })"
git cat-file -e "$have^{commit}" 2>/dev/null || git fetch -q origin "$have"
echo "carried: $(field ref) (${have:0:9})   new: $ref (${new:0:9})"
echo; echo "== upstream commits since the carried version"
git log --oneline "$have..$new" | head -200
ids() { for f in $(git ls-tree -r --name-only "$1" crates/engine/src/cmd | grep '\.rs$'); do git show "$1:$f"; done \
  | sed -nE 's/^\s*id: "([a-z_]+(\.[a-z_0-9]+)+)".*/\1/p' | sort -u; }
old_ids="$(ids "$have")"; new_ids="$(ids "$new")"
echo; echo "== command ids that are new"; comm -13 <(echo "$old_ids") <(echo "$new_ids")
echo; echo "== command ids that went away"; comm -23 <(echo "$old_ids") <(echo "$new_ids")
