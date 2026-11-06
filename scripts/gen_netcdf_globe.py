"""A CF lon/lat sea-surface temperature file for the user guide: testdata/netcdf/globe_sst.nc.

    python3 scripts/gen_netcdf_globe.py [out]

A 2-degree global grid (180 x 90) with lon/lat bounds, three monthly time steps, `sst`
(Kelvin; warm equator, cooler poles, a seasonal shift and a few gyres) with the continents
as `_FillValue`, stored as float32 with zlib so the file stays small.
"""
import os, sys
import numpy as np
from netCDF4 import Dataset

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "testdata", "netcdf", "globe_sst.nc")
nlon, nlat, nt = 180, 90, 3
lon = np.arange(nlon) * 2.0 + 1.0
lat = -90.0 + np.arange(nlat) * 2.0 + 1.0
LON, LAT = np.meshgrid(lon, lat)
rl, rt = np.radians(LON), np.radians(LAT)


def land(lo, la):
    """A made-up set of continents from a few smooth blobs (true where land)."""
    blobs = [(-100, 45, 35, 25), (-60, -15, 20, 30), (20, 10, 30, 35), (90, 45, 60, 25), (135, -25, 20, 15), (0, -85, 180, 8)]
    m = np.zeros_like(lo, dtype=bool)
    for cx, cy, sx, sy in blobs:
        d = ((((lo - cx + 180) % 360) - 180) / sx) ** 2 + ((la - cy) / sy) ** 2
        m |= d + 0.25 * np.sin(5 * np.radians(lo)) * np.cos(3 * np.radians(la)) < 1.0
    return m


LAND = land(LON, LAT)
with Dataset(out, "w", format="NETCDF4") as d:
    d.Conventions = "CF-1.8"
    d.title = "Synthetic sea surface temperature on a 2-degree grid"
    d.createDimension("time", None)
    d.createDimension("lat", nlat)
    d.createDimension("lon", nlon)
    d.createDimension("bnds", 2)
    t = d.createVariable("time", "f8", ("time",))
    t.units = "days since 2001-01-01"
    t.calendar = "standard"
    t.bounds = "time_bnds"
    t[:] = [15.5, 45.0, 74.5]
    tb = d.createVariable("time_bnds", "f8", ("time", "bnds"))
    tb[:] = [[0, 31], [31, 59], [59, 90]]
    la = d.createVariable("lat", "f8", ("lat",))
    la.units, la.standard_name, la.bounds = "degrees_north", "latitude", "lat_bnds"
    la[:] = lat
    d.createVariable("lat_bnds", "f8", ("lat", "bnds"))[:] = np.c_[lat - 1, lat + 1]
    lo = d.createVariable("lon", "f8", ("lon",))
    lo.units, lo.standard_name, lo.bounds = "degrees_east", "longitude", "lon_bnds"
    lo[:] = lon
    d.createVariable("lon_bnds", "f8", ("lon", "bnds"))[:] = np.c_[lon - 1, lon + 1]
    v = d.createVariable("sst", "f4", ("time", "lat", "lon"), zlib=True, fill_value=np.float32(1e20))
    v.units, v.standard_name, v.long_name = "K", "sea_surface_temperature", "Sea Surface Temperature"
    for k in range(nt):
        season = 0.4 * np.sin(2 * np.pi * (k + 0.5) / 12.0)
        f = 271.5 + 30.0 * np.cos(rt) ** 2 * (1 + 0.08 * np.sin(3 * rl + k) * np.cos(2 * rt)) + 3.0 * season * np.sin(rt)
        f += 1.5 * np.sin(2 * rl + 0.6) * np.exp(-((LAT - 35) / 15) ** 2) - 1.5 * np.sin(2 * rl) * np.exp(-((LAT + 40) / 15) ** 2)
        f[LAND] = 1e20
        v[k, :, :] = f.astype(np.float32)
print(out, os.path.getsize(out))
