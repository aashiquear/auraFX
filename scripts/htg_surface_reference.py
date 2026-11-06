"""ParaView 6.1.1's Hyper Tree Grid Geometry and Feature Edges (testdata/htg/surface_reference.json).

    (ulimit -v 8000000; timeout 180 pvpython scripts/htg_surface_reference.py)

For each grid and filter: every output cell in order as its point coordinates, and the Depth
value it carries.
"""
import json, os
from paraview.simple import *
from vtk.util.numpy_support import vtk_to_numpy as v2n

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "htg")
def source(dims, depth, desc, mask=None):
    s = HyperTreeGrid(); s.Dimensions = dims; s.MaxDepth = depth; s.Descriptor = desc
    if mask:
        s.UseMask = 1; s.Mask = mask
    return s
def random(dims, seed, depth, split, frac):
    r = HyperTreeGridRandom(); r.Dimensions = dims; r.Seed = seed; r.MaxDepth = depth; r.SplitFraction = split; r.MaskedFraction = frac
    return r
inputs = {
    "tree3d.htg": OpenDataFile(os.path.join(td, "tree3d.htg")),
    "tree2d.htg": OpenDataFile(os.path.join(td, "tree2d.htg")),
    "tree3d_mask.htg": OpenDataFile(os.path.join(td, "tree3d_mask.htg")),
    "line": source([6, 1, 1], 3, "R.R.R | .R.... | .."),
    "plane_mask": source([3, 3, 1], 3, "R.R. | .R...... | ....", "1111 | 01111111 | 1101"),
    "box_mask2": source([3, 3, 3], 3, "R......R | .......R ........ | ........", "11101111 | 11111111 11011111 | 11111110"),
    "random_mask3d": random([3, 3, 3], 5, 3, 0.5, 0.4),
    "random_mask2d": random([5, 5, 1], 3, 3, 0.5, 0.4),
}
first = True
out = {}
for name, src in inputs.items():
    for fname, F in (("geometry", HyperTreeGridGeometryFilter), ("edges", HyperTreeGridFeatureEdgesFilter)):
        f = F(Input=src)
        if first:
            print(fname, {p: f.GetPropertyValue(p) for p in f.ListProperties() if p != "Input"})
        d = servermanager.Fetch(f)
        depth = d.GetCellData().GetArray("Depth")
        cells = []
        for c in range(d.GetNumberOfCells()):
            ids = d.GetCell(c).GetPointIds()
            cells.append([[list(d.GetPoint(ids.GetId(k))) for k in range(ids.GetNumberOfIds())], depth.GetValue(c) if depth else None, d.GetCellType(c)])
        out[f"{name}#{fname}"] = {"points": d.GetNumberOfPoints(), "cells": cells, "arrays": [d.GetCellData().GetArrayName(i) for i in range(d.GetCellData().GetNumberOfArrays())]}
        print(name, fname, d.GetNumberOfPoints(), d.GetNumberOfCells(), out[f"{name}#{fname}"]["arrays"])
    first = False
json.dump(out, open(os.path.join(td, "surface_reference.json"), "w"))
