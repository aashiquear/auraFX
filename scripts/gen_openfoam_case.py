"""Write a tiny OpenFOAM case (structured hex block) in ASCII or binary, with two time steps.

usage: python3 scripts/gen_openfoam_case.py <case_dir> [ascii|binary]
"""
import os, struct, sys

case = sys.argv[1]
fmt = sys.argv[2] if len(sys.argv) > 2 else "ascii"
nx, ny, nz = 4, 3, 2
os.makedirs(f"{case}/constant/polyMesh", exist_ok=True)
for t in ("0", "1"):
    os.makedirs(f"{case}/{t}", exist_ok=True)
open(f"{case}/case.foam", "w").close()
os.makedirs(f"{case}/system", exist_ok=True)
open(f"{case}/system/controlDict", "w").write("FoamFile { version 2.0; format ascii; class dictionary; object controlDict; }\napplication icoFoam;\n")

def pid(i, j, k): return i + (nx + 1) * (j + (ny + 1) * k)
def cid(i, j, k): return i + nx * (j + ny * k)
pts = [(i * 1.0, j * 1.0, k * 1.0) for k in range(nz + 1) for j in range(ny + 1) for i in range(nx + 1)]
internal, boundary = [], []  # (face pts, owner, neighbour)
for k in range(nz):
    for j in range(ny):
        for i in range(nx):
            c = cid(i, j, k)
            # +x face (normal +x): points ordered counter-clockwise seen from +x
            fx = [pid(i+1, j, k), pid(i+1, j+1, k), pid(i+1, j+1, k+1), pid(i+1, j, k+1)]
            fy = [pid(i, j+1, k), pid(i, j+1, k+1), pid(i+1, j+1, k+1), pid(i+1, j+1, k)]
            fz = [pid(i, j, k+1), pid(i+1, j, k+1), pid(i+1, j+1, k+1), pid(i, j+1, k+1)]
            for f, nb in ((fx, (i+1, j, k)), (fy, (i, j+1, k)), (fz, (i, j, k+1))):
                if nb[0] < nx and nb[1] < ny and nb[2] < nz:
                    internal.append((f, c, cid(*nb)))
                else:
                    boundary.append((f, c))
            if i == 0: boundary.append(([pid(0, j, k), pid(0, j, k+1), pid(0, j+1, k+1), pid(0, j+1, k)], c))
            if j == 0: boundary.append(([pid(i, 0, k), pid(i+1, 0, k), pid(i+1, 0, k+1), pid(i, 0, k+1)], c))
            if k == 0: boundary.append(([pid(i, j, 0), pid(i, j+1, 0), pid(i+1, j+1, 0), pid(i+1, j, 0)], c))
internal.sort(key=lambda f: (f[1], f[2]))
faces = [f for f, _, _ in internal] + [f for f, _ in boundary]
owner = [o for _, o, _ in internal] + [o for _, o in boundary]
neigh = [n for _, _, n in internal]
ncells = nx * ny * nz

def header(cls, obj, note=""):
    return f'FoamFile\n{{\n    version 2.0;\n    format {fmt};\n    arch "LSB;label=32;scalar=64";\n    class {cls};\n    {note}object {obj};\n}}\n\n'

def write_list(f, vals, kind):
    n = len(vals)
    if fmt == "binary":
        f.write(f"{n}\n(".encode())
        if kind == "label": f.write(struct.pack(f"<{n}i", *vals))
        elif kind == "scalar": f.write(struct.pack(f"<{n}d", *vals))
        else: f.write(struct.pack(f"<{3*n}d", *[c for v in vals for c in v]))
        f.write(b")\n")
    else:
        f.write(f"{n}\n(\n".encode())
        for v in vals:
            f.write((f"({v[0]} {v[1]} {v[2]})\n" if kind == "vector" else f"{v}\n").encode())
        f.write(b")\n")

with open(f"{case}/constant/polyMesh/points", "wb") as f:
    f.write(header("vectorField", "points").encode()); write_list(f, pts, "vector")
with open(f"{case}/constant/polyMesh/faces", "wb") as f:
    if fmt == "binary":
        f.write(header("faceCompactList", "faces").encode())
        offs = [0]
        for fc in faces: offs.append(offs[-1] + len(fc))
        write_list(f, offs, "label"); write_list(f, [p for fc in faces for p in fc], "label")
    else:
        f.write(header("faceList", "faces").encode())
        f.write(f"{len(faces)}\n(\n".encode())
        for fc in faces: f.write(f"{len(fc)}({' '.join(map(str, fc))})\n".encode())
        f.write(b")\n")
with open(f"{case}/constant/polyMesh/owner", "wb") as f:
    f.write(header("labelList", "owner", f'note "nPoints:{len(pts)} nCells:{ncells} nFaces:{len(faces)} nInternalFaces:{len(internal)}";\n    ').encode()); write_list(f, owner, "label")
with open(f"{case}/constant/polyMesh/neighbour", "wb") as f:
    f.write(header("labelList", "neighbour").encode()); write_list(f, neigh, "label")
nb = len(boundary)
open(f"{case}/constant/polyMesh/boundary", "w").write(header("polyBoundaryMesh", "boundary").replace(f"format {fmt}", "format ascii") + f"1\n(\n    walls\n    {{\n        type wall;\n        nFaces {nb};\n        startFace {len(internal)};\n    }}\n)\n")

centers = [((i + .5), (j + .5), (k + .5)) for k in range(nz) for j in range(ny) for i in range(nx)]
for t, scale in (("0", 0.0), ("1", 1.0)):
    with open(f"{case}/{t}/T", "wb") as f:
        f.write(header("volScalarField", "T").encode())
        f.write(b"dimensions [0 0 0 1 0 0 0];\n\n")
        if scale == 0.0:
            f.write(b"internalField uniform 300;\n\n")
        else:
            f.write(b"internalField nonuniform List<scalar> "); write_list(f, [300 + 10 * c[0] for c in centers], "scalar"); f.write(b";\n\n")
        f.write(b"boundaryField\n{\n    walls { type zeroGradient; }\n}\n")
    with open(f"{case}/{t}/U", "wb") as f:
        f.write(header("volVectorField", "U").encode())
        f.write(b"dimensions [0 1 -1 0 0 0 0];\n\n")
        if scale == 0.0:
            f.write(b"internalField uniform (1 0 0);\n\n")
        else:
            f.write(b"internalField nonuniform List<vector> "); write_list(f, [(c[0], c[1], c[2]) for c in centers], "vector"); f.write(b";\n\n")
        f.write(b"boundaryField\n{\n    walls { type noSlip; }\n}\n")
print(case, fmt, len(pts), "points", ncells, "cells", len(faces), "faces")
