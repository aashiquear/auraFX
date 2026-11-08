#!/usr/bin/env bash
# Pull a newer SolidCraft into vendor/solidcraft (and the solidcore revision it pins into
# vendor/solidcore) with git subtree, prune again, and say what to do next.
#
#   scripts/cad/upstream-pull.sh v0.40.0        # a SolidCraft tag
#
# The trees are carried in this repository (vendor/README.md). Only whole directories are ever
# deleted (vendor/prune.txt), so a pull merges; a file upstream changed in a directory we pruned
# shows as a modify/delete conflict, which this script settles by keeping it deleted.
set -euo pipefail
tag="${1:?usage: upstream-pull.sh <solidcraft tag>}"
root="$(cd "$(dirname "$0")/../.." && pwd)"; cd "$root"
sc_url=https://github.com/tangency-labs/solidcraft
core_url=https://github.com/tangency-labs/solidcore
[ -z "$(git status --porcelain)" ] || { echo "the working tree has changes: commit or stash them first" >&2; exit 1; }

prune() {
  grep -v '^#' vendor/prune.txt | grep -v '^$' | while read -r p; do git rm -rq --ignore-unmatch "$p" || true; done
}
settle() {  # after a subtree pull that stopped on modify/delete conflicts in pruned directories
  prune
  if [ -n "$(git diff --name-only --diff-filter=U)" ]; then
    echo "conflicts remain outside the pruned directories:" >&2; git diff --name-only --diff-filter=U >&2
    echo "resolve them, run 'git add -A' and 'git commit --no-edit', then re-run this script's remaining steps by hand (vendor/README.md)." >&2
    exit 1
  fi
  git add -A; git commit -q --no-edit
}

echo "== SolidCraft $tag"
git subtree pull --prefix vendor/solidcraft "$sc_url" "$tag" --squash -m "Vendor SolidCraft $tag (git subtree pull)" || settle
rev=$(git fetch -q "$sc_url" "$tag" && git show FETCH_HEAD:Cargo.toml | grep -E '^solidcore *=' | sed -E 's/.*rev *= *"([0-9a-f]+)".*/\1/' | head -1)
[ -n "$rev" ] || { echo "could not read the solidcore revision $tag pins" >&2; exit 1; }
echo "== solidcore $rev (pinned by $tag)"
git subtree pull --prefix vendor/solidcore "$core_url" "$rev" --squash -m "Vendor solidcore $rev (git subtree pull)" || settle
prune
if [ -n "$(git status --porcelain)" ]; then git add -A; git commit -q -m "Prune the vendored trees again after pulling SolidCraft $tag"; fi

cat <<MSG

Pulled SolidCraft $tag with solidcore $rev. Next:
  1. update the tag, revision and date in vendor/README.md and docs/reference/third-party.md
     (scripts/cad/third-party.sh regenerates the licence table);
  2. cargo build --release -p aurafx-app && cargo test -p aurafx-cad;
  3. scripts/cad/upstream-diff.sh shows the commands the new version added (wire them into
     crates/aurafx-cad/src/kernel/brep.rs and the ribbon);
  4. a change of ours inside the trees carries '// auraFX:' and an entry in vendor/README.md.
MSG
