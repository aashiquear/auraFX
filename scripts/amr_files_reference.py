"""ParaView 6.1.1's reading of the FLASH and Enzo test files (testdata/amr/flash_enzo_reference.json).

    (ulimit -v 8000000; timeout 180 pvpython scripts/amr_files_reference.py)

Every level and block: origin, spacing, dimensions, the sum of each cell array, and the cells
marked covered in vtkGhostType. All levels and arrays are loaded (ParaView's defaults load one
level and no arrays); Enzo is read with and without ConvertToCGS; Chombo through ParaView's VisIt Chombo reader
(which keeps level 0's ghost cells as ordinary cells and drops the other levels'). ParaView 6.1.1 cannot read 2-D FLASH files
("Inconsistent types of vtkCartesianGrid", or a crash with several levels), so the 2-D file
is checked against the values it was written with.
"""
import json, os
from paraview.simple import FlashReader, EnzoReader, VisItChomboReader
from vtk.util.numpy_support import vtk_to_numpy as v2n

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "amr")
cases = {
    "flash3d_hdf5_plt_cnt_0007": (FlashReader, ["dens", "pres", "velx"], {}),
    "flash3d_ffv7_hdf5_chk_0007": (FlashReader, ["dens", "pres", "velx"], {}),
    "enzo/DD0003/data0003.hierarchy": (EnzoReader, ["Density", "TotalEnergy"], {"ConvertToCGS": 0}),
    "enzo/DD0003/data0003.hierarchy#cgs": (EnzoReader, ["Density", "TotalEnergy"], {"ConvertToCGS": 1}),
    "chombo3d.hdf5": (VisItChomboReader, ["density", "energy"], {}),
    "chombo2d.hdf5": (VisItChomboReader, ["density", "energy"], {}),
    "chombo3d_node.hdf5": (VisItChomboReader, ["density", "energy"], {"node": 1}),
}
out = {}
for key, (Reader, arrays, props) in cases.items():
    fn = os.path.join(td, key.split("#")[0])
    r = VisItChomboReader(FileName=fn) if Reader is VisItChomboReader else Reader(FileNames=[fn])
    if Reader is not VisItChomboReader:
        r.Level = 10
    r.CellArrayStatus = arrays
    if props.pop("node", None):
        r.CellArrayStatus = []
        r.PointArrayStatus = arrays
    for k, v in props.items():
        setattr(r, k, v)
    r.UpdatePipeline()
    amr = r.GetClientSideObject().GetOutputDataObject(0)
    levels = []
    for l in range(amr.GetNumberOfLevels()):
        blocks = []
        for b in range(amr.GetNumberOfDataSets(l)):
            g = amr.GetDataSetAsImageData(l, b)
            if g is None:
                continue
            sums = {}
            for kind, cd in (("", g.GetCellData()), ("point:", g.GetPointData())):
                for i in range(cd.GetNumberOfArrays()):
                    a = cd.GetArray(i)
                    sums[kind + a.GetName()] = [a.GetClassName(), float(v2n(a).astype("float64").sum())]
            ghost = g.GetCellGhostArray()
            hidden = int((v2n(ghost) & 8 != 0).sum()) if ghost is not None else 0
            blocks.append({"origin": list(g.GetOrigin()), "spacing": list(g.GetSpacing()), "dims": list(g.GetDimensions()), "arrays": sums, "covered": hidden})
        levels.append(blocks)
    out[key] = {"levels": levels}
    print(key, [len(l) for l in levels])
json.dump(out, open(os.path.join(td, "flash_enzo_reference.json"), "w"), indent=1)
