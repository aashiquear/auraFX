#!/usr/bin/env python3
"""Small NIfTI and PTS files for the readers' tests, written byte by byte (numpy only):

- testdata/volumes/head.nii.gz: NIfTI-1, int16, 6 x 5 x 4 voxels of value i + 10 j + 100 k,
  scl_slope 2 and scl_inter -5, an sform that reverses x and swaps y and z
  (world x = 30 - 2 i, y = 1.5 k, z = 3 j + 4).
- testdata/volumes/vectors.nii: NIfTI-2, float32, 4 x 3 x 2 voxels with 3 components (dim 5),
  component c of voxel (i, j, k) = c + i + 10 j + 100 k, voxel sizes 0.5 0.5 1, no sform or qform.
- testdata/volumes/pair.hdr + pair.img: NIfTI-1 header and data apart, big endian, uint8,
  3 x 2 x 2 voxels of value 1 + i + 3 j + 6 k, a qform turning 90 degrees about z
  (quatern_d = sqrt(1/2)), voxel sizes 2 2 2, offset (10, 0, 0).
- testdata/lidar/scan.pts: two scans (3 and 2 points) with intensity and RGB.

    python3 scripts/gen_nifti.py
"""
import gzip
import os
import struct

import numpy as np

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata")


def nifti1(dims, datatype, bitpix, pixdim, slope=0.0, inter=0.0, qform=None, sform=None, single=True, endian="<"):
    h = bytearray(348)
    e = endian
    struct.pack_into(e + "i", h, 0, 348)
    d = [len(dims)] + list(dims) + [1] * (7 - len(dims))
    struct.pack_into(e + "8h", h, 40, *d)
    struct.pack_into(e + "h", h, 70, datatype)
    struct.pack_into(e + "h", h, 72, bitpix)
    struct.pack_into(e + "8f", h, 76, *pixdim)
    struct.pack_into(e + "f", h, 108, 352.0 if single else 0.0)
    struct.pack_into(e + "ff", h, 112, slope, inter)
    if qform is not None:
        struct.pack_into(e + "h", h, 252, 1)
        struct.pack_into(e + "6f", h, 256, *qform)
    if sform is not None:
        struct.pack_into(e + "h", h, 254, 1)
        struct.pack_into(e + "12f", h, 280, *[v for row in sform for v in row])
    h[344:348] = b"n+1\0" if single else b"ni1\0"
    return bytes(h)


def nifti2(dims, datatype, bitpix, pixdim):
    h = bytearray(540)
    struct.pack_into("<i", h, 0, 540)
    h[4:12] = b"n+2\0\r\n\x1a\n"
    struct.pack_into("<h", h, 12, datatype)
    struct.pack_into("<h", h, 14, bitpix)
    d = [len(dims)] + list(dims) + [1] * (7 - len(dims))
    struct.pack_into("<8q", h, 16, *d)
    struct.pack_into("<8d", h, 104, *pixdim)
    struct.pack_into("<q", h, 168, 544)
    struct.pack_into("<dd", h, 176, 0.0, 0.0)
    return bytes(h)


vol = os.path.join(ROOT, "volumes")
os.makedirs(vol, exist_ok=True)
nx, ny, nz = 6, 5, 4
v = np.array([[[i + 10 * j + 100 * k for i in range(nx)] for j in range(ny)] for k in range(nz)], dtype="<i2")
hdr = nifti1([nx, ny, nz], 4, 16, [1, 2, 3, 1.5, 1, 1, 1, 1], slope=2.0, inter=-5.0,
             sform=[[-2, 0, 0, 30], [0, 0, 1.5, 0], [0, 3, 0, 4]])
with gzip.open(os.path.join(vol, "head.nii.gz"), "wb") as f:
    f.write(hdr + b"\0\0\0\0" + v.tobytes())

nx, ny, nz = 4, 3, 2
comps = np.array([[[[c + i + 10 * j + 100 * k for i in range(nx)] for j in range(ny)] for k in range(nz)] for c in range(3)], dtype="<f4")
with open(os.path.join(vol, "vectors.nii"), "wb") as f:
    f.write(nifti2([nx, ny, nz, 1, 3], 16, 32, [1, 0.5, 0.5, 1, 1, 1, 1, 1]) + b"\0\0\0\0" + comps.tobytes())

nx, ny, nz = 3, 2, 2
u = np.array([[[1 + i + 3 * j + 6 * k for i in range(nx)] for j in range(ny)] for k in range(nz)], dtype="u1")
with open(os.path.join(vol, "pair.hdr"), "wb") as f:
    f.write(nifti1([nx, ny, nz], 2, 8, [1, 2, 2, 2, 1, 1, 1, 1], qform=[0, 0, 0.5 ** 0.5, 10, 0, 0], single=False, endian=">"))
with open(os.path.join(vol, "pair.img"), "wb") as f:
    f.write(u.tobytes())

lid = os.path.join(ROOT, "lidar")
os.makedirs(lid, exist_ok=True)
with open(os.path.join(lid, "scan.pts"), "w") as f:
    f.write("3\n0 0 0 10 255 0 0\n1 0 0 20 0 255 0\n0 1 0 30 0 0 255\n2\n0 0 1 40 10 20 30\n1 1 1 50 40 50 60\n")
print("wrote head.nii.gz, vectors.nii, pair.hdr/.img, scan.pts")
