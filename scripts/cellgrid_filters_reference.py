"""ParaView 6.1.1's Cell Grid Transform, Cell Grid Warp and Convert To Cell Grid
(testdata/cellgrid/filters_reference.json), each seen through Cell Grid To Unstructured Grid.

    (ulimit -v 8000000; timeout 180 pvpython scripts/cellgrid_filters_reference.py)

For each case: the corner points and every point array of the unstructured grid. The mesh
given to Convert To Cell Grid (a tetrahedralized wavelet with point and cell ids) is saved as
convert_input.vtu.
"""
import json, os
from paraview.simple import *
from vtk.util.numpy_support import vtk_to_numpy as v2n

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "cellgrid")
def dump(src):
    u = servermanager.Fetch(CellGridToUnstructuredGrid(Input=src))
    while not u.IsA("vtkDataSet"):  # a partitioned collection from Convert To Cell Grid
        u = u.GetPartitionAsDataObject(0, 0) if u.IsA("vtkPartitionedDataSetCollection") else u.GetBlock(0)
    pd = u.GetPointData()
    return {"cells": u.GetNumberOfCells(), "points": v2n(u.GetPoints().GetData()).tolist(),
            "arrays": {pd.GetArrayName(i): v2n(pd.GetArray(i)).reshape(u.GetNumberOfPoints(), -1).tolist() for i in range(pd.GetNumberOfArrays())}}
out = {}
r = CompositeCellGridReader(FileName=os.path.join(td, "dgHexahedra.dg"))
t = CellGridTransform(Input=r)
t.Transform.Translate = [0.5, -1, 2]
t.Transform.Rotate = [30, 40, 50]
t.Transform.Scale = [2, 3, 0.5]
out["transform"] = dump(t)
s = CellGridSource(CellType="vtkDGHex")
w = CellGridWarp(Input=s, DeformationAttribute=["CELLS", "hgrad"], DeformationScale=0.5)
out["warp"] = dump(w)
wv = Wavelet(WholeExtent=[0, 2, 0, 1, 0, 1])
ids = PointAndCellIds(Input=Tetrahedralize(Input=wv))
SaveData(os.path.join(td, "convert_input.vtu"), proxy=ids)  # the mesh converted (for the test)
c = ConvertToCellGrid(Input=ids)
c.UpdatePipeline()
out["convert"] = dump(c)
for k, v in out.items():
    print(k, v["cells"], len(v["points"]), list(v["arrays"]))
json.dump(out, open(os.path.join(td, "filters_reference.json"), "w"))
