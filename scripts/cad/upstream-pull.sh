#!/usr/bin/env bash
# Take upstream work into the vendored SolidCraft (and the solidcore revision it pins) as a patch:
# the upstream change from the carried revision (vendor/UPSTREAM.toml) to <ref>, limited to the
# directories auraFX carries (vendor/prune.txt lists the others), applied with a three-way merge
# so that auraFX's own edits inside the trees stay. For developers; users never need it.
#
#   scripts/cad/upstream-pull.sh v0.41.0             # a SolidCraft tag, branch or commit
#   scripts/cad/upstream-pull.sh v0.41.0 --dry-run   # report what would change, apply nothing
#
# The upstream repositories are cloned (without file contents until needed) into a temporary
# folder outside this repository, so none of their history enters it. Only the changed files of
# the carried directories do. Nothing is committed: review, build, test, then commit.
set -euo pipefail
ref="${1:?usage: upstream-pull.sh <solidcraft tag|branch|commit> [--dry-run]}"
dry=0; [ "${2:-}" = "--dry-run" ] && dry=1
root="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$root"
[ -z "$(git status --porcelain)" ] || { echo "the working tree has changes: commit or stash them first" >&2; exit 1; }
work="$(mktemp -d "${TMPDIR:-/tmp}/afx-upstream.XXXXXX")"; trap 'rm -rf "$work"' EXIT

field() { python3 -I -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))[sys.argv[2]][sys.argv[3]])' vendor/UPSTREAM.toml "$1" "$2"; }
set_fields() { # tree ref rev: record the new revision
  python3 -I - vendor/UPSTREAM.toml "$1" "$2" "$3" "$(date +%F)" <<'PY'
import re, sys
p, tree, ref, rev, day = sys.argv[1:]
s = open(p).read()
head, sep, rest = s.partition(f"[{tree}]\n")
body, nxt, tail = rest.partition("\n[")
for k, v in (("ref", ref), ("rev", rev), ("carried", day)):
    body = re.sub(rf'^{k} = ".*"$', f'{k} = "{v}"', body, flags=re.M)
open(p, "w").write(head + sep + body + nxt + tail)
PY
}
excludes() {
  grep -v '^#' vendor/prune.txt | grep "^vendor/$1/" | sed "s#^vendor/$1/#:(exclude)#"
  # Newer SolidCraft carries solidcore inside its own tree (solidcore/): prune it the same way.
  [ "$1" = solidcraft ] && grep -v '^#' vendor/prune.txt | grep '^vendor/solidcore/' | sed 's#^vendor/solidcore/#:(exclude)solidcore/#'
  return 0
}
clone() { # tree ref -> prints the commit
  local dir="$work/$1"
  [ -d "$dir" ] || git clone -q --filter=blob:none --no-checkout "$(field "$1" url)" "$dir"
  git -C "$dir" rev-parse -q --verify "$2^{commit}" 2>/dev/null \
    || { git -C "$dir" fetch -q origin "$2" && git -C "$dir" rev-parse FETCH_HEAD; }
}
status=0
take() { # tree ref
  local tree=$1 want=$2 have new dir
  have="$(field "$tree" rev)"; new="$(clone "$tree" "$want")"; dir="$work/$tree"
  git -C "$dir" cat-file -e "$have^{commit}" 2>/dev/null || git -C "$dir" fetch -q origin "$have"
  echo "== $tree: carried $(field "$tree" ref) (${have:0:9}), new $want (${new:0:9})"
  if [ "$have" = "$new" ]; then echo "   already carried"; return; fi
  echo "   $(git -C "$dir" rev-list --count "$have..$new" 2>/dev/null || echo '?') upstream commits"
  # Directories upstream added that are not carried yet and not pruned: decide per directory.
  local added; added="$(git -C "$dir" diff --name-only --diff-filter=A "$have" "$new" -- . $(excludes "$tree") \
    | awk -F/ '{ d = ($1 == "crates" || $1 == "assets") ? $1 "/" $2 : $1; print d }' | sort -u \
    | while read -r d; do [ -e "vendor/$tree/$d" ] || echo "$d"; done)"
  [ -z "$added" ] || { echo "   new upstream directories (carried by this pull; add them to vendor/prune.txt if auraFX does not need them):"; echo "$added" | sed 's/^/     /'; }
  git -C "$dir" diff --binary --full-index "$have" "$new" -- . $(excludes "$tree") > "$work/$tree.patch"
  git apply --stat --directory="vendor/$tree" "$work/$tree.patch" | tail -1 | sed 's/^/  /'
  [ "$dry" = 1 ] && return
  if git apply --3way --whitespace=nowarn --directory="vendor/$tree" "$work/$tree.patch"; then
    echo "   applied"
  else
    echo "   applied with conflicts (auraFX and upstream changed the same lines): resolve the files below" >&2
    git diff --name-only --diff-filter=U | sed 's/^/     /' >&2
    status=1
  fi
  set_fields "$tree" "$want" "$new"
}

take solidcraft "$ref"
# solidcore: the revision the new SolidCraft pins, or carried inside SolidCraft's own tree.
manifest="$(git -C "$work/solidcraft" show "$(clone solidcraft "$ref"):Cargo.toml")"
core="$(echo "$manifest" | sed -nE 's/^solidcore *=.*rev *= *"([0-9a-f]+)".*/\1/p' | head -1)"
if [ -n "$core" ]; then
  take solidcore "$core"
elif echo "$manifest" | grep -qE '^solidcore *=.*path *= *"solidcore/'; then
  cat <<MSG
== solidcore: SolidCraft $ref carries it inside its own tree (solidcore/), no longer by git revision.
   This pull brings it to vendor/solidcraft/solidcore (pruned like vendor/solidcore). Once, by hand:
   remove vendor/solidcore and the [patch."https://github.com/tangency-labs/solidcore"] section of
   the root Cargo.toml, move the vendor/solidcore/ lines of vendor/prune.txt to
   vendor/solidcraft/solidcore/, and drop the [solidcore] section of vendor/UPSTREAM.toml
   (vendor/README.md, "When upstream moves solidcore").
MSG
else
  echo "could not tell how SolidCraft $ref includes solidcore (Cargo.toml): stop and check by hand" >&2; exit 1
fi
# Directories that must exist, empty (vendor/keep.txt says why).
grep -v '^#' vendor/keep.txt | grep -v '^$' | while read -r p; do mkdir -p "$p"; [ -e "$p/.gitkeep" ] || { touch "$p/.gitkeep"; [ "$dry" = 1 ] || git add -f "$p/.gitkeep"; }; done
[ "$dry" = 1 ] && exit 0
cat <<MSG

Next:
  1. review: git diff --cached --stat (and git diff for unresolved conflicts);
  2. scripts/licences.sh (regenerates THIRD-PARTY-NOTICES.txt and docs/reference/third-party.md;
     it stops on a licence that is not permissive);
  3. cargo build --release -p aurafx-app && cargo test -p aurafx-cad;
  4. scripts/cad/upstream-diff.sh lists the commands the new version added (wire them into
     crates/aurafx-cad/src/kernel/brep.rs and the ribbon);
  5. update the table in vendor/README.md, then commit ("Take SolidCraft $ref into vendor/").
MSG
exit $status
