"""ParaView 6.1.1 reference for Hyper Tree Grid To Dual Grid (testdata/htg/dual_reference.json).

    (ulimit -v 8000000; timeout 180 pvpython scripts/htg_dual_reference.py)

For each case: the dual cells in order, each as the coordinates of its points and the Depth
value at each point. ParaView keeps a point for every cell of the grid (unused ones at the
origin), so cells are compared by coordinates, not point ids.
"""
import json, os
from paraview.simple import *
here = os.path.dirname(os.path.abspath(__file__))
td = os.path.join(here, "..", "testdata", "htg")
cases = {}
def source(dims, depth, desc, mask=None, bf=2):
    s = HyperTreeGrid(); s.Dimensions = dims; s.MaxDepth = depth; s.Descriptor = desc; s.BranchFactor = bf
    if mask:
        s.UseMask = 1; s.Mask = mask
    return s
inputs = {
    "tree3d.htg": OpenDataFile(os.path.join(td, "tree3d.htg")),
    "tree2d.htg": OpenDataFile(os.path.join(td, "tree2d.htg")),
    "tree3d_mask.htg": OpenDataFile(os.path.join(td, "tree3d_mask.htg")),
    "line": source([6, 1, 1], 3, "R.R.R | .R.... | .."),
    "plane": source([4, 3, 1], 3, "R..R.. | .R.. R... | ........"),
    "plane_mask": source([3, 3, 1], 3, "R.R. | .R...... | ....", "1111 | 01111111 | 1101"),
    "plane_mask2": source([4, 4, 1], 2, "R...R.... | ........", "111011111 | 11111011"),
    "box_mask": source([3, 3, 3], 2, "R....R.. | ................", "10111111 | 1111111111111110"),
    "box_mask2": source([3, 3, 3], 3, "R......R | .......R ........ | ........", "11101111 | 11111111 11011111 | 11111110"),
}
def random(dims, seed, depth, split, frac):
    r = HyperTreeGridRandom(); r.Dimensions = dims; r.Seed = seed; r.MaxDepth = depth; r.SplitFraction = split; r.MaskedFraction = frac
    return r
inputs["random_mask2d"] = random([5, 5, 1], 3, 3, 0.5, 0.4)
inputs["random_mask3d"] = random([3, 3, 3], 5, 3, 0.5, 0.4)
for name, src in inputs.items():
    d = servermanager.Fetch(HyperTreeGridToDualGrid(Input=src))
    depth = d.GetPointData().GetArray("Depth")
    cells = []
    for c in range(d.GetNumberOfCells()):
        ids = d.GetCell(c).GetPointIds()
        pts = [ids.GetId(k) for k in range(ids.GetNumberOfIds())]
        cells.append({"type": d.GetCellType(c), "points": [list(d.GetPoint(p)) for p in pts], "depth": [depth.GetValue(p) for p in pts] if depth else []})
    cases[name] = {"points": d.GetNumberOfPoints(), "cells": cells}
    print(name, d.GetNumberOfPoints(), d.GetNumberOfCells())
json.dump(cases, open(os.path.join(td, "dual_reference.json"), "w"))
