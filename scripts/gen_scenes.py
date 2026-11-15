#!/usr/bin/env python3
"""Write testdata/scenes/ and testdata/lidar/ (numpy only):

- scenes/tetra.off: a tetrahedron with a color per vertex (COFF) and one face color;
- scenes/boxes.gltf and boxes.glb: two unit boxes (12 triangles each) placed by nodes at x = 0 and
  x = 3 (the second turned 90 degrees about z and scaled by 2), with normals and vertex colors;
- scenes/scene.wrl: VRML 2.0 with a red box face set inside two transforms and a line set;
- lidar/terrain.las: 20 000 lidar points (LAS 1.2, point format 3) over 100 m x 100 m: ground on a
  smooth hill (class 2), trees (class 5) and a building roof (class 6), intensities, two returns,
  GPS times and colors.

    python3 scripts/gen_scenes.py
"""
import base64
import json
import math
import os
import struct

import numpy as np

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata")


def off():
    p = os.path.join(ROOT, "scenes", "tetra.off")
    open(p, "w").write(
        "COFF\n# a tetrahedron, a color per vertex\n4 4 6\n"
        "0 0 0 255 0 0 255\n1 0 0 0 255 0 255\n0 1 0 0 0 255 255\n0 0 1 255 255 255 255\n"
        "3 0 2 1\n3 0 1 3 255 0 0\n3 1 2 3\n3 0 3 2\n"
    )


def box():
    # 24 vertices (4 per face) for flat normals.
    faces = [((0, 0, -1), [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0)]), ((0, 0, 1), [(0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1)]),
             ((0, -1, 0), [(0, 0, 0), (1, 0, 0), (1, 0, 1), (0, 0, 1)]), ((0, 1, 0), [(0, 1, 0), (1, 1, 0), (1, 1, 1), (0, 1, 1)]),
             ((-1, 0, 0), [(0, 0, 0), (0, 1, 0), (0, 1, 1), (0, 0, 1)]), ((1, 0, 0), [(1, 0, 0), (1, 1, 0), (1, 1, 1), (1, 0, 1)])]
    pos, nrm, idx = [], [], []
    for n, quad in faces:
        b = len(pos)
        pos += quad
        nrm += [n] * 4
        idx += [b, b + 1, b + 2, b, b + 2, b + 3]
    return np.array(pos, "<f4"), np.array(nrm, "<f4"), np.array(idx, "<u2")


def gltf():
    pos, nrm, idx = box()
    col = np.array([[0.9, 0.5, 0.1]] * len(pos), "<f4")
    blobs = [pos.tobytes(), nrm.tobytes(), col.tobytes(), idx.tobytes()]
    views, off = [], 0
    data = b""
    for b in blobs:
        views.append({"buffer": 0, "byteOffset": off, "byteLength": len(b)})
        data += b
        off += len(b)
        while len(data) % 4:
            data += b"\0"
            off += 1
    accessors = [
        {"bufferView": 0, "componentType": 5126, "count": len(pos), "type": "VEC3", "min": [0, 0, 0], "max": [1, 1, 1]},
        {"bufferView": 1, "componentType": 5126, "count": len(pos), "type": "VEC3"},
        {"bufferView": 2, "componentType": 5126, "count": len(pos), "type": "VEC3"},
        {"bufferView": 3, "componentType": 5123, "count": len(idx), "type": "SCALAR"},
    ]
    h = math.sqrt(0.5)
    doc = {
        "asset": {"version": "2.0", "generator": "auraFX test generator"},
        "scene": 0,
        "scenes": [{"nodes": [0, 1]}],
        "nodes": [{"name": "box A", "mesh": 0}, {"name": "box B", "mesh": 0, "translation": [3, 0, 0], "rotation": [0, 0, h, h], "scale": [2, 2, 2]}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0, "NORMAL": 1, "COLOR_0": 2}, "indices": 3}]}],
        "accessors": accessors,
        "bufferViews": views,
        "buffers": [{"byteLength": len(data)}],
    }
    d2 = dict(doc)
    d2["buffers"] = [{"byteLength": len(data), "uri": "data:application/octet-stream;base64," + base64.b64encode(data).decode()}]
    open(os.path.join(ROOT, "scenes", "boxes.gltf"), "w").write(json.dumps(d2, indent=1))
    j = json.dumps(doc).encode()
    while len(j) % 4:
        j += b" "
    glb = b"glTF" + struct.pack("<II", 2, 12 + 8 + len(j) + 8 + len(data)) + struct.pack("<I", len(j)) + b"JSON" + j + struct.pack("<I", len(data)) + b"BIN\0" + data
    open(os.path.join(ROOT, "scenes", "boxes.glb"), "wb").write(glb)


def vrml():
    open(os.path.join(ROOT, "scenes", "scene.wrl"), "w").write("""#VRML V2.0 utf8
# A red box face set moved by two transforms, and a line set.
Transform { translation 10 0 0 children [
  Transform { scale 2 2 2 rotation 0 0 1 1.5707963 children [
    Shape {
      appearance Appearance { material Material { diffuseColor 1 0 0 } }
      geometry IndexedFaceSet {
        coord Coordinate { point [ 0 0 0, 1 0 0, 1 1 0, 0 1 0, 0 0 1, 1 0 1, 1 1 1, 0 1 1 ] }
        coordIndex [ 0 3 2 1 -1, 4 5 6 7 -1, 0 1 5 4 -1, 2 3 7 6 -1, 1 2 6 5 -1, 3 0 4 7 -1 ]
        color Color { color [ 1 0 0, 0 1 0 ] }
        colorIndex [ 0 0 1 1 0 1 ]
        colorPerVertex FALSE
      }
    }
  ] }
  Shape { geometry IndexedLineSet { coord Coordinate { point [ 0 0 0, 0 0 3, 3 0 3 ] } coordIndex [ 0 1 2 -1 ] } }
] }
""")


def las():
    rng = np.random.default_rng(7)
    n = 20000
    x = rng.uniform(0, 100, n)
    y = rng.uniform(0, 100, n)
    ground = 5 * np.exp(-((x - 40) ** 2 + (y - 60) ** 2) / 800)
    z = ground.copy()
    cls = np.full(n, 2, np.uint8)
    roof = (x > 65) & (x < 85) & (y > 15) & (y < 35)
    z[roof] = ground[roof] + 8
    cls[roof] = 6
    trees = np.zeros(n, bool)
    for cx, cy in [(20, 20), (25, 80), (55, 40), (80, 75), (40, 10)]:
        r = np.hypot(x - cx, y - cy)
        t = (r < 6) & ~roof
        z[t] = ground[t] + 10 - 1.2 * r[t] + rng.uniform(-0.5, 0.5, t.sum())
        cls[t] = 5
        trees |= t
    inten = (200 + 50 * cls + rng.integers(0, 40, n)).astype(np.uint16)
    rgb = np.zeros((n, 3), np.uint16)
    rgb[cls == 2] = [110 * 256, 140 * 256, 70 * 256]
    rgb[cls == 5] = [30 * 256, 110 * 256, 40 * 256]
    rgb[cls == 6] = [170 * 256, 60 * 256, 50 * 256]
    scale = (0.001, 0.001, 0.001)
    offset = (0.0, 0.0, 0.0)
    head = bytearray(227)
    head[0:4] = b"LASF"
    head[24], head[25] = 1, 2
    head[26:58] = b"auraFX".ljust(32, b"\0")
    head[58:90] = b"auraFX test generator".ljust(32, b"\0")
    struct.pack_into("<HI", head, 94, 227, 227)
    struct.pack_into("<I", head, 100, 0)
    head[104] = 3
    struct.pack_into("<HI", head, 105, 34, n)
    struct.pack_into("<3d", head, 131, *scale)
    struct.pack_into("<3d", head, 155, *offset)
    struct.pack_into("<6d", head, 179, x.max(), x.min(), y.max(), y.min(), z.max(), z.min())
    rec = np.zeros(n, dtype=[("x", "<i4"), ("y", "<i4"), ("z", "<i4"), ("i", "<u2"), ("ret", "u1"), ("cls", "u1"), ("ang", "i1"), ("ud", "u1"), ("src", "<u2"), ("gps", "<f8"), ("r", "<u2"), ("g", "<u2"), ("b", "<u2")])
    rec["x"] = np.round(x / scale[0]).astype(np.int32)
    rec["y"] = np.round(y / scale[1]).astype(np.int32)
    rec["z"] = np.round(z / scale[2]).astype(np.int32)
    rec["i"] = inten
    rec["ret"] = np.where(trees, 1 | (2 << 3), 1 | (1 << 3))
    rec["cls"] = cls
    rec["ang"] = np.round((x - 50) / 5).astype(np.int8)
    rec["gps"] = np.arange(n) * 1e-4
    rec["r"], rec["g"], rec["b"] = rgb[:, 0], rgb[:, 1], rgb[:, 2]
    open(os.path.join(ROOT, "lidar", "terrain.las"), "wb").write(bytes(head) + rec.tobytes())


def main():
    os.makedirs(os.path.join(ROOT, "scenes"), exist_ok=True)
    os.makedirs(os.path.join(ROOT, "lidar"), exist_ok=True)
    off()
    gltf()
    vrml()
    las()
    print("wrote", os.path.normpath(os.path.join(ROOT, "scenes")), "and lidar")


main()
