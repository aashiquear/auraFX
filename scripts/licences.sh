#!/usr/bin/env bash
# The licences of everything auraFX builds in, from the dependency lock file.
#
#   scripts/licences.sh          # regenerate THIRD-PARTY-NOTICES.txt and docs/reference/third-party.md
#   scripts/licences.sh --check  # CI: stop on a licence outside the allowed list, or on a crate
#                                # missing from THIRD-PARTY-NOTICES.txt (run without --check then)
#
# Run it after any dependency change and after scripts/cad/upstream-pull.sh. THIRD-PARTY-NOTICES.txt
# is shipped in every package and shown in Help > About auraFX > Third-party licences: MIT, BSD and
# Apache-2.0 ask for their copyright notices, licence texts and NOTICE files to go with the program.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"; cd "$root"
meta="$(mktemp "${TMPDIR:-/tmp}/afx-meta.XXXXXX.json")"; trap 'rm -f "$meta"' EXIT
cargo metadata --format-version 1 --locked > "$meta"
python3 -I - "$meta" "${1:-}" <<'PY'
import json, os, re, sys, collections, hashlib

meta, mode = sys.argv[1], sys.argv[2]
m = json.load(open(meta))
pk = {p["id"]: p for p in m["packages"]}
nodes = {n["id"]: n for n in m["resolve"]["nodes"]}

# Everything a workspace member builds in: normal and build dependencies (not dev), all platforms.
seen, stack = set(), list(m["workspace_members"])
while stack:
    i = stack.pop()
    if i in seen:
        continue
    seen.add(i)
    for d in nodes[i]["deps"]:
        if any(k["kind"] in (None, "build") for k in d["dep_kinds"]):
            stack.append(d["pkg"])
own = lambda p: "/crates/aurafx-" in p["manifest_path"] and "/vendor/" not in p["manifest_path"]
crates = sorted((pk[i] for i in seen if not own(pk[i])), key=lambda p: (p["name"], p["version"]))

def where(p):
    path = p["manifest_path"]
    if "/vendor/solidcraft/" in path: return "vendor/solidcraft"
    if "/vendor/solidcore/" in path: return "vendor/solidcore"
    s = p["source"] or ""
    return "crates.io" if s.startswith("registry") else ("git" if s.startswith("git") else "path")

# Licences auraFX (MIT OR Apache-2.0) can ship. A package passes when every AND-part of its SPDX
# expression has at least one option here. MPL-2.0 is file-level: fine while the files are
# unmodified (patch a dependency under MPL-2.0 only in a public fork). Fonts: OFL and the Ubuntu
# font licence travel with the font (their texts are in the notices).
ALLOWED = {"MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause",
           "ISC", "Zlib", "0BSD", "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "BSL-1.0", "Unlicense",
           "CDLA-Permissive-2.0", "NCSA", "bzip2-1.0.6", "MPL-2.0", "OFL-1.1", "Ubuntu-font-1.0"}
def allowed(expr):
    if not expr:
        return False
    parts = re.split(r"\s+AND\s+", expr.replace("/", " OR "))
    return all(any(o.strip(" ()") in ALLOWED for o in re.split(r"\s+OR\s+", part.strip(" ()"))) for part in parts)

bad = [f'{p["name"]} {p["version"]} ({p["license"] or "no SPDX licence; file " + str(p.get("license_file"))})'
       for p in crates if not allowed(p["license"])]
if bad:
    print("licences outside the allowed list (scripts/licences.sh says what is allowed and why):", file=sys.stderr)
    for b in bad: print("  " + b, file=sys.stderr)
    sys.exit(1)

line = lambda p: f'{p["name"]} {p["version"]}  {p["license"]}  [{where(p)}]'
if mode == "--check":
    have = open("THIRD-PARTY-NOTICES.txt", encoding="utf-8").read() if os.path.exists("THIRD-PARTY-NOTICES.txt") else ""
    missing = [line(p) for p in crates if "\n" + line(p) + "\n" not in have]
    if missing:
        print("THIRD-PARTY-NOTICES.txt is out of date (run scripts/licences.sh and commit it):", file=sys.stderr)
        for x in missing[:20]: print("  " + x, file=sys.stderr)
        sys.exit(1)
    print(f"licences: {len(crates)} crates, all allowed and listed")
    sys.exit(0)

LIC = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright)([-_.].*)?$", re.I)
def texts(d, extra=None):
    out = []
    names = sorted(os.listdir(d)) if os.path.isdir(d) else []
    for f in names + ([extra] if extra and extra not in names else []):
        p = os.path.join(d, f)
        if f and LIC.match(os.path.basename(f)) and os.path.isfile(p):
            out.append((os.path.basename(f), open(p, encoding="utf-8", errors="replace").read().strip()))
    # Licence folders, and the licence texts that come with built-in fonts (fonts/OFL.txt).
    for sub in ("fonts", "licenses", "LICENSES", "licences", "license"):
        s = os.path.join(d, sub)
        if os.path.isdir(s):
            for f in sorted(os.listdir(s)):
                if f.lower().endswith((".txt", ".md")) and os.path.isfile(os.path.join(s, f)):
                    out.append((f"{sub}/{f}", open(os.path.join(s, f), encoding="utf-8", errors="replace").read().strip()))
    return out
read = lambda p: open(p, encoding="utf-8").read().strip()

o = []
w = o.append
w("auraFX third-party notices")
w("==========================")
w("")
w("auraFX is licensed under the MIT licence or the Apache License 2.0, at your option (LICENSE-MIT,")
w("LICENSE-APACHE). It includes the open-source software below. This file passes on the copyright")
w("notices, NOTICE files and licence texts that software asks to be passed on. It is generated by")
w("scripts/licences.sh from Cargo.lock; do not edit it by hand.")
w("")
w("1. Code carried in the auraFX repository")
w("----------------------------------------")
w("")
w("The B-rep CAD kernel and the CAD file formats come from two open-source projects, carried in the")
w("repository under vendor/ (vendor/README.md says which version and what auraFX changed):")
w("")
w("  SolidCraft  https://github.com/tangency-labs/solidcraft  MIT OR Apache-2.0  (vendor/solidcraft)")
w("  solidcore   https://github.com/tangency-labs/solidcore   Apache-2.0         (vendor/solidcore)")
w("")
for name, d in (("SolidCraft", "vendor/solidcraft"), ("solidcore", "vendor/solidcore")):
    w(f"---- NOTICE of {name} ({d}/NOTICE)")
    w(read(f"{d}/NOTICE"))
    w("")
    for f in sorted(x for x in os.listdir(d) if x.startswith("LICENSE")):
        w(f"---- {d}/{f}")
        w(read(f"{d}/{f}"))
        w("")
w("The fonts and render textures SolidCraft builds in are listed file by file, with their authors")
w("and licences, in vendor/solidcraft/ATTRIBUTION.md. The font licences:")
w("")
fonts = sorted(os.path.join(r, f) for r, _, fs in os.walk("vendor/solidcraft/assets") for f in fs
               if re.match(r"^(OFL|UFL|LICEN[CS]E)", f) and f.endswith(".txt"))
for f in fonts:
    w(f"---- {f}")
    w(read(f))
    w("")
w("---- Phosphor icons (assets/fonts/Phosphor.ttf), MIT")
w(read("assets/fonts/Phosphor-LICENSE-MIT.txt"))
w("")
w("2. Python runtime in the installers")
w("-----------------------------------")
w("")
w("The installers include a portable CPython (python-build-standalone, Python Software Foundation")
w("License) and NumPy (BSD-3-Clause). Their licence files are inside the python folder next to the")
w("program (LICENSE.txt in the Python library folder, LICENSE.txt in numpy's dist-info folder).")
w("")
w(f"3. Rust crates built into auraFX ({len(crates)})")
w("-" * len(f"3. Rust crates built into auraFX ({len(crates)})"))
w("")
w("name version  SPDX licence  [where it comes from]")
w("")
for p in crates:
    w(line(p))
w("")
w("4. Licence texts of the crates")
w("------------------------------")
w("")
w("Crates that share the same texts are listed together. Where a crate ships no licence file, the")
w("standard text of its SPDX licence applies (MIT: LICENSE-MIT here without auraFX's copyright line;")
w("Apache-2.0: LICENSE-APACHE).")
w("")
groups, none = collections.OrderedDict(), []
for p in crates:
    d = os.path.dirname(p["manifest_path"])
    t = texts(d, p.get("license_file"))
    if not t:
        none.append(p)
        continue
    key = hashlib.sha256("\0".join(re.sub(r"\s+", " ", x) for _, x in t).encode()).hexdigest()
    groups.setdefault(key, ([], t))[0].append(f'{p["name"]} {p["version"]}')
for names, t in groups.values():
    w("=" * 78)
    w(", ".join(names))
    for f, x in t:
        w(f"---- {f}")
        w(x)
    w("")
if none:
    w("=" * 78)
    w("Crates without a licence file in the published crate (the standard SPDX text applies, with the")
    w("copyright notice in their repository; for vendor/ crates, see section 1 and the licence files")
    w("at the root of their tree):")
    for p in none:
        w("  " + line(p) + (f'  {p["repository"]}' if p.get("repository") else ""))
open("THIRD-PARTY-NOTICES.txt", "w", encoding="utf-8").write("\n".join(o) + "\n")

# The summary page of the reference docs.
by = collections.defaultdict(list)
for p in crates:
    by[(where(p), p["license"])].append(f'{p["name"]} {p["version"]}')
d = ["# Third-party code", "",
     "auraFX is MIT OR Apache-2.0. This page is generated by `scripts/licences.sh` from `Cargo.lock`:",
     f"every crate the app and the server build in ({len(crates)}), grouped by where it comes from and its",
     "SPDX licence. `THIRD-PARTY-NOTICES.txt` at the repository root carries the licence texts and NOTICE",
     "files; the packages ship it and Help › About auraFX › Third-party licences shows it.", "",
     "The B-rep CAD kernel and the CAD file formats come from SolidCraft",
     "(https://github.com/tangency-labs/solidcraft, MIT OR Apache-2.0) and solidcore",
     "(https://github.com/tangency-labs/solidcore, Apache-2.0), carried in `vendor/` (`vendor/README.md`).",
     "The parametric part model, sketch solver and expression evaluator were ported from the author's",
     "own EZ-YX-craft project (MIT OR Apache-2.0).", "",
     "Allowed licences, checked in CI (`scripts/licences.sh --check`): " + ", ".join(sorted(ALLOWED)) + ".", ""]
order = {"vendor/solidcraft": 0, "vendor/solidcore": 1, "crates.io": 2, "git": 3, "path": 4}
for (src, lic) in sorted(by, key=lambda k: (order.get(k[0], 9), k[1] or "")):
    d += [f"## {src}: {lic}", "", ", ".join(by[(src, lic)]), ""]
open("docs/reference/third-party.md", "w", encoding="utf-8").write("\n".join(d))
print(f"licences: {len(crates)} crates, {len(groups)} distinct licence texts, {len(none)} without a licence file")
PY
