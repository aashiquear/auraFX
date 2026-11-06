"""A small CML molecule for the molecule tests: testdata/molecules/test.cml.

    python3 scripts/gen_cml.py

A benzene ring with Cl, NH2 and SH groups, a water molecule with a sodium ion 2.4 Å from its
oxygen (bonded by the 0.45 Å tolerance rule), and a bromine atom 3.1 Å from the chlorine (not
bonded). The bonds of the organic part are written with their orders (aromatic ring as 1/2).
"""
import math, os
out = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "molecules", "test.cml")
atoms, bonds = [], []
def add(el, p):
    atoms.append((el, p)); return len(atoms)
ring = []
for k in range(6):
    a = math.radians(60 * k)
    ring.append(add("C", (1.39 * math.cos(a), 1.39 * math.sin(a), 0.0)))
for k in range(6):
    bonds.append((ring[k], ring[(k + 1) % 6], 2 if k % 2 == 0 else 1))
def out_from(k, d):
    a = math.radians(60 * k); return ((1.39 + d) * math.cos(a), (1.39 + d) * math.sin(a), 0.0)
cl = add("Cl", out_from(0, 1.74)); bonds.append((ring[0], cl, 1))
for k in (1, 3, 5):
    bonds.append((ring[k], add("H", out_from(k, 1.08)), 1))
n = add("N", out_from(2, 1.40)); bonds.append((ring[2], n, 1))
nx, ny, _ = atoms[n - 1][1]
for s in (1, -1):
    bonds.append((n, add("H", (nx + 0.5 * math.cos(math.radians(120 + s * 60)), ny + 0.87, 0.3 * s)), 1))
sx = add("S", out_from(4, 1.77)); bonds.append((ring[4], sx, 1))
px, py, _ = atoms[sx - 1][1]
bonds.append((sx, add("H", (px - 0.6, py - 1.2, 0.0)), 1))
cx, cy, _ = atoms[cl - 1][1]
add("Br", (cx + 3.1, cy, 0.0))
o = add("O", (0.0, 0.0, 4.0))
bonds.append((o, add("H", (0.957, 0.0, 4.0)), 1)); bonds.append((o, add("H", (-0.24, 0.927, 4.0)), 1))
add("Na", (0.0, -2.4, 4.0))
with open(out, "w") as f:
    f.write('<?xml version="1.0"?>\n<molecule id="auraFX-test" xmlns="http://www.xml-cml.org/schema">\n <atomArray>\n')
    for i, (el, p) in enumerate(atoms, 1):
        f.write(f'  <atom id="a{i}" elementType="{el}" x3="{p[0]:.4f}" y3="{p[1]:.4f}" z3="{p[2]:.4f}"/>\n')
    f.write(' </atomArray>\n <bondArray>\n')
    for a, b, order in bonds:
        f.write(f'  <bond atomRefs2="a{a} a{b}" order="{order}"/>\n')
    f.write(' </bondArray>\n</molecule>\n')
print(out, len(atoms), "atoms", len(bonds), "bonds")
