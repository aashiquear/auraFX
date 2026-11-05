"""A small POP-style ocean history file: testdata/netcdf/pop_ocean.nc.

    python3 scripts/gen_pop.py [out]

POP (the Parallel Ocean Program, CESM's ocean) writes its displaced-pole grid with index
dimensions (nlat, nlon) and 2-D coordinates TLONG/TLAT named in each variable's `coordinates`
attribute; depth z_t is in centimeters, positive down. Variables: TEMP(time, z_t, nlat, nlon)
with land as _FillValue, SSH(time, nlat, nlon), KMT(nlat, nlon) (the number of ocean levels),
two time steps (days since 0000-01-01). The grid here is a smooth curvilinear patch, so the
reader has to use TLONG/TLAT rather than the index dimensions.
"""
import os, sys, warnings
import numpy as np
from netCDF4 import Dataset
warnings.filterwarnings("ignore", category=DeprecationWarning)

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "testdata", "netcdf", "pop_ocean.nc")
nlon, nlat, nz, nt = 36, 24, 5, 2
i, j = np.meshgrid(np.arange(nlon), np.arange(nlat))
TLONG = (100 + 6.0 * i + 1.5 * np.sin(j / 4.0)) % 360       # degrees east, wraps past 360
TLAT = -60 + 4.5 * j + 2.0 * np.sin(i / 5.0)                # degrees north
z_t = np.array([500.0, 1500.0, 3000.0, 6000.0, 12000.0])    # cm
fill = 9.96921e36
land = ((i - 12) ** 2 / 30.0 + (j - 10) ** 2 / 12.0) < 1.0  # an island
KMT = np.where(land, 0, np.clip(5 - (np.abs(j - 12) // 5), 1, 5)).astype(np.int32)

d = Dataset(out, "w", format="NETCDF4")
d.title = "auraFX test (POP-style)"
d.Conventions = "CF-1.0; http://www.cgd.ucar.edu/cms/eaton/netcdf/CF-current.htm"
d.createDimension("time", None)
d.createDimension("z_t", nz)
d.createDimension("nlat", nlat)
d.createDimension("nlon", nlon)
t = d.createVariable("time", "f8", ("time",)); t.units = "days since 0000-01-01 00:00:00"; t.calendar = "noleap"; t.long_name = "time"
t[:] = [365.0, 396.0]
z = d.createVariable("z_t", "f4", ("z_t",)); z.units = "centimeters"; z.positive = "down"; z.long_name = "depth from surface to midpoint of layer"
z[:] = z_t
lo = d.createVariable("TLONG", "f8", ("nlat", "nlon")); lo.units = "degrees_east"; lo.long_name = "array of t-grid longitudes"
lo[:] = TLONG
la = d.createVariable("TLAT", "f8", ("nlat", "nlon")); la.units = "degrees_north"; la.long_name = "array of t-grid latitudes"
la[:] = TLAT
k = d.createVariable("KMT", "i4", ("nlat", "nlon")); k.coordinates = "TLONG TLAT"; k.long_name = "k Index of Deepest Grid Cell on T Grid"
k[:] = KMT
T = d.createVariable("TEMP", "f4", ("time", "z_t", "nlat", "nlon"), fill_value=fill)
T.units = "degC"; T.long_name = "Potential Temperature"; T.coordinates = "TLONG TLAT z_t time"
S = d.createVariable("SSH", "f4", ("time", "nlat", "nlon"), fill_value=fill)
S.units = "centimeter"; S.long_name = "Sea Surface Height"; S.coordinates = "TLONG TLAT time"
for it in range(nt):
    temp = np.empty((nz, nlat, nlon), np.float32)
    for kz in range(nz):
        v = 28 * np.cos(np.radians(TLAT)) ** 2 - 2 - 3 * kz + it * np.sin(np.radians(TLONG))
        temp[kz] = np.where(kz < KMT, v, fill)
    T[it] = temp
    S[it] = np.where(land, fill, 50 * np.sin(np.radians(TLAT * 2)) * np.cos(np.radians(TLONG - 10 * it)))
d.close()
print(out, os.path.getsize(out))
