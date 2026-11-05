"""A small CAM-SE-style output and its connectivity file:
testdata/netcdf/cam_se.nc and testdata/netcdf/cam_se_connectivity.nc.

    python3 scripts/gen_cam.py [n=6] [outdir]

CAM-SE (the spectral-element dynamical core of CESM's atmosphere) writes its columns on a
cubed sphere as a list of points: lat(ncol), lon(ncol) in degrees with longitudes in
[0, 360). The quads joining them come in a separate file: element_corners(ncorners=4, ncells),
1-based, clockwise on a lon/lat map. Variables: PS(time, ncol), T(time, lev, ncol), OMEGA(time, ilev, ncol), with
lev/ilev coordinates (hybrid levels, top first) and two time steps.
"""
import os, sys, warnings
import numpy as np
from netCDF4 import Dataset
warnings.filterwarnings("ignore", category=DeprecationWarning)

n = int(sys.argv[1]) if len(sys.argv) > 1 else 6
out = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(__file__), "..", "testdata", "netcdf")

# Cubed sphere: each face an (n+1) x (n+1) grid of points, merged where faces meet.
faces = [
    lambda a, b: (1, a, b), lambda a, b: (-a, 1, b), lambda a, b: (-1, -a, b),
    lambda a, b: (a, -1, b), lambda a, b: (-b, a, 1), lambda a, b: (b, a, -1),
]
key = {}
pts = []
quads = []
for face in faces:
    ids = np.zeros((n + 1, n + 1), int)
    for i in range(n + 1):
        for j in range(n + 1):
            a, b = np.tan(np.pi / 4 * (2 * i / n - 1)), np.tan(np.pi / 4 * (2 * j / n - 1))
            p = np.array(face(a, b), float)
            p /= np.linalg.norm(p)
            k = tuple(np.round(p, 9))
            if k not in key:
                key[k] = len(pts)
                pts.append(p)
            ids[i, j] = key[k]
    for i in range(n):
        for j in range(n):
            q = [ids[i, j], ids[i + 1, j], ids[i + 1, j + 1], ids[i, j + 1]]
            # Clockwise seen from outside (clockwise on a lon/lat map), the order ParaView's CAM
            # reader expects: it takes counter-clockwise cells for ones crossing the 0/360 seam.
            c = sum(pts[x] for x in q)
            if np.dot(np.cross(pts[q[1]] - pts[q[0]], pts[q[2]] - pts[q[0]]), c) > 0:
                q = q[::-1]
            quads.append(q)
P = np.array(pts)
ncol, ncells = len(P), len(quads)
lat = np.degrees(np.arcsin(np.clip(P[:, 2], -1, 1)))
lon = (np.degrees(np.arctan2(P[:, 1], P[:, 0])) + 10.0) % 360.0   # turned so no column sits on the 0/360 seam
lev = np.array([100.0, 300.0, 500.0, 700.0, 900.0])
ilev = np.array([0.0, 200.0, 400.0, 600.0, 800.0, 1000.0])

d = Dataset(os.path.join(out, "cam_se.nc"), "w", format="NETCDF3_64BIT_OFFSET")
d.title = "auraFX test (CAM-SE-style)"
d.np = 4
d.ne = n
d.createDimension("time", None)
d.createDimension("lev", len(lev))
d.createDimension("ilev", len(ilev))
d.createDimension("ncol", ncol)
v = d.createVariable("lat", "f8", ("ncol",)); v.units = "degrees_north"; v[:] = lat
v = d.createVariable("lon", "f8", ("ncol",)); v.units = "degrees_east"; v[:] = lon
v = d.createVariable("lev", "f8", ("lev",)); v.units = "level"; v.positive = "down"; v[:] = lev
v = d.createVariable("ilev", "f8", ("ilev",)); v.units = "level"; v.positive = "down"; v[:] = ilev
t = d.createVariable("time", "f8", ("time",)); t.units = "days since 0001-01-01 00:00:00"; t[:] = [0.0, 0.25]
PS = d.createVariable("PS", "f4", ("time", "ncol")); PS.units = "Pa"
T = d.createVariable("T", "f4", ("time", "lev", "ncol")); T.units = "K"
OM = d.createVariable("OMEGA", "f4", ("time", "ilev", "ncol")); OM.units = "Pa/s"
for it in range(2):
    PS[it] = 100000 + 1500 * np.cos(np.radians(lat)) * np.sin(np.radians(lon + 30 * it))
    for k in range(len(lev)):
        T[it, k] = 300 - 40 * np.sin(np.radians(lat)) ** 2 - 12 * k + 3 * it * np.cos(np.radians(lon))
    for k in range(len(ilev)):
        OM[it, k] = 0.1 * np.sin(np.radians(2 * lon)) * np.cos(np.radians(lat)) * (k + 1) / 6 + 0.01 * it
d.close()

c = Dataset(os.path.join(out, "cam_se_connectivity.nc"), "w", format="NETCDF3_64BIT_OFFSET")
c.createDimension("ncorners", 4)
c.createDimension("ncells", ncells)
e = c.createVariable("element_corners", "i4", ("ncorners", "ncells"))
e[:] = (np.array(quads) + 1).T
c.close()
print(f"cam_se.nc: {ncol} columns; cam_se_connectivity.nc: {ncells} quads")
