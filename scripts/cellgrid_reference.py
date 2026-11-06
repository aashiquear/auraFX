"""ParaView's reading of testdata/cellgrid/*.dg (written to testdata/cellgrid/paraview_reference.json).

    pvpython scripts/cellgrid_reference.py

For every file: Cell Grid To Unstructured Grid (point coordinates and every point array), and
Cell Grid Centers followed by To Unstructured Grid (the centers and the values there).
"""
import glob, json, os
from paraview.simple import CompositeCellGridReader, CellGridToUnstructuredGrid, CellGridCenters
from vtk.util.numpy_support import vtk_to_numpy as v2n

here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "cellgrid")

def first(o):
    if o.IsA("vtkDataSet"):
        return o
    it = o.NewIterator(); it.InitTraversal()
    return it.GetCurrentDataObject()

def dump(o):
    o = first(o)
    pd = o.GetPointData()
    arrays = {}
    for i in range(pd.GetNumberOfArrays()):
        a = pd.GetArray(i)
        if a is not None:
            arrays[a.GetName()] = v2n(a).reshape(a.GetNumberOfTuples(), -1).tolist()
    return {"cells": o.GetNumberOfCells(), "points": v2n(o.GetPoints().GetData()).tolist() if o.GetNumberOfPoints() else [], "arrays": arrays}

out = {}
for f in sorted(glob.glob(os.path.join(here, "*.dg"))):
    r = CompositeCellGridReader(FileName=f)
    u = CellGridToUnstructuredGrid(Input=r); u.UpdatePipeline()
    c = CellGridToUnstructuredGrid(Input=CellGridCenters(Input=r)); c.UpdatePipeline()
    out[os.path.basename(f)] = {"ug": dump(u.GetClientSideObject().GetOutputDataObject(0)), "centers": dump(c.GetClientSideObject().GetOutputDataObject(0))}
json.dump(out, open(os.path.join(here, "paraview_reference.json"), "w"))
print({k: (v["ug"]["cells"], len(v["ug"]["points"]), len(v["centers"]["points"])) for k, v in out.items()})
