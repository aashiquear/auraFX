"""Small GIS vector files: testdata/vectors/.

    pvpython scripts/gen_vectors.py [outdir]

Three layers written as GeoJSON by hand (lon/lat degrees, WGS 84):
  countries: two polygons (one with a hole) and a multipolygon of two islands;
  rivers:    a line string and a multi line string;
  cities:    points (one with a height) and a multipoint;
with string, integer and real attributes. The GDAL library inside ParaView then converts them
(GDALVectorTranslate through ctypes) to ESRI Shapefiles (one per layer, with .dbf and .prj),
one KML file (a folder per layer) and one GeoPackage holding all three layers; then FlatGeobuf
(one .fgb per layer), GML (world.gml with its .xsd, all three layers; world2.gml in GML 2), KMZ (world.kml zipped)
and MapInfo (each layer as a .tab with its .map, .id and .dat, and as .mif with .mid, plus a detailed
mixed layer, detail.tab and detail.mif), and an Esri file geodatabase (world.gdb: the three layers, and the detail layer's areas, lines
and points as lots, roads and wells).

    pvpython scripts/gen_vectors.py [outdir] --only fgb,gml,kmz,mapinfo,gdb   (just those formats)
"""
import ctypes, glob, json, os, sys

args = [a for a in sys.argv[1:] if not a.startswith("--") and "," not in a and a not in ("fgb", "gml", "kmz", "mapinfo", "gdb")]
only = set(sys.argv[sys.argv.index("--only") + 1].split(",")) if "--only" in sys.argv else None
out = args[0] if args else os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "vectors")
os.makedirs(out, exist_ok=True)

def fc(features):
    return {"type": "FeatureCollection", "features": [{"type": "Feature", "properties": p, "geometry": g} for p, g in features]}

countries = fc([
    ({"name": "Westland", "pop": 1200000, "area": 152.5}, {"type": "Polygon", "coordinates": [
        [[-10, 40], [0, 40], [2, 46], [-4, 50], [-11, 47], [-10, 40]],
        [[-6, 43], [-3, 43], [-3, 45], [-6, 45], [-6, 43]]]}),
    ({"name": "Eastmark", "pop": 830500, "area": 98.25}, {"type": "Polygon", "coordinates": [
        [[0, 40], [9, 41], [10, 47], [2, 46], [0, 40]]]}),
    ({"name": "Twin Isles", "pop": 40210, "area": 7.75}, {"type": "MultiPolygon", "coordinates": [
        [[[12, 38], [14, 38], [14, 40], [12, 40], [12, 38]]],
        [[[15, 41], [17, 41.5], [16, 43], [15, 41]]]]}),
])
rivers = fc([
    ({"name": "Long River", "length": 1240.5, "order": 1}, {"type": "LineString", "coordinates": [[-8, 48], [-5, 46.5], [-2, 44], [1, 42.5], [4, 41.5]]}),
    ({"name": "Forks", "length": 310.0, "order": 2}, {"type": "MultiLineString", "coordinates": [
        [[5, 45], [6, 43.5], [7, 42]], [[8, 46], [7.5, 44], [6, 43.5]]]}),
])
cities = fc([
    ({"name": "Portby", "pop": 250000, "elev": 12.5}, {"type": "Point", "coordinates": [-9, 43]}),
    ({"name": "Highcastle", "pop": 61000, "elev": 840.0}, {"type": "Point", "coordinates": [6, 44, 840]}),
    ({"name": "Isle towns", "pop": 9000, "elev": 3.0}, {"type": "MultiPoint", "coordinates": [[13, 39], [16, 42]]}),
])
layers = {"countries": countries, "rivers": rivers, "cities": cities}
for n, d in layers.items():
    with open(os.path.join(out, n + ".geojson"), "w") as f:
        json.dump(d, f, indent=1)

lib = [p for p in glob.glob(os.path.join(os.path.dirname(sys.executable), "..", "lib", "libgdal.so*"))][0]
g = ctypes.CDLL(lib)
g.GDALAllRegister()
g.GDALOpenEx.restype = ctypes.c_void_p
g.GDALOpenEx.argtypes = [ctypes.c_char_p, ctypes.c_uint, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
g.GDALVectorTranslateOptionsNew.restype = ctypes.c_void_p
g.GDALVectorTranslateOptionsNew.argtypes = [ctypes.c_void_p, ctypes.c_void_p]
g.GDALVectorTranslate.restype = ctypes.c_void_p
g.GDALVectorTranslate.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p]
g.GDALClose.argtypes = [ctypes.c_void_p]

def translate(src, dst, *args, into=None):
    ds = g.GDALOpenEx(src.encode(), 4, None, None, None)  # GDAL_OF_VECTOR
    assert ds, src
    argv = (ctypes.c_char_p * (len(args) + 1))(*[a.encode() for a in args], None)
    opts = g.GDALVectorTranslateOptionsNew(argv, None)
    srcs = (ctypes.c_void_p * 1)(ds)
    err = ctypes.c_int(0)
    r = g.GDALVectorTranslate(dst.encode() if into is None else None, into, 1, srcs, opts, ctypes.byref(err))
    assert r, (src, dst)
    if into is None:
        g.GDALClose(r)
    g.GDALClose(ds)

def old():
    for n in layers:
        shp = os.path.join(out, n + ".shp")
        for ext in ("shp", "shx", "dbf", "prj", "cpg"):
            p = os.path.join(out, f"{n}.{ext}")
            if os.path.exists(p):
                os.remove(p)
        # A shapefile holds one shape type: the cities become multipoints.
        translate(os.path.join(out, n + ".geojson"), shp, "-f", "ESRI Shapefile", *(["-nlt", "PROMOTE_TO_MULTI"] if n == "cities" else []))
    for p in ("world.kml", "world.gpkg"):
        if os.path.exists(os.path.join(out, p)):
            os.remove(os.path.join(out, p))
    for i, n in enumerate(layers):
        translate(os.path.join(out, n + ".geojson"), os.path.join(out, "world.gpkg"), "-f", "GPKG", *(["-update", "-append"] if i else []), "-nln", n)
    # KML: the driver writes one file per call, so each layer is written alone and the folders
    # (one per layer) are put into one document.
    import re
    parts = []
    for n in layers:
        tmp = os.path.join(out, f"_{n}.kml")
        translate(os.path.join(out, n + ".geojson"), tmp, "-f", "KML", "-nln", n)
        parts.append(open(tmp).read())
        os.remove(tmp)
    head = parts[0][: parts[0].index("<Folder>")]
    schemas = "".join(re.findall(r"<Schema.*?</Schema>\n", p, re.S)[0] for p in parts)
    head = re.sub(r"<Schema.*?</Schema>\n", "", head, flags=re.S)
    head = head.replace("<Document id=\"root_doc\">\n", "<Document id=\"root_doc\">\n" + schemas)
    folders = "".join(p[p.index("<Folder>"): p.rindex("</Folder>") + len("</Folder>\n")] for p in parts)
    open(os.path.join(out, "world.kml"), "w").write(head + folders + "</Document></kml>\n")

def new():
    if only is None or "fgb" in only:
        for n in layers:
            p = os.path.join(out, n + ".fgb")
            if os.path.exists(p):
                os.remove(p)
            translate(os.path.join(out, n + ".geojson"), p, "-f", "FlatGeobuf")
        # 300 points: a spatial index of three levels (GDAL sorts the features along a
        # Hilbert curve).
        pts = fc([({"id": k, "value": k * 0.5, "label": f"p{k}"}, {"type": "Point", "coordinates": [(k * 37) % 101 - 50.0, (k * 53) % 89 - 44.0]}) for k in range(300)])
        tmp = os.path.join(out, "_points.geojson")
        json.dump(pts, open(tmp, "w"))
        p = os.path.join(out, "points.fgb")
        if os.path.exists(p):
            os.remove(p)
        translate(tmp, p, "-f", "FlatGeobuf")
        os.remove(tmp)
    if only is None or "gml" in only:
        for ext in ("gml", "xsd"):
            if os.path.exists(os.path.join(out, "world." + ext)):
                os.remove(os.path.join(out, "world." + ext))
        translate(os.path.join(out, "world.gpkg"), os.path.join(out, "world.gml"), "-f", "GML")
        # GML 2 (coordinates as "x,y" tuples, outer/innerBoundaryIs, longitude first).
        for ext in ("gml", "xsd"):
            if os.path.exists(os.path.join(out, "world2." + ext)):
                os.remove(os.path.join(out, "world2." + ext))
        translate(os.path.join(out, "world.gpkg"), os.path.join(out, "world2.gml"), "-f", "GML", "-dsco", "FORMAT=GML2")
    if only is None or "kmz" in only:
        import zipfile
        with zipfile.ZipFile(os.path.join(out, "world.kmz"), "w", zipfile.ZIP_DEFLATED) as z:
            z.write(os.path.join(out, "world.kml"), "doc.kml")
    if only is None or "mapinfo" in only:
        # A detailed layer for MapInfo (mixed geometry types at irregular coordinates in a
        # small area, so MapInfo stores them compressed, rounded to its integer grid; long lines
        # spanning several coordinate blocks; multipoints; a two-point line; no geometry).
        import math
        feats = []
        x0, y0 = 12.3456789123, 45.6789012345
        for k in range(40):
            cx, cy = x0 + (k % 8) * 0.0123457, y0 + (k // 8) * 0.0098765
            r = 0.003 + 0.0007 * math.sin(k)
            ring = [[cx + r * math.cos(t * 2 * math.pi / 7 + k), cy + 0.8 * r * math.sin(t * 2 * math.pi / 7 + k)] for t in range(7)]
            rings = [ring + [ring[0]]]
            if k % 5 == 0:
                hole = [[cx + 0.3 * r * math.cos(-t * 2 * math.pi / 5), cy + 0.3 * r * math.sin(-t * 2 * math.pi / 5)] for t in range(5)]
                rings.append(hole + [hole[0]])
            feats.append(({"kind": "lot", "id": k, "size": round(r * 1e5, 3)}, {"type": "Polygon", "coordinates": rings}))
        for k in range(4):
            line = [[x0 + 0.1 * t / 300, y0 + 0.002 * k + 0.01 * math.sin(t / 17 + k)] for t in range(300)]
            feats.append(({"kind": "road", "id": 100 + k, "size": 300.0}, {"type": "LineString", "coordinates": line}))
        feats.append(({"kind": "pair", "id": 200, "size": 2.0}, {"type": "LineString", "coordinates": [[x0, y0], [x0 + 0.0311, y0 + 0.0222]]}))
        feats.append(({"kind": "forks", "id": 201, "size": 5.0}, {"type": "MultiLineString", "coordinates": [[[x0 + 0.01, y0], [x0 + 0.02, y0 + 0.013]], [[x0 + 0.03, y0 + 0.001], [x0 + 0.025, y0 + 0.007], [x0 + 0.02, y0 + 0.013]]]}))
        for k in range(3):
            feats.append(({"kind": "wells", "id": 300 + k, "size": 1.5 * k}, {"type": "MultiPoint", "coordinates": [[x0 + 0.001 * j + 0.0371 * k, y0 + 0.0013 * j * j] for j in range(4 + k)]}))
        feats.append(({"kind": "marker", "id": 400, "size": 0.25}, {"type": "Point", "coordinates": [x0 + 0.04567, y0 + 0.03456]}))
        feats.append(({"kind": "none", "id": 500, "size": 0.0}, None))
        tmp = os.path.join(out, "_detail.geojson")
        json.dump(fc(feats), open(tmp, "w"))
        for ext in ("tab", "map", "id", "dat", "mif", "mid"):
            q = os.path.join(out, f"detail.{ext}")
            if os.path.exists(q):
                os.remove(q)
        translate(tmp, os.path.join(out, "detail.tab"), "-f", "MapInfo File")
        translate(tmp, os.path.join(out, "detail.mif"), "-f", "MapInfo File", "-dsco", "FORMAT=MIF")
        os.remove(tmp)
        for n in layers:
            for ext in ("tab", "map", "id", "dat", "mif", "mid"):
                q = os.path.join(out, f"{n}.{ext}")
                if os.path.exists(q):
                    os.remove(q)
            translate(os.path.join(out, n + ".geojson"), os.path.join(out, n + ".tab"), "-f", "MapInfo File")
            translate(os.path.join(out, n + ".geojson"), os.path.join(out, n + ".mif"), "-f", "MapInfo File", "-dsco", "FORMAT=MIF")
    if only is None or "gdb" in only:
        # An Esri file geodatabase (a folder) holding the three layers and the detail layer.
        import shutil
        gdb = os.path.join(out, "world.gdb")
        if os.path.exists(gdb):
            shutil.rmtree(gdb)
        # A geodatabase table holds one geometry type: multi types throughout, and the mixed
        # detail layer split into its areas, lines and points.
        kinds = {"countries": "MULTIPOLYGON", "rivers": "MULTILINESTRING", "cities": "MULTIPOINTZ"}
        jobs = [(os.path.join(out, n + ".geojson"), n, kinds[n], []) for n in layers]
        mif = os.path.join(out, "detail.mif")
        jobs += [(mif, "lots", "MULTIPOLYGON", ["-where", "OGR_GEOMETRY IN ('POLYGON', 'MULTIPOLYGON')"]),
                 (mif, "roads", "MULTILINESTRING", ["-where", "OGR_GEOMETRY IN ('LINESTRING', 'MULTILINESTRING')"]),
                 (mif, "wells", "MULTIPOINT", ["-where", "OGR_GEOMETRY IN ('POINT', 'MULTIPOINT')"])]
        for i, (src, n, kind, extra) in enumerate(jobs):
            translate(src, gdb, "-f", "OpenFileGDB", *(["-update"] if i else []), "-nln", n, "-nlt", kind, *extra)
if only is None:
    old()
new()
for f in sorted(os.listdir(out)):
    print(f, os.path.getsize(os.path.join(out, f)))
