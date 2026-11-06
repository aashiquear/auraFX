"""GeoTIFF and Esri ASCII grid test files in testdata/rasters/, written by the GDAL library
bundled with ParaView (through ctypes; ParaView's Python has no osgeo module):

    pvpython scripts/gen_rasters.py [outdir]

- dem.tif: Float32 elevation, 120 x 80, 30 m pixels in UTM 33N (EPSG:32633), north-up,
  DEFLATE-compressed, no-data -9999 in a corner.
- rgb.tif: Byte RGB, 64 x 48, lon/lat (EPSG:4326), pixel-interleaved.
- bands.tif: Int16, 4 bands (not colors), band-interleaved (planar) and tiled.
- point.tif: Float32 with AREA_OR_POINT=Point (the tie point is a pixel center).
- dem.asc: the same elevations as an Esri ASCII grid (6 significant digits).
- dem.img, rgb.img, bands.img: the GeoTIFFs copied to ERDAS Imagine (HFA) by GDAL; dem.img
  run-length compressed (COMPRESSED=YES), all in 64 x 64 blocks (dem.img: four, two of them
  partly outside the image).
- classes.img: Byte classes in long runs, 100 x 70, run-length compressed; bits1.img and
  bits4.img: 1-bit and 4-bit layers (NBITS), compressed.
"""
import ctypes, os, sys, math
from ctypes import c_char_p, c_int, c_double, c_void_p, POINTER

lib = ctypes.CDLL(os.path.expanduser("~/Downloads/ParaView-6.1.1-MPI-Linux-Python3.12-x86_64/lib/libgdal.so"))
lib.GDALAllRegister()
lib.GDALGetDriverByName.restype = c_void_p
lib.GDALGetDriverByName.argtypes = [c_char_p]
lib.GDALCreate.restype = c_void_p
lib.GDALCreate.argtypes = [c_void_p, c_char_p, c_int, c_int, c_int, c_int, POINTER(c_char_p)]
lib.GDALCreateCopy.restype = c_void_p
lib.GDALCreateCopy.argtypes = [c_void_p, c_char_p, c_void_p, c_int, POINTER(c_char_p), c_void_p, c_void_p]
lib.GDALSetGeoTransform.argtypes = [c_void_p, POINTER(c_double)]
lib.GDALGetRasterBand.restype = c_void_p
lib.GDALGetRasterBand.argtypes = [c_void_p, c_int]
lib.GDALRasterIO.argtypes = [c_void_p, c_int, c_int, c_int, c_int, c_int, c_void_p, c_int, c_int, c_int, c_int, c_int]
lib.GDALSetRasterNoDataValue.argtypes = [c_void_p, c_double]
lib.GDALSetRasterColorInterpretation.argtypes = [c_void_p, c_int]
lib.GDALSetMetadataItem.argtypes = [c_void_p, c_char_p, c_char_p, c_char_p]
lib.GDALClose.argtypes = [c_void_p]
lib.OSRNewSpatialReference.restype = c_void_p
lib.OSRNewSpatialReference.argtypes = [c_char_p]
lib.OSRImportFromEPSG.argtypes = [c_void_p, c_int]
lib.OSRExportToWkt.argtypes = [c_void_p, POINTER(c_char_p)]
lib.GDALSetProjection.argtypes = [c_void_p, c_char_p]

GDT = {"Byte": 1, "Int16": 3, "Float32": 6}
CT = {"Byte": ctypes.c_uint8, "Int16": ctypes.c_int16, "Float32": ctypes.c_float}

def opts(*o):
    arr = (c_char_p * (len(o) + 1))(*[s.encode() for s in o], None)
    return arr

def wkt(epsg):
    srs = lib.OSRNewSpatialReference(None)
    lib.OSRImportFromEPSG(srs, epsg)
    out = c_char_p()
    lib.OSRExportToWkt(srs, ctypes.byref(out))
    return out.value

def make(path, w, h, typ, bands, gt, epsg, options=(), nodata=None, color=None, meta=None):
    drv = lib.GDALGetDriverByName(b"GTiff")
    ds = lib.GDALCreate(drv, path.encode(), w, h, len(bands), GDT[typ], opts(*options))
    assert ds, path
    lib.GDALSetGeoTransform(ds, (c_double * 6)(*gt))
    lib.GDALSetProjection(ds, wkt(epsg))
    if meta:
        for k, v in meta.items():
            lib.GDALSetMetadataItem(ds, k.encode(), v.encode(), None)
    for b, f in enumerate(bands, 1):
        band = lib.GDALGetRasterBand(ds, b)
        buf = (CT[typ] * (w * h))(*[f(i, j) for j in range(h) for i in range(w)])
        assert lib.GDALRasterIO(band, 1, 0, 0, w, h, buf, w, h, GDT[typ], 0, 0) == 0
        if nodata is not None:
            lib.GDALSetRasterNoDataValue(band, nodata)
        if color:
            lib.GDALSetRasterColorInterpretation(band, color[b - 1])
    lib.GDALClose(ds)
    print(path, os.path.getsize(path))

out = sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "rasters")
os.makedirs(out, exist_ok=True)

def elev(i, j):
    if i < 15 and j < 10:
        return -9999.0                     # no data in the upper-left corner
    x, y = i / 119.0, j / 79.0
    return 400 + 300 * math.exp(-((x - 0.6) ** 2 + (y - 0.45) ** 2) / 0.05) + 80 * math.sin(6 * x) * math.cos(4 * y) + 2 * i

make(f"{out}/dem.tif", 120, 80, "Float32", [elev], [500000, 30, 0, 4200000, 0, -30], 32633,
     ("COMPRESS=DEFLATE",), nodata=-9999.0)
make(f"{out}/rgb.tif", 64, 48, "Byte",
     [lambda i, j: (i * 4) % 256, lambda i, j: (j * 5) % 256, lambda i, j: (i * j) % 256],
     [10.0, 0.25, 0, 50.0, 0, -0.25], 4326, ("PHOTOMETRIC=RGB",), color=[3, 4, 5])
make(f"{out}/bands.tif", 40, 30, "Int16",
     [lambda i, j, k=k: (k + 1) * 100 + i - 2 * j for k in range(4)],
     [-1000, 50, 0, 2000, 0, -50], 32633, ("INTERLEAVE=BAND", "TILED=YES", "BLOCKXSIZE=16", "BLOCKYSIZE=16"))
make(f"{out}/point.tif", 20, 10, "Float32", [lambda i, j: i + 100 * j],
     [100.0, 2.0, 0, 300.0, 0, -2.0], 32633, meta={"AREA_OR_POINT": "Point"})

# Esri ASCII grid of the same elevations (ParaView's GDAL has no AAIGrid driver, so written
# here): rows from the top, lower-left corner and cell size in the header.
with open(f"{out}/dem.asc", "w") as f:
    f.write("ncols        120\nnrows        80\nxllcorner    500000\nyllcorner    4197600\ncellsize     30\nNODATA_value -9999\n")
    for j in range(80):
        f.write(" ".join(f"{elev(i, j):.6g}" for i in range(120)) + "\n")
print(f"{out}/dem.asc", os.path.getsize(f"{out}/dem.asc"))

# ERDAS Imagine copies of the GeoTIFFs.
lib.GDALOpen.restype = c_void_p
lib.GDALOpen.argtypes = [c_char_p, c_int]
hfa = lib.GDALGetDriverByName(b"HFA")
for name, o in (("dem", ("COMPRESSED=YES",)), ("rgb", ()), ("bands", ())):
    src = lib.GDALOpen(f"{out}/{name}.tif".encode(), 0)
    for ext in ("img", "rrd", "aux.xml"):
        if os.path.exists(f"{out}/{name}.{ext}"):
            os.remove(f"{out}/{name}.{ext}")
    dst = lib.GDALCreateCopy(hfa, f"{out}/{name}.img".encode(), src, 0, opts(*o), None, None)
    assert dst, name
    lib.GDALClose(dst)
    lib.GDALClose(src)
    if os.path.exists(f"{out}/{name}.img.aux.xml"):  # (GDAL's notes from the copy)
        os.remove(f"{out}/{name}.img.aux.xml")
    print(f"{out}/{name}.img", os.path.getsize(f"{out}/{name}.img"))

# Images that run-length coding shrinks (GDAL stores a block plainly when it does not), and
# sub-byte layers.
mem = lib.GDALGetDriverByName(b"MEM")
for name, nbits, f in (("classes", None, lambda i, j: (i // 13 + 3 * (j // 9)) % 7 * 30),
                       ("bits1", "1", lambda i, j: 1 if (i // 7 + j // 5) % 2 else 0),
                       ("bits4", "4", lambda i, j: (i // 9 + j // 4) % 16)):
    w, h = 100, 70
    src = lib.GDALCreate(mem, b"", w, h, 1, 1, None)
    lib.GDALSetGeoTransform(src, (c_double * 6)(*[300000.0, 10.0, 0, 4100000.0, 0, -10.0]))
    lib.GDALSetProjection(src, wkt(32633))
    band = lib.GDALGetRasterBand(src, 1)
    buf = (ctypes.c_uint8 * (w * h))(*[f(i, j) for j in range(h) for i in range(w)])
    assert lib.GDALRasterIO(band, 1, 0, 0, w, h, buf, w, h, 1, 0, 0) == 0
    o = ["COMPRESSED=YES"] + ([f"NBITS={nbits}"] if nbits else [])
    if os.path.exists(f"{out}/{name}.img"):
        os.remove(f"{out}/{name}.img")
    dst = lib.GDALCreateCopy(hfa, f"{out}/{name}.img".encode(), src, 0, opts(*o), None, None)
    assert dst, name
    lib.GDALClose(dst)
    lib.GDALClose(src)
    if os.path.exists(f"{out}/{name}.img.aux.xml"):
        os.remove(f"{out}/{name}.img.aux.xml")
    print(f"{out}/{name}.img", os.path.getsize(f"{out}/{name}.img"))
