"""Hexahedron quality reference values from ParaView's Mesh Quality filter (Verdict).

    (ulimit -v 8000000; timeout 300 pvpython scripts/hex_quality_reference.py)

24 hexahedra: perturbed unit cubes (seeded), sheared, tapered, stretched, warped and partly
inverted ones. Writes testdata/mesh/hex_quality_reference.json: the corner coordinates and,
for every hexahedron measure ParaView offers, the value per element.
"""
import json, os
import numpy as np
from paraview import simple as pv
from vtkmodules.vtkCommonDataModel import vtkUnstructuredGrid, VTK_HEXAHEDRON
from vtkmodules.vtkCommonCore import vtkPoints
from vtkmodules.util.numpy_support import vtk_to_numpy

rng = np.random.default_rng(5)
cube = np.array([[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], float)
hexes = []
for s in (0.05, 0.1, 0.2, 0.3):
    for _ in range(3):
        hexes.append(cube + rng.normal(0, s, (8, 3)))
hexes.append(cube * [3.0, 1.0, 0.5])                             # stretched box
hexes.append(cube + np.outer(cube[:, 2], [0.6, 0.0, 0.0]))       # sheared
hexes.append(np.where(cube[:, 2:3] > 0, cube * [0.5, 0.5, 1] + [0.25, 0.25, 0], cube))  # tapered
w = cube.copy(); w[6, 2] += 0.5; hexes.append(w)                 # warped top
w = cube.copy(); w[6] = [0.3, 0.3, 0.3]; hexes.append(w)         # corner pushed in
w = cube.copy(); w[6] = [-0.2, -0.2, -0.2]; hexes.append(w)      # corner inverted
w = cube.copy(); w[[4, 5, 6, 7], 2] = -0.1; hexes.append(w)      # flattened through itself
w = cube.copy() * 2.5 + [10, -3, 7]; hexes.append(w)             # scaled and moved cube
rot = np.array([[0.8, -0.6, 0], [0.6, 0.8, 0], [0, 0, 1.0]])
hexes.append(cube @ rot.T * [1.0, 2.0, 1.0])
hexes.append(cube + rng.normal(0, 0.4, (8, 3)))
hexes.append(cube * [1, 1, 0.02])                                 # very thin
hexes.append(np.array([[0, 0, 0], [2, 0, 0], [2, 1, 0], [0, 1, 0], [0, 0, 1], [1, 0, 1], [1, 1, 1], [0, 1, 1]], float))

pts = vtkPoints()
ug = vtkUnstructuredGrid()
for hx in hexes:
    ids = [pts.InsertNextPoint(*p) for p in hx]
    ug.InsertNextCell(VTK_HEXAHEDRON, 8, ids)
ug.SetPoints(pts)
src = pv.TrivialProducer()
src.GetClientSideObject().SetOutput(ug)

measures = ["Diagonal", "Dimension", "Distortion", "Edge Ratio", "Jacobian", "Maximum Aspect Frobenius", "Maximum Edge Ratio",
            "Mean Aspect Frobenius", "Oddy", "Relative Size Squared", "Scaled Jacobian", "Shape", "Shape and Size",
            "Shear", "Shear and Size", "Skew", "Stretch", "Taper", "Volume"]
out = {"hexes": [h.tolist() for h in hexes], "measures": {}}
for m in measures:
    q = pv.MeshQuality(Input=src)
    try:
        q.HexQualityMeasure = m
    except Exception as e:
        print("skip", m, e)
        continue
    q.UpdatePipeline()
    d = pv.servermanager.Fetch(q)
    out["measures"][m] = vtk_to_numpy(d.GetCellData().GetArray("Quality")).tolist()
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "mesh", "hex_quality_reference.json")
with open(path, "w") as f:
    json.dump(out, f)
print(path, list(out["measures"]))
