#!/usr/bin/env python3
"""Read a STEP (or IGES) file with an independent reader (OpenCASCADE through the cadquery-ocp wheel) and
print its solid count, face count and volume as JSON. Used by the aurafx-cad tests when
AURAFX_OCP_PYTHON points to a Python that has OCP (pip install cadquery-ocp)."""
import json, sys
from OCP.IFSelect import IFSelect_RetDone
from OCP.GProp import GProp_GProps
from OCP.BRepGProp import BRepGProp
from OCP.TopExp import TopExp_Explorer
from OCP.TopAbs import TopAbs_SOLID, TopAbs_FACE

if sys.argv[1].lower().endswith(("igs", "iges")):
    from OCP.IGESControl import IGESControl_Reader as STEPControl_Reader
else:
    from OCP.STEPControl import STEPControl_Reader
r = STEPControl_Reader()
if r.ReadFile(sys.argv[1]) != IFSelect_RetDone:
    print(json.dumps({"error": "could not read the file"})); sys.exit(1)
r.TransferRoots()
shape = r.OneShape()
props = GProp_GProps()
BRepGProp.VolumeProperties_s(shape, props)
def count(kind):
    e, n = TopExp_Explorer(shape, kind), 0
    while e.More():
        n += 1; e.Next()
    return n
print(json.dumps({"solids": count(TopAbs_SOLID), "faces": count(TopAbs_FACE), "volume": props.Mass()}))
