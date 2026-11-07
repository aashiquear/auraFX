"""Tetrahedron quality reference values from ParaView's Mesh Quality filter (Verdict).

    (ulimit -v 8000000; timeout 300 pvpython scripts/tet_quality_reference.py)

24 tetrahedra: the regular one, perturbed regular ones (seeded), slivers, needles, caps,
wedges, flattened, inverted, scaled and moved ones. Writes testdata/mesh/tet_quality_reference.json:
the corner coordinates and, for every tetrahedron measure ParaView offers, the value per
element.
"""
import json, os
import numpy as np
from paraview import simple as pv
from vtkmodules.vtkCommonDataModel import vtkUnstructuredGrid, VTK_TETRA
from vtkmodules.vtkCommonCore import vtkPoints
from vtkmodules.util.numpy_support import vtk_to_numpy

rng = np.random.default_rng(11)
reg = np.array([[0, 0, 0], [1, 0, 0], [0.5, np.sqrt(3) / 2, 0], [0.5, np.sqrt(3) / 6, np.sqrt(2.0 / 3.0)]])
tets = [reg.copy()]
for s in (0.05, 0.1, 0.2, 0.3):
    for _ in range(3):
        tets.append(reg + rng.normal(0, s, (4, 3)))
tets.append(np.array([[0, 0, 0], [1, 0, 0], [0, 1, 0], [0, 0, 1.0]]))          # right-angle corner
tets.append(np.array([[0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0.05]]))         # sliver
tets.append(np.array([[0, 0, 0], [0.1, 0, 0], [0, 0.1, 0], [0.03, 0.03, 3.0]])) # needle
tets.append(np.array([[0, 0, 0], [1, 0, 0], [0.5, 0.9, 0], [0.5, 0.3, 0.05]])) # cap
tets.append(np.array([[0, 0, 0], [2, 0, 0], [1, 0.05, 0], [1, 0.5, 1.0]]))     # wedge
t = reg.copy(); t[3, 2] = 0.01; tets.append(t)                                  # flattened
t = reg.copy(); t[3, 2] = -0.4; tets.append(t)                                  # inverted
tets.append(reg * 3.5 + [10, -4, 2])                                            # scaled and moved
tets.append(reg[[0, 2, 1, 3]] * [1, 1, -1])                                     # mirrored order
tets.append(reg + rng.normal(0, 0.5, (4, 3)))
tets.append(reg * [4, 1, 1])                                                    # stretched

pts = vtkPoints()
ug = vtkUnstructuredGrid()
for tt in tets:
    ids = [pts.InsertNextPoint(*p) for p in tt]
    ug.InsertNextCell(VTK_TETRA, 4, ids)
ug.SetPoints(pts)
src = pv.TrivialProducer()
src.GetClientSideObject().SetOutput(ug)

measures = ["Aspect Frobenius", "Aspect Gamma", "Aspect Ratio", "Collapse Ratio", "Condition", "Distortion",
            "Edge Ratio", "Equiangle Skew", "Equivolume Skew", "Inradius", "Jacobian", "Mean Ratio",
            "Minimum Dihedral Angle", "Normalized Inradius", "Radius Ratio", "Relative Size Squared",
            "Scaled Jacobian", "Shape", "Shape and Size", "Squish Index", "Volume"]
out = {"tets": [t.tolist() for t in tets], "measures": {}}
for m in measures:
    q = pv.MeshQuality(Input=src)
    try:
        q.TetQualityMeasure = m
    except Exception as e:
        print("skip", m, e)
        continue
    q.UpdatePipeline()
    d = pv.servermanager.Fetch(q)
    out["measures"][m] = vtk_to_numpy(d.GetCellData().GetArray("Quality")).tolist()
path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "mesh", "tet_quality_reference.json")
with open(path, "w") as f:
    json.dump(out, f)
print(path, list(out["measures"]))
