"""Translate VTK's DG basis headers (Filters/CellGrid/Basis/*/*.h, generated from Intrepid2)
into Rust: crates/aurafx-data/src/dg_basis.rs.

    python3 scripts/gen_dg_basis.py <folder with the headers named Space_File.h>

The headers are straight-line C (constants, locals, a few ifs and ternaries) assigning
basis[i] (or basis[i * 3 + c] for vector bases) and basisGradient[i * 3 + c].
"""
import os, re, sys
src = sys.argv[1]
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "crates", "aurafx-data", "src", "dg_basis.rs")

def floats(expr):
    # Bare integers in arithmetic become floats (indices stay integers).
    return re.sub(r'(?<![\w.\[])(\d+)(?![\w.\]])', r'\1.0', expr)

def statements(text):
    """Logical statements: continuation lines joined until ';', a brace, an if or else."""
    out, cur = [], ""
    for raw in text.splitlines():
        line = raw.split("//")[0].strip()
        if not line:
            continue
        cur = (cur + " " + line).strip()
        if cur.endswith(";") or cur in ("{", "}", "else") or (cur.startswith("if") and cur.endswith(")") and cur.count("(") == cur.count(")")):
            out.append(cur)
            cur = ""
    if cur:
        raise SystemExit(f"unterminated statement: {cur!r}")
    return out

def translate(text, target):
    lines = []
    for s in statements(text):
        m = re.match(r'(const\s+)?RealT\s+(\w+)\s*=\s*(.*);$', s)
        if m:
            name, expr = m.group(2), m.group(3)
            t = re.match(r'(.*)\?(.*):(.*)', expr)
            if t:
                expr = f"if {t.group(1).strip()} {{ {t.group(2).strip()} }} else {{ {t.group(3).strip()} }}"
            lines.append(f"let mut {name}: f64 = {floats(expr)};")
            continue
        m = re.match(r'(basis|basisGradient)\[(.*)\]\s*=\s*(.*);$', s)
        if m:
            lines.append(f"{target}[{m.group(2)}] = {floats(m.group(3))};")
            continue
        m = re.match(r'(\w+)\s*([+\-*/]?=)\s*(.*);$', s)
        if m:
            lines.append(f"{m.group(1)} {m.group(2)} {floats(m.group(3))};")
            continue
        m = re.match(r'if\s*\((.*)\)$', s)
        if m:
            lines.append(f"if {m.group(1)}")
            continue
        if s in ("{", "}", "else"):
            lines.append(s)
            continue
        raise SystemExit(f"cannot translate: {s!r}")
    body = "\n".join(lines)
    body = re.sub(r'(?<![\w:])abs\(', 'f64::abs(', body)
    # Reads of earlier results; unary plus (not Rust).
    body = body.replace("basisGradient[", "g[").replace("basis[", "b[")
    body = re.sub(r'([=(,]\s*)\+(?=[\w(.])', r'\1', body)
    return body

def count(text, name, per):
    idx = [int(m.group(1)) for m in re.finditer(name + r'\[(\d+)(?:\s*\*\s*3\s*\+\s*\d+)?\]', text)]
    return (max(idx) + 1) if idx else 0

entries = []
fns = []
for f in sorted(os.listdir(src)):
    if not f.endswith(".h"):
        continue
    space, rest = f[:-2].split("_", 1)
    text = open(os.path.join(src, f)).read()
    grad = rest.endswith("Gradient")
    kind = rest[: -len("Gradient")] if grad else rest[: -len("Basis")]
    fname = f"{space.lower()}_{kind.lower()}_{'gradient' if grad else 'basis'}"
    target = "g" if grad else "b"
    body = translate(text, target)
    if grad:
        vec = re.findall(r'basisGradient\[(\d+)\s*\*\s*3', text)
        n = max(int(x) for x in vec) + 1 if vec else (max(int(x) for x in re.findall(r'basisGradient\[(\d+)\]', text)) + 1) // 3
    elif space in ("HCurl", "HDiv"):
        n = max(int(x) for x in re.findall(r'basis\[(\d+)\s*\*\s*3', text)) + 1
    else:
        n = max(int(x) for x in re.findall(r'basis\[(\d+)\]', text)) + 1
    fns.append(f"#[allow(unused_mut, unused_variables, clippy::all)]\npub fn {fname}(p: [f64; 3], {target}: &mut [f64]) {{\n    let (mut rr, mut ss, mut tt) = (p[0], p[1], p[2]);\n    let eps = f64::EPSILON;\n{body}\n}}\n")
    entries.append((space, kind, grad, fname, n))

with open(out, "w") as o:
    o.write("//! DG basis functions, translated from VTK's Filters/CellGrid/Basis headers (generated from\n//! Intrepid2; VTK is BSD-licensed) by scripts/gen_dg_basis.py. Do not edit by hand.\n\n")
    o.write("#![allow(unused_parens, unused_mut, unused_variables, unused_assignments, non_snake_case, clippy::all)]\n\n")
    o.write("pub type BasisFn = fn([f64; 3], &mut [f64]);\n\n")
    o.write("/// (function space, shape and basis kind like \"HexC1\", gradient?, function, number of functions)\n")
    o.write("pub const TABLE: &[(&str, &str, bool, BasisFn, usize)] = &[\n")
    for space, kind, grad, fname, n in entries:
        o.write(f'    ("{ {"HGrad": "HGRAD", "HCurl": "HCURL", "HDiv": "HDIV", "Constant": "constant"}[space] }", "{kind}", {str(grad).lower()}, {fname}, {n}),\n')
    o.write("];\n\n")
    o.write("\n".join(fns))
print(out, len(entries))
