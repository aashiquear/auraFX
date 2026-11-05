#!/usr/bin/env python3
"""Write curved higher-order test meshes (VTK XML, ASCII, float64 points):

* testdata/lagrange_annulus.vtu: a quarter of a hollow cylinder (radius 1 to 2, height 1)
  as four order-3 Lagrange hexahedra, with point scalar T = r^2 + z, point vector
  "radial" = (x, y, 0) and cell scalar "cell". Volume of the exact shape: 3*pi/4.
* testdata/bezier_annulus.vtu: the same quarter cylinder as one order-2 rational Bezier
  hexahedron, which is exact (RationalWeights 1, sqrt(1/2), 1 around the arc). T = r.
* testdata/lagrange_shapes.vtu: one curved cell of every other Lagrange shape (curve,
  triangle, quadrilateral, tetrahedron, wedge, order 3 or 4) bent onto a cylinder, with
  point scalar "u" = x + 2y + 3z and an order-(2, 2, 3) hexahedron that uses
  HigherOrderDegrees.

Node numbering is VTK's (checked against VTK in crates/aurafx-data/tests)."""
import math
import os

root = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata")


def tri_nodes(p):
    if p < 0:
        return []
    if p == 0:
        return [(0, 0)]
    v = [(0, 0), (p, 0), (0, p)]
    v += [(k, 0) for k in range(1, p)] + [(p - k, k) for k in range(1, p)] + [(0, p - k) for k in range(1, p)]
    return v + [(i + 1, j + 1) for (i, j) in tri_nodes(p - 3)]


def tet_nodes(p):
    if p < 0:
        return []
    if p == 0:
        return [(0, 0, 0)]
    c = [(0, 0, 0), (p, 0, 0), (0, p, 0), (0, 0, p)]
    v = list(c)
    for a, b in [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)]:
        v += [tuple((c[a][t] * (p - k) + c[b][t] * k) // p for t in range(3)) for k in range(1, p)]
    for f in [(0, 1, 3), (2, 3, 1), (0, 3, 2), (0, 2, 1)]:
        q = p - 3
        for (i, j) in tri_nodes(q):
            w = [1 + (q - i - j), 1 + i, 1 + j]
            v.append(tuple(sum(w[m] * c[f[m]][t] for m in range(3)) // p for t in range(3)))
    return v + [(i + 1, j + 1, k + 1) for (i, j, k) in tet_nodes(p - 4)]


def quad_nodes(p, q):
    v = [(0, 0), (p, 0), (p, q), (0, q)]
    v += [(k, 0) for k in range(1, p)] + [(p, k) for k in range(1, q)] + [(k, q) for k in range(1, p)] + [(0, k) for k in range(1, q)]
    return v + [(i, j) for j in range(1, q) for i in range(1, p)]


def hex_nodes(p, q, r):
    v = [(0, 0, 0), (p, 0, 0), (p, q, 0), (0, q, 0), (0, 0, r), (p, 0, r), (p, q, r), (0, q, r)]
    for z in (0, r):
        v += [(k, 0, z) for k in range(1, p)] + [(p, k, z) for k in range(1, q)] + [(k, q, z) for k in range(1, p)] + [(0, k, z) for k in range(1, q)]
    for (i, j) in [(0, 0), (p, 0), (p, q), (0, q)]:
        v += [(i, j, k) for k in range(1, r)]
    for x in (0, p):
        v += [(x, j, k) for k in range(1, r) for j in range(1, q)]
    for y in (0, q):
        v += [(i, y, k) for k in range(1, r) for i in range(1, p)]
    for z in (0, r):
        v += [(i, j, z) for j in range(1, q) for i in range(1, p)]
    return v + [(i, j, k) for k in range(1, r) for j in range(1, q) for i in range(1, p)]


def wedge_nodes(p, q):
    tri_int = [(i, j) for j in range(1, p) for i in range(1, p - j)]
    v = [(0, 0, 0), (p, 0, 0), (0, p, 0), (0, 0, q), (p, 0, q), (0, p, q)]
    for z in (0, q):
        v += [(k, 0, z) for k in range(1, p)] + [(p - k, k, z) for k in range(1, p)] + [(0, p - k, z) for k in range(1, p)]
    for (i, j) in [(0, 0), (p, 0), (0, p)]:
        v += [(i, j, k) for k in range(1, q)]
    for z in (0, q):
        v += [(i, j, z) for (i, j) in tri_int]
    v += [(i, 0, k) for k in range(1, q) for i in range(1, p)]
    v += [(p - i, i, k) for k in range(1, q) for i in range(1, p)]
    v += [(0, p - j, k) for k in range(1, q) for j in range(1, p)]
    return v + [(i, j, k) for k in range(1, q) for (i, j) in tri_int]


class Grid:
    def __init__(self):
        self.points, self.conn, self.offsets, self.types = [], [], [], []
        self.point_data, self.cell_data = {}, {}
        self.index = {}

    def point(self, key, xyz, **data):
        if key not in self.index:
            self.index[key] = len(self.points)
            self.points.append(xyz)
            for k, v in data.items():
                self.point_data.setdefault(k, []).append(v)
        return self.index[key]

    def cell(self, vtk_type, ids, **data):
        self.conn += ids
        self.offsets.append(len(self.conn))
        self.types.append(vtk_type)
        for k, v in data.items():
            self.cell_data.setdefault(k, []).append(v)

    def write(self, name, attrs="", cell_attrs=""):
        def arr(name, values, typ="Float64"):
            flat = [x for v in values for x in (v if isinstance(v, (tuple, list)) else (v,))]
            nc = len(values[0]) if isinstance(values[0], (tuple, list)) else 1
            body = " ".join(f"{x:.17g}" if typ.startswith("Float") else str(x) for x in flat)
            return f'<DataArray type="{typ}" Name="{name}" NumberOfComponents="{nc}" format="ascii">{body}</DataArray>\n'

        pd = "".join(arr(k, v) for k, v in self.point_data.items())
        cd = "".join(arr(k, v) for k, v in self.cell_data.items())
        with open(os.path.join(root, name), "w") as f:
            f.write('<?xml version="1.0"?>\n<VTKFile type="UnstructuredGrid" version="2.2" byte_order="LittleEndian">\n<UnstructuredGrid>\n')
            f.write(f'<Piece NumberOfPoints="{len(self.points)}" NumberOfCells="{len(self.types)}">\n')
            f.write(f"<PointData{attrs}>\n{pd}</PointData>\n<CellData{cell_attrs}>\n{cd}</CellData>\n")
            f.write("<Points>\n" + arr("Points", self.points) + "</Points>\n<Cells>\n")
            f.write(arr("connectivity", self.conn, "Int64") + arr("offsets", self.offsets, "Int64") + arr("types", self.types, "UInt8"))
            f.write("</Cells>\n</Piece>\n</UnstructuredGrid>\n</VTKFile>\n")
        print("wrote", name, len(self.points), "points", len(self.types), "cells")


# Lagrange annulus: i -> radius, j -> angle, k -> height (right-handed).
P, NTH = 3, 4
g = Grid()
for c in range(NTH):
    ids = []
    for (i, j, k) in hex_nodes(P, P, P):
        gj = c * P + j
        th = (math.pi / 2) * gj / (NTH * P)
        r, z = 1 + i / P, k / P
        x, y = r * math.cos(th), r * math.sin(th)
        ids.append(g.point((i, gj, k), (x, y, z), T=r * r + z, radial=(x, y, 0.0)))
    g.cell(72, ids, cell=float(c))
g.write("lagrange_annulus.vtu", ' Scalars="T"')

# Rational Bezier annulus (one cell, exact).
g = Grid()
ids = []
for (i, j, k) in hex_nodes(2, 2, 2):
    r, z = 1 + i / 2, k / 2
    cp = [(r, 0.0), (r, r), (0.0, r)][j]
    ids.append(g.point((i, j, k), (cp[0], cp[1], z), RationalWeights=[1.0, math.sqrt(0.5), 1.0][j], T=r))
g.cell(79, ids, cell=0.0)
g.write("bezier_annulus.vtu", ' Scalars="T" RationalWeights="RationalWeights"')


# Every other shape, bent around the z axis: (a, b, c) -> radius 2 + b, angle a / 2, height -c
# (right-handed, so the 3D cells have positive volume).
def bend(a, b, c):
    r = 2.0 + b
    return (r * math.cos(a / 2), r * math.sin(a / 2), -c)


g = Grid()
n = [0]
share = [False]


def add(vtk_type, nodes, place, degrees=None):
    ids = []
    for nd in nodes:
        x = bend(*place(nd))
        key = tuple(round(v, 9) for v in x) if share[0] else ("c", n[0], nd)
        ids.append(g.point(key, x, u=x[0] + 2 * x[1] + 3 * x[2]))
    data = {"cell": float(n[0])}
    if degrees:
        data["HigherOrderDegrees"] = degrees
    g.cell(vtk_type, ids, **data)
    n[0] += 1


add(68, [(i,) for i in [0, 4, 1, 2, 3]], lambda nd: (nd[0] / 4, -0.5, 0.0))
add(69, tri_nodes(3), lambda nd: (nd[0] / 3, nd[1] / 3, -1.5))
add(70, quad_nodes(3, 3), lambda nd: (nd[0] / 3, nd[1] / 3, 2.5))
add(71, tet_nodes(3), lambda nd: (1.5 + nd[0] / 3, nd[1] / 3, nd[2] / 3))
add(73, wedge_nodes(4, 2), lambda nd: (3.0 + nd[0] / 4, nd[1] / 4, nd[2] / 2))
add(72, hex_nodes(2, 2, 3), lambda nd: (4.5 + nd[0] / 2, nd[1] / 2, nd[2] / 3), degrees=(2.0, 2.0, 3.0))
# All cells need the same array set; the degrees of the others are their orders.
hod = [(4.0, 0.0, 0.0), (3.0, 3.0, 0.0), (3.0, 3.0, 0.0), (3.0, 3.0, 3.0), (4.0, 4.0, 2.0), (2.0, 2.0, 3.0)]
g.cell_data["HigherOrderDegrees"] = hod
g.write("lagrange_shapes.vtu", cell_attrs=' HigherOrderDegrees="HigherOrderDegrees"')

# Quadratic (serendipity and partial) VTK cells, bent like the shapes above: two 20-node
# hexahedra, a 15-node wedge, an 8-node quad, a 6-node quadratic-linear quad, a 12-node
# quadratic-linear wedge and a cubic line. Nodes are given as parametric positions x2.
HEX20 = [(0, 0, 0), (2, 0, 0), (2, 2, 0), (0, 2, 0), (0, 0, 2), (2, 0, 2), (2, 2, 2), (0, 2, 2),
         (1, 0, 0), (2, 1, 0), (1, 2, 0), (0, 1, 0), (1, 0, 2), (2, 1, 2), (1, 2, 2), (0, 1, 2),
         (0, 0, 1), (2, 0, 1), (2, 2, 1), (0, 2, 1)]
WEDGE15 = [(0, 0, 0), (2, 0, 0), (0, 2, 0), (0, 0, 2), (2, 0, 2), (0, 2, 2),
           (1, 0, 0), (1, 1, 0), (0, 1, 0), (1, 0, 2), (1, 1, 2), (0, 1, 2), (0, 0, 1), (2, 0, 1), (0, 2, 1)]
QUAD8 = [(0, 0), (2, 0), (2, 2), (0, 2), (1, 0), (2, 1), (1, 2), (0, 1)]
QLQUAD = [(0, 0), (2, 0), (2, 2), (0, 2), (1, 0), (1, 2)]  # quadratic along r, linear along s
QLWEDGE = [(0, 0, 0), (2, 0, 0), (0, 2, 0), (0, 0, 2), (2, 0, 2), (0, 2, 2), (1, 0, 0), (1, 1, 0), (0, 1, 0), (1, 0, 2), (1, 1, 2), (0, 1, 2)]

g = Grid()
n[0] = 0
share[0] = True  # the two hexahedra share their common face
add(25, HEX20, lambda nd: (nd[0] / 2 * 1.5, nd[1] / 2, nd[2] / 2))
add(25, HEX20, lambda nd: (1.5 + nd[0] / 2 * 1.5, nd[1] / 2, nd[2] / 2))
add(26, WEDGE15, lambda nd: (3.5 + nd[0] / 2, nd[1] / 2, nd[2] / 2))
add(23, QUAD8, lambda nd: (nd[0] / 2 * 3.0, nd[1] / 2, 1.5))
add(30, QLQUAD, lambda nd: (nd[0] / 2 * 3.0, nd[1] / 2, -1.0))
add(31, QLWEDGE, lambda nd: (5.0 + nd[0] / 2, nd[1] / 2, nd[2] / 2))
add(35, [(i,) for i in [0, 3, 1, 2]], lambda nd: (nd[0] / 3 * 3.0, -0.5, 2.0))
g.write("quadratic_shapes.vtu")
