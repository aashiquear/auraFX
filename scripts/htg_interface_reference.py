"""Hyper tree grids with material interfaces, and ParaView 6.1.1's Hyper Tree Grid Geometry on
them (testdata/htg/interface_*.htg and testdata/htg/interface_reference.json).

    (ulimit -v 8000000; timeout 300 pvpython scripts/htg_interface_reference.py)

Each grid (VTK's descriptor and random sources, and tree2d.htg with branch factor 3) gets three
cell arrays: "gid" (the cell's global index), "Normals" and "Intercepts" (declared as the
grid's interface arrays). A cell crossing a spherical shell (radii R1 < R around a center) gets
an interface (the shell is centered near the middle of the grid, radii 0.35 and 0.15 of its
largest side): crossing only the outer sphere, type -1 (one plane, material inside); only the
inner one, type 1 (material outside); both, type 0 (the shell between two planes), every third
of those with its two intercepts swapped. Some crossing cells get a zero normal (VTK then
treats them as pure). Other cells are pure (type 2 or 3). The grid is saved with ParaView's
writer, read back, and vtkHyperTreeGridGeometry is run with Fill Material on and off: the
output points (12 significant digits) and every cell's point ids in order, its type and gid.
"""
import json, math, os
import paraview.simple  # noqa: F401
from vtkmodules.vtkCommonCore import vtkDoubleArray
from vtkmodules.vtkCommonDataModel import vtkHyperTreeGrid, vtkHyperTreeGridNonOrientedGeometryCursor
from vtkmodules.vtkFiltersSources import vtkHyperTreeGridSource, vtkRandomHyperTreeGridSource
from vtkmodules.vtkFiltersHyperTree import vtkHyperTreeGridGeometry
from vtkmodules.vtkIOXML import vtkXMLHyperTreeGridWriter, vtkXMLHyperTreeGridReader

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "htg")


def descriptor(dims, depth, desc, mask=None):
    s = vtkHyperTreeGridSource()
    s.SetDimensions(*dims)
    s.SetMaxDepth(depth)
    s.SetDescriptor(desc)
    if mask:
        s.SetUseMask(1)
        s.SetMask(mask)
    s.Update()
    return s.GetOutput()


def random(dims, seed, depth, split, frac):
    r = vtkRandomHyperTreeGridSource()
    r.SetDimensions(*dims)
    r.SetSeed(seed)
    r.SetMaxDepth(depth)
    r.SetSplitFraction(split)
    r.SetMaskedFraction(frac)
    r.Update()
    return r.GetOutput()


def read(path):
    r = vtkXMLHyperTreeGridReader()
    r.SetFileName(path)
    r.Update()
    return r.GetOutput()


# name: grid. The shell is centered a little off the middle of the grid's bounds, with radii
# 0.35 and 0.15 of its largest side.
GRIDS = {
    "line": lambda: descriptor([6, 1, 1], 3, "R.R.R | .R.... | .."),
    "plane": lambda: random([5, 5, 1], 3, 4, 0.6, 0.0),
    "plane_mask": lambda: descriptor([3, 3, 1], 3, "R.R. | .R...... | ....", "1111 | 01111111 | 1101"),
    "plane_bf3": lambda: read(os.path.join(td, "tree2d.htg")),
    "box": lambda: random([3, 3, 3], 5, 3, 0.5, 0.0),
    "box_mask": lambda: random([3, 3, 3], 7, 3, 0.5, 0.3),
}


def shell(h):
    b = h.GetBounds()
    big = max(b[1] - b[0], b[3] - b[2], b[5] - b[4])
    c = [0.5 * (b[2 * k] + b[2 * k + 1]) + (0.07 * big if b[2 * k + 1] > b[2 * k] else 0.0) for k in range(3)]
    return c, 0.35 * big, 0.15 * big


def with_interfaces(src, c, R, R1):
    h = vtkHyperTreeGrid()
    h.DeepCopy(src)
    n_ids = h.GetNumberOfCells()
    gid, normals, inter = vtkDoubleArray(), vtkDoubleArray(), vtkDoubleArray()
    gid.SetName("gid")
    normals.SetName("Normals")
    normals.SetNumberOfComponents(3)
    inter.SetName("Intercepts")
    inter.SetNumberOfComponents(3)
    for a in (gid, normals, inter):
        a.SetNumberOfTuples(n_ids)
    cur = vtkHyperTreeGridNonOrientedGeometryCursor()
    count = [0]

    def visit(cur):
        g = cur.GetGlobalNodeIndex()
        b = [0.0] * 6
        cur.GetBounds(b)
        p = [0.0] * 3
        cur.GetPoint(p)
        hd = 0.5 * math.sqrt(sum((b[2 * k + 1] - b[2 * k]) ** 2 for k in range(3)))
        r = math.sqrt(sum((p[k] - c[k]) ** 2 for k in range(3)))
        gid.SetValue(g, g)
        outer, inner = abs(r - R) < hd, abs(r - R1) < hd
        if r == 0 or not (outer or inner):
            normals.SetTuple3(g, 0, 0, 0)
            inter.SetTuple3(g, 0, 0, 2 + g % 2)
        else:
            n = [(p[k] - c[k]) / r for k in range(3)]
            nc = sum(n[k] * c[k] for k in range(3))
            count[0] += 1
            if outer and inner:
                d1, d2 = -(nc + R), -(nc + R1)
                if count[0] % 3 == 0:
                    d1, d2 = d2, d1
                normals.SetTuple3(g, *n)
                inter.SetTuple3(g, d1, d2, 0)
            elif outer:
                normals.SetTuple3(g, *[-x for x in n])
                inter.SetTuple3(g, nc + R, 0, -1)
            else:
                normals.SetTuple3(g, *[-x for x in n])
                inter.SetTuple3(g, 0, nc + R1, 1)
            if count[0] % 11 == 0:
                normals.SetTuple3(g, 0, 0, 0)
        if not cur.IsLeaf():
            for k in range(cur.GetNumberOfChildren()):
                cur.ToChild(k)
                visit(cur)
                cur.ToParent()

    for t in range(h.GetMaxNumberOfTrees()):
        if h.GetTree(t) is None:
            continue
        h.InitializeNonOrientedGeometryCursor(cur, t)
        visit(cur)
    for a in (gid, normals, inter):
        h.GetCellData().AddArray(a)
    h.SetHasInterface(1)
    h.SetInterfaceNormalsName("Normals")
    h.SetInterfaceInterceptsName("Intercepts")
    return h


out = {}
for name, make in GRIDS.items():
    src = make()
    h = with_interfaces(src, *shell(src))
    path = os.path.join(td, f"interface_{name}.htg")
    w = vtkXMLHyperTreeGridWriter()
    w.SetInputData(h)
    w.SetFileName(path)
    w.Write()
    back = read(path)
    assert back.GetHasInterface(), name
    # (In 3-D Fill Material changes nothing; the output is kept once.)
    for fill in ((1, 0) if back.GetDimension() < 3 else (1,)):
        f = vtkHyperTreeGridGeometry()
        f.SetInputData(back)
        f.SetFillMaterial(fill)
        f.Update()
        d = f.GetOutput()
        g = d.GetCellData().GetArray("gid")
        cells = []
        for i in range(d.GetNumberOfCells()):
            ids = d.GetCell(i).GetPointIds()
            cells.append([[ids.GetId(k) for k in range(ids.GetNumberOfIds())], int(g.GetValue(i)), d.GetCellType(i)])
        pts = [[float("%.12g" % x) for x in d.GetPoint(i)] for i in range(d.GetNumberOfPoints())]
        out[f"{name}#{fill}"] = {"points": pts, "cells": cells}
        print(name, "fill" if fill else "no fill", d.GetNumberOfPoints(), d.GetNumberOfCells())
json.dump(out, open(os.path.join(td, "interface_reference.json"), "w"))
