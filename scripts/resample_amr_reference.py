"""ParaView 6.1.1's Resample AMR (testdata/amr/resample_reference.json).

    (ulimit -v 8000000; timeout 180 pvpython scripts/resample_amr_reference.py)

Demand-driven mode off (ParaView's default needs reader metadata and gives nothing on a
source). For each case: the output grid's origin, spacing, dimensions and every value of every
array (point data, or cell data when TransferToNodes is off).
"""
import json, os
from paraview.simple import AMRGaussianPulseSource, ResampleAMR, FlashReader
from vtk.util.numpy_support import vtk_to_numpy as v2n

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "amr")
def pulse(dim):
    s = AMRGaussianPulseSource()
    s.Dimension = dim
    return s
def flash():
    r = FlashReader(FileNames=[os.path.join(td, "flash3d_hdf5_plt_cnt_0007")])
    r.Level = 10
    r.CellArrayStatus = ["dens", "pres", "velx"]
    return r
cases = {
    "pulse3d": (lambda: pulse(3), [10, 10, 10], None, 1),
    "pulse3d_box": (lambda: pulse(3), [13, 7, 5], ([-2.5, -1.0, 0.3], [0.2, 0.9, 3.0]), 1),
    "pulse3d_cells": (lambda: pulse(3), [9, 8, 7], None, 0),
    "pulse2d": (lambda: pulse(2), [12, 9, 1], None, 1),
    "flash3d": (flash, [11, 6, 6], None, 1),
}
out = {}
for key, (make, n, region, nodes) in cases.items():
    src = make()
    src.UpdatePipeline()
    b = src.GetDataInformation().GetBounds()
    lo, hi = region if region else ([b[0], b[2], b[4]], [b[1], b[3], b[5]])
    r = ResampleAMR(Input=src, NumberofSamples=n, Min=lo, Max=hi, DemandDrivenMode=0, TransferToNodes=nodes)
    r.UpdatePipeline()
    g = r.GetClientSideObject().GetOutputDataObject(0).GetBlock(0)
    d = g.GetPointData() if nodes else g.GetCellData()
    arrays = {d.GetArrayName(i): v2n(d.GetArray(i)).astype(float).ravel().tolist() for i in range(d.GetNumberOfArrays())}
    out[key] = {"samples": n, "region": [lo, hi] if region else None, "nodes": nodes, "origin": list(g.GetOrigin()), "spacing": list(g.GetSpacing()), "dims": list(g.GetDimensions()), "arrays": arrays}
    print(key, g.GetDimensions(), list(arrays))
json.dump(out, open(os.path.join(td, "resample_reference.json"), "w"))
