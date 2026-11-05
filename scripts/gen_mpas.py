"""A small MPAS-style ocean output file on an icosahedral sphere: testdata/netcdf/mpas_ocean.nc.

    python3 scripts/gen_mpas.py [subdivisions=3] [out]

MPAS cells are the icosphere's vertices (pentagons and hexagons), MPAS vertices are its
triangles (cellsOnVertex lists the three cells around each, 1-based), as in MPAS meshes.
Variables: temperature(Time, nCells, nVertLevels), salinity(Time, nCells, nVertLevels),
ssh(Time, nCells), relativeVorticity(Time, nVertices, nVertLevels), areaCell(nCells), and
maxLevelCell(nCells) with land (0) where a smooth "continent" function is negative, so
the reader's land masking is exercised. Written as 64-bit-offset classic NetCDF.
"""
import sys, os
import warnings
import numpy as np
warnings.filterwarnings("ignore", category=DeprecationWarning)
from netCDF4 import Dataset

k = int(sys.argv[1]) if len(sys.argv) > 1 else 3
out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), "..", "testdata", "netcdf", "mpas_ocean.nc")
R = 6371229.0

t = (1 + 5 ** 0.5) / 2
verts = [(-1, t, 0), (1, t, 0), (-1, -t, 0), (1, -t, 0), (0, -1, t), (0, 1, t), (0, -1, -t), (0, 1, -t),
         (t, 0, -1), (t, 0, 1), (-t, 0, -1), (-t, 0, 1)]
verts = [np.array(v, float) / np.linalg.norm(v) for v in verts]
faces = [(0, 11, 5), (0, 5, 1), (0, 1, 7), (0, 7, 10), (0, 10, 11), (1, 5, 9), (5, 11, 4), (11, 10, 2),
         (10, 7, 6), (7, 1, 8), (3, 9, 4), (3, 4, 2), (3, 2, 6), (3, 6, 8), (3, 8, 9), (4, 9, 5),
         (2, 4, 11), (6, 2, 10), (8, 6, 7), (9, 8, 1)]
for _ in range(k):
    mid = {}
    def m(a, b):
        key = (min(a, b), max(a, b))
        if key not in mid:
            p = verts[a] + verts[b]
            verts.append(p / np.linalg.norm(p))
            mid[key] = len(verts) - 1
        return mid[key]
    nf = []
    for a, b, c in faces:
        ab, bc, ca = m(a, b), m(b, c), m(c, a)
        nf += [(a, ab, ca), (b, bc, ab), (c, ca, bc), (ab, bc, ca)]
    faces = nf

P = np.array(verts)                       # cell centers (unit)
F = np.array(faces)                       # triangles = MPAS vertices
nCells, nVertices, nLev, nTime = len(P), len(F), 4, 3
V = P[F].sum(axis=1)
V /= np.linalg.norm(V, axis=1)[:, None]  # vertex positions (unit)

# verticesOnCell: the triangles around each cell, ordered counter-clockwise.
around = [[] for _ in range(nCells)]
for f, tri in enumerate(F):
    for c in tri:
        around[c].append(f)
maxEdges = 6
voc = np.zeros((nCells, maxEdges), np.int32)
nEdgesOnCell = np.zeros(nCells, np.int32)
for c in range(nCells):
    n = P[c]
    e1 = np.cross(n, [0, 0, 1.0]) if abs(n[2]) < 0.9 else np.cross(n, [1.0, 0, 0])
    e1 /= np.linalg.norm(e1)
    e2 = np.cross(n, e1)
    fs = sorted(around[c], key=lambda f: np.arctan2(V[f] @ e2, V[f] @ e1))
    nEdgesOnCell[c] = len(fs)
    voc[c, :len(fs)] = np.array(fs) + 1

lat = np.arcsin(P[:, 2]); lon = np.arctan2(P[:, 1], P[:, 0]) % (2 * np.pi)
latV = np.arcsin(V[:, 2]); lonV = np.arctan2(V[:, 1], V[:, 0]) % (2 * np.pi)
# Land: three round "continents" (angular radius in degrees around a lon/lat center).
land = np.full(nCells, -1.0)
for lon0, lat0, rad in [(30, 20, 38), (250, 45, 30), (300, -35, 22)]:
    c0 = np.array([np.cos(np.radians(lat0)) * np.cos(np.radians(lon0)), np.cos(np.radians(lat0)) * np.sin(np.radians(lon0)), np.sin(np.radians(lat0))])
    land = np.maximum(land, P @ c0 - np.cos(np.radians(rad)))   # > 0 inside the cap
maxLevelCell = np.where(land > 0, 0, nLev).astype(np.int32)

d = Dataset(out, "w", format="NETCDF3_64BIT_OFFSET")
d.on_a_sphere = "YES"
d.sphere_radius = R
d.model_name = "auraFX test (MPAS-style)"
d.createDimension("Time", None)
for name, n in [("nCells", nCells), ("nVertices", nVertices), ("vertexDegree", 3), ("maxEdges", maxEdges),
                ("nVertLevels", nLev), ("StrLen", 64)]:
    d.createDimension(name, n)
def var(name, typ, dims, data):
    v = d.createVariable(name, typ, dims)
    v[:] = data
    return v
var("xCell", "f8", ("nCells",), R * P[:, 0]); var("yCell", "f8", ("nCells",), R * P[:, 1]); var("zCell", "f8", ("nCells",), R * P[:, 2])
var("latCell", "f8", ("nCells",), lat); var("lonCell", "f8", ("nCells",), lon)
var("xVertex", "f8", ("nVertices",), R * V[:, 0]); var("yVertex", "f8", ("nVertices",), R * V[:, 1]); var("zVertex", "f8", ("nVertices",), R * V[:, 2])
var("latVertex", "f8", ("nVertices",), latV); var("lonVertex", "f8", ("nVertices",), lonV)
var("cellsOnVertex", "i4", ("nVertices", "vertexDegree"), F + 1)
var("verticesOnCell", "i4", ("nCells", "maxEdges"), voc)
var("nEdgesOnCell", "i4", ("nCells",), nEdgesOnCell)
var("maxLevelCell", "i4", ("nCells",), maxLevelCell)
var("areaCell", "f8", ("nCells",), 4 * np.pi * R * R / nCells * (1 + 0.05 * np.cos(lat)))
xt = d.createVariable("xtime", "S1", ("Time", "StrLen"))
temp = d.createVariable("temperature", "f4", ("Time", "nCells", "nVertLevels"))
salt = d.createVariable("salinity", "f4", ("Time", "nCells", "nVertLevels"))
ssh = d.createVariable("ssh", "f4", ("Time", "nCells"))
vort = d.createVariable("relativeVorticity", "f4", ("Time", "nVertices", "nVertLevels"))
for it in range(nTime):
    s = f"0001-01-{it + 1:02d}_00:00:00".ljust(64)
    xt[it] = np.array(list(s), "S1")
    for lv in range(nLev):
        temp[it, :, lv] = 28 * np.cos(lat) ** 2 - 2 - 4 * lv + 1.5 * it * np.sin(lon)
        salt[it, :, lv] = 34 + np.sin(2 * lat) + 0.1 * lv + 0.05 * it
        vort[it, :, lv] = 1e-5 * np.sin(3 * lonV + 0.4 * it) * np.cos(latV) / (1 + lv)
    ssh[it] = 0.8 * np.sin(lat) * np.cos(lon - 0.3 * it)
d.close()
print(f"{out}: {nCells} cells, {nVertices} vertices, {int((maxLevelCell == 0).sum())} land cells, {os.path.getsize(out)} bytes")
