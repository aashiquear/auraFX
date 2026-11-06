"""ParaView 6.1.1's Compute Sides (vtkCellGridComputeSides) on VTK's DG test files
(testdata/cellgrid/sides_reference.json).

    (ulimit -v 8000000; timeout 900 pvpython scripts/cellgrid_sides_reference.py)

Each file runs in its own pvpython process. For every case (sides to include, omit sides of
renderable inputs, preserve renderable inputs, strategy; "twice" runs the default and then
edges of inputs on the result) the output grid is written with vtkCellGridWriter and its
"cell-types" section kept along with the (cell, side) pairs of every side list, and the output
is put through Cell Grid To Unstructured Grid: each cell's type and corner points. ParaView's
point data there is not kept: it evaluates a side's fields on the cell numbered like the side
in its list, not on the side's cell. On dgEdges it crashes once there are vertex sides, so
only the side lists are kept there.
"""
import json, os, subprocess, sys, tempfile

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "cellgrid")
FILES = ["dgHexahedra", "dgTetrahedra", "dgWedges", "dgPyramids", "dgPyramid19", "dgQuadrilateral",
         "dgQuadraticQuadrilaterals", "dgTriangle", "dgEdges", "dgMixed"]
# name: (flags, omit, preserve, strategy) with strategy 0 Winding, 1 AnyOccurrence, 2 Boundary
CASES = {
    "default": (37, 1, 1, 2),
    "all": (63, 0, 1, 2),
    "edges": (20, 0, 0, 2),
    "vertices": (11, 0, 0, 2),
    "surfaces": (32, 1, 0, 2),
    "all_any": (63, 0, 1, 1),
    "all_winding": (63, 0, 1, 0),
    "edges_keep": (20, 0, 1, 2),
}


def one(name):
    import paraview.simple  # noqa: F401 (registers the cell grid responders)
    from vtkmodules.vtkFiltersCellGrid import vtkCellGridComputeSides, vtkCellGridToUnstructuredGrid
    from vtkmodules.vtkIOCellGrid import vtkCellGridReader, vtkCellGridWriter
    r = vtkCellGridReader()
    r.SetFileName(os.path.join(td, name + ".dg"))
    r.Update()

    def sides(port, case):
        flags, omit, keep, strat = CASES[case]
        f = vtkCellGridComputeSides()
        f.SetInputConnection(port)
        f.SetOutputDimensionControl(flags)
        f.SetOmitSidesForRenderableInputs(omit)
        f.SetPreserveRenderableInputs(keep)
        f.SetStrategy(strat)
        return f

    def dump(f):
        tmp = tempfile.NamedTemporaryFile(suffix=".dg", delete=False).name
        w = vtkCellGridWriter()
        w.SetInputConnection(f.GetOutputPort())
        w.SetFileName(tmp)
        w.Write()
        d = json.load(open(tmp))
        os.unlink(tmp)
        lists = {}
        for ct in d["cell-types"]:
            for sp in ct.get("side-specs", []):
                g, a = sp["connectivity"]
                arr = [x for x in d["arrays"][g] if x["name"] == a][0]
                v = arr["data"]
                lists[g] = [[v[2 * i], v[2 * i + 1]] for i in range(len(v) // 2)]
        if name == "dgEdges":  # ParaView's To Unstructured Grid crashes on vertex sides of edges
            return {"cell-types": d["cell-types"], "lists": lists, "cells": None}
        u = vtkCellGridToUnstructuredGrid()
        u.SetInputConnection(f.GetOutputPort())
        u.Update()
        o = u.GetOutput()
        cells = []
        for i in range(o.GetNumberOfCells()):
            c = o.GetCell(i)
            cells.append([o.GetCellType(i), [list(o.GetPoint(c.GetPointId(k))) for k in range(c.GetNumberOfPoints())]])
        return {"cell-types": d["cell-types"], "lists": lists, "cells": cells}

    out = {}
    for case in CASES:
        out[case] = dump(sides(r.GetOutputPort(), case))
    first = sides(r.GetOutputPort(), "default")
    out["twice"] = dump(sides(first.GetOutputPort(), "edges_keep"))
    return out


if __name__ == "__main__":
    if len(sys.argv) > 1:
        print(json.dumps(one(sys.argv[1])))
        sys.exit(0)
    res = {}
    for n in FILES:
        p = subprocess.run([sys.executable, os.path.abspath(__file__), n], capture_output=True, text=True, timeout=170)
        line = [l for l in p.stdout.splitlines() if l.startswith("{")]
        if not line:
            print(n, "FAILED", p.stderr[-2000:])
            continue
        res[n] = json.loads(line[-1])
        print(n, {k: (sum(len(x) for x in v["lists"].values()), len(v["cells"] or [])) for k, v in res[n].items()})
    json.dump({"cases": CASES, "files": res}, open(os.path.join(td, "sides_reference.json"), "w"))
