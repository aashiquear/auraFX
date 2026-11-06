"""ParaView 6.1.1's Cell Grid Probe (vtkCellGridPointProbe) on VTK's DG test files
(testdata/cellgrid/probe_reference.json).

    (ulimit -v 8000000; timeout 600 pvpython scripts/cellgrid_probe_reference.py)

Each file is probed in its own pvpython process (ParaView keeps state between cell grid
readers in one session). The probe points are a 5 x 5 x 5 lattice over the grid's bounds
grown by a tenth of the largest side (3 layers across a flat axis) followed by the cell
centers (the mean of each cell's corners). For each file: the probe with no attribute (classification only) and with every
attribute; the output points, ContainingCellID, ParametricCoordinates and the values (NaN
written as null). VTK's SMP backend is set to Sequential: its parallel interpolation
races (see one()).
"""
import json, os, subprocess, sys

td = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "cellgrid")
FILES = ["dgHexahedra", "dgTetrahedra", "dgWedges", "dgPyramids", "dgPyramid19", "dgQuadrilateral",
         "dgQuadraticQuadrilaterals", "dgTriangle", "dgEdges", "dgMixed"]


def lattice(b):
    big = max(b[1] - b[0], b[3] - b[2], b[5] - b[4])
    axes = []
    for k in range(3):
        lo, hi = b[2 * k], b[2 * k + 1]
        if hi - lo < 1e-12 * max(big, 1.0):
            axes.append([lo - 0.1 * big, lo, lo + 0.1 * big])
        else:
            d = 0.1 * big
            axes.append([lo - d + (hi - lo + 2 * d) * i / 4 for i in range(5)])
    return [[x, y, z] for z in axes[2] for y in axes[1] for x in axes[0]]


def clean(v):
    return [None if x != x else x for x in v]


def one(name):
    import paraview.simple  # noqa: F401 (loads ParaView's cell grid responders)
    from vtkmodules.vtkFiltersCellGrid import vtkCellGridPointProbe, vtkCellGridToUnstructuredGrid
    from vtkmodules.vtkIOCellGrid import vtkCellGridReader
    import vtk
    from vtkmodules.vtkCommonCore import vtkSMPTools
    # VTK interpolates in a parallel loop sharing one calculator (and its per-cell cache)
    # between threads, which scrambles values (and can corrupt the heap); run it serially.
    vtkSMPTools.SetBackend("Sequential")
    r = vtkCellGridReader()
    r.SetFileName(os.path.join(td, name + ".dg"))
    r.Update()
    g = r.GetOutput()
    u = vtkCellGridToUnstructuredGrid()
    u.SetInputData(g)
    cc = vtk.vtkCellCenters()
    cc.SetInputConnection(u.GetOutputPort())
    cc.Update()
    c = cc.GetOutput()
    b = [0.0] * 6
    g.GetBounds(b)
    pts = lattice(b) + [list(c.GetPoint(i)) for i in range(c.GetNumberOfPoints())]
    P = vtk.vtkPoints()
    P.SetDataTypeToDouble()
    for p in pts:
        P.InsertNextPoint(p)
    poly = vtk.vtkPolyData()
    poly.SetPoints(P)
    names = [a.GetName().Data() for a in g.GetCellAttributeList()]
    shape = g.GetShapeAttribute().GetName().Data()
    res = {"points": pts, "attributes": names, "shape": shape, "probes": {}}
    for att in [""] + names:
        f = vtkCellGridPointProbe()
        f.SetInputData(0, poly)
        f.SetInputData(1, g)
        f.SetAttributeName(att)
        f.Update()
        o = f.GetOutput()
        pd = o.GetPointData()
        arr = lambda n: [list(pd.GetArray(n).GetTuple(i)) for i in range(o.GetNumberOfPoints())] if pd.GetArray(n) else None
        e = {"points": [list(o.GetPoint(i)) for i in range(o.GetNumberOfPoints())],
             "cell": [int(x[0]) for x in arr("ContainingCellID") or []],
             "params": [clean(x) for x in arr("ParametricCoordinates") or []]}
        if att and pd.GetArray(att) is not None:
            e["values"] = [clean(x) for x in arr(att)]
        res["probes"][att] = e
    return res


if __name__ == "__main__":
    if len(sys.argv) > 1:
        print(json.dumps(one(sys.argv[1])))
        sys.exit(0)
    out = {}
    for n in FILES:
        p = subprocess.run([sys.executable, os.path.abspath(__file__), n], capture_output=True, text=True, timeout=170)
        line = [l for l in p.stdout.splitlines() if l.startswith("{")]
        if not line:
            print(n, "FAILED", p.stderr[-2000:])
            continue
        out[n] = json.loads(line[-1])
        print(n, {k: len(v["cell"]) for k, v in out[n]["probes"].items()})
    json.dump(out, open(os.path.join(td, "probe_reference.json"), "w"))
