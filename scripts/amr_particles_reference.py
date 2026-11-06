"""VTK's FLASH and Enzo particle readers (as in ParaView 6.1.1) on the particle test files
(testdata/amr/particles_reference.json).

    (ulimit -v 8000000; timeout 180 pvpython scripts/amr_particles_reference.py)

The readers are driven directly (the ParaView proxies list no particle arrays to enable). For
each file: the number of output blocks and, per block with particles, the points and every
array (type and values). Enzo is read with ParticleType 0 (ParaView's "All Particles").
"""
import json, os
from vtkmodules.vtkIOAMR import vtkAMRFlashParticlesReader, vtkAMREnzoParticlesReader
from vtk.util.numpy_support import vtk_to_numpy as v2n

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "amr")
out = {}
for R, f in ((vtkAMRFlashParticlesReader, "flash3d_hdf5_part_0007"), (vtkAMRFlashParticlesReader, "flash3d_ffv7_hdf5_part_0007"), (vtkAMREnzoParticlesReader, "enzo/DD0004/data0004.hierarchy")):
    r = R()
    r.SetFileName(os.path.join(td, f))
    r.Update()
    if hasattr(r, "SetParticleType"):
        r.SetParticleType(0)
    r.GetParticleDataArraySelection().EnableAllArrays()
    r.Update()
    o = r.GetOutput()
    blocks = {}
    for b in range(o.GetNumberOfBlocks()):
        p = o.GetBlock(b)
        if p is None or p.GetNumberOfPoints() == 0:
            continue
        pd = p.GetPointData()
        blocks[b] = {"points": v2n(p.GetPoints().GetData()).ravel().tolist(),
                     "arrays": {pd.GetArrayName(i): [pd.GetArray(i).GetClassName(), v2n(pd.GetArray(i)).astype(float).ravel().tolist()] for i in range(pd.GetNumberOfArrays())}}
    out[f] = {"blocks": o.GetNumberOfBlocks(), "data": blocks}
    print(f, o.GetNumberOfBlocks(), list(blocks))
# Chombo particles through ParaView's VisIt Chombo reader (the particles mesh alone).
from paraview.simple import VisItChomboReader
r = VisItChomboReader(FileName=os.path.join(td, "chombo2d_particles.hdf5"))
r.MeshStatus = ["particles"]
r.PointArrayStatus = ["mass", "particle_id"]
r.UpdatePipeline()
o = r.GetClientSideObject().GetOutputDataObject(0)
while o.IsA("vtkMultiBlockDataSet"):
    o = [o.GetBlock(i) for i in range(o.GetNumberOfBlocks()) if o.GetBlock(i) is not None][-1]
pd = o.GetPointData()
out["chombo2d_particles.hdf5"] = {"blocks": 1, "data": {"0": {"points": v2n(o.GetPoints().GetData()).ravel().tolist(),
    "arrays": {pd.GetArrayName(i): [pd.GetArray(i).GetClassName(), v2n(pd.GetArray(i)).astype(float).ravel().tolist()] for i in range(pd.GetNumberOfArrays())}}}}
print("chombo2d_particles.hdf5", o.GetClassName(), o.GetNumberOfPoints())
json.dump(out, open(os.path.join(td, "particles_reference.json"), "w"))
