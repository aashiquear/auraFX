# Builds the text of docs/userguide/src/python-api.md in `_reference` (run by api_reference.rs).
import aurafx as afx, inspect

def sig(o, name):
    try:
        s = str(inspect.signature(o))
    except (TypeError, ValueError):
        s = getattr(o, "__text_signature__", None) or "(...)"
    s = s.replace("($self, ", "(").replace("($self)", "()").replace("(self, ", "(").replace("(self)", "()").replace("(/, ", "(").replace("(/)", "()")
    return name + s

def doc(o):
    d = inspect.getdoc(o) or ""
    return " ".join(line.strip() for line in d.strip().splitlines()).replace("|", "\\|")

def entry(prefix, name, o):
    d = doc(o)
    return "- `" + sig(o, prefix + name) + "`" + ((": " + d) if d else "")

filters = set()
try:
    filters = {f["id"] for f in afx.filters()}
except Exception:
    pass

out = ["# Python API reference", "",
       "<!-- Generated from the aurafx module by crates/aurafx-python/tests/api_reference.rs; do not edit. -->", "",
       "Every function of the `aurafx` module (`import aurafx as afx`) and every method of the objects it returns, with the text `help()` shows for it. "
       "Filters and sources are not repeated here: each is `node.<name>(...)` or `afx.<name>(...)` with the settings listed in the [filter reference](filter-reference.md). "
       "The [Python chapter](python.md) introduces the API by task.", ""]
funcs, classes = [], []
for name in sorted(dir(afx)):
    if name.startswith("_") or name in filters:
        continue
    o = getattr(afx, name)
    if inspect.isclass(o):
        classes.append((name, o))
    elif callable(o):
        funcs.append((name, o))
out += ["## Functions", ""]
out += [entry("afx.", n, o) for n, o in funcs]
# The helper objects reached through others: node.cad, afx.mesh, a panel's ui.
titles = {"_CadApi": "node.cad (the CAD document of a CAD Model)", "_CadSketch": "A sketch description (node.cad.sketch)",
          "_MeshModule": "afx.mesh (geometry commands)", "_MeshGeometry": "afx.mesh.geometry() (a list of commands)", "_PanelUI": "ui (inside an @afx.panel function)"}
for hidden, title in titles.items():
    if hasattr(afx, hidden):
        classes.append((title, getattr(afx, hidden)))
for cname, c in classes:
    members = [(n, getattr(c, n)) for n in sorted(dir(c)) if not n.startswith("_")]
    members = [(n, m) for n, m in members if callable(m) or isinstance(inspect.getattr_static(c, n, None), property) or inspect.isgetsetdescriptor(m)]
    if not members and not doc(c):
        continue
    out += ["", "## " + cname, ""]
    if doc(c):
        out += [doc(c), ""]
    for n, m in members:
        if callable(m):
            out.append(entry("", n, m))
        else:
            d = doc(m)
            out.append("- `" + n + "` (attribute)" + ((": " + d) if d else ""))
_reference = "\n".join(out) + "\n"
