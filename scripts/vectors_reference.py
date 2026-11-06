"""ParaView's GDAL vector reader on testdata/vectors (written to testdata/vectors/paraview_reference.json).

    pvpython scripts/vectors_reference.py

For every file and layer: ParaView's blocks (one per feature, its default) with each block's
point coordinates, vertex and line sizes and cell attributes. ParaView reads one layer at a time
(ActiveLayer); KML is not readable by its GDAL build. ParaView cannot pick a file's last layer
alone: that index (world.gdb#5) reads every layer.
"""
import json, os
from paraview.simple import GDALVectorReader
from vtk.util.numpy_support import vtk_to_numpy as v2n

here = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "vectors")
cases = [("countries.shp", 0), ("rivers.shp", 0), ("cities.shp", 0), ("countries.geojson", 0), ("rivers.geojson", 0), ("cities.geojson", 0),
         ("world.gpkg", 0), ("world.gpkg", 1),
         ("countries.fgb", 0), ("rivers.fgb", 0), ("cities.fgb", 0), ("points.fgb", 0), ("world.gml", 0), ("world.gml", 1), ("world.gml", 2), ("world.kmz", 0),
         ("countries.tab", 0), ("rivers.tab", 0), ("cities.tab", 0), ("countries.mif", 0), ("rivers.mif", 0), ("cities.mif", 0),
         ("detail.tab", 0), ("detail.mif", 0),
         ("world.gdb", 0), ("world.gdb", 1), ("world.gdb", 2), ("world.gdb", 3), ("world.gdb", 4), ("world.gdb", 5)]
out = {}
for f, layer in cases:
    r = GDALVectorReader(FileName=os.path.join(here, f), ActiveLayer=layer)
    try:
        r.UpdatePipeline()
    except Exception as e:
        print(f, "unreadable:", e)
        continue
    d = r.GetClientSideObject().GetOutputDataObject(0)
    if d is None or d.GetNumberOfBlocks() == 0:
        print(f, layer, "unreadable (no blocks)")
        continue
    blocks = []
    for b in range(d.GetNumberOfBlocks()):
        p = d.GetBlock(b)
        cells = {}
        for kind, ca in (("verts", p.GetVerts()), ("lines", p.GetLines())):
            cells[kind] = v2n(ca.GetOffsetsArray()).tolist()
        attrs = {}
        cd = p.GetCellData()
        for i in range(cd.GetNumberOfArrays()):
            a = cd.GetAbstractArray(i)
            if a.IsA("vtkStringArray"):
                attrs[a.GetName()] = ["str", [a.GetValue(k) for k in range(a.GetNumberOfValues())]]
            else:
                attrs[a.GetName()] = [a.GetClassName(), v2n(a).tolist()]
        blocks.append({"points": v2n(p.GetPoints().GetData()).tolist(), "cells": cells, "attrs": attrs})
    out[f"{f}#{layer}"] = blocks
json.dump(out, open(os.path.join(here, "paraview_reference.json"), "w"), indent=0)
print({k: len(v) for k, v in out.items()})
