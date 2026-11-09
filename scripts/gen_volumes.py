#!/usr/bin/env python3
"""Write the small volume files of testdata/volumes/ byte by byte (numpy only), one per format
and variant the readers handle: NRRD (raw, gzip with space directions, a detached .nhdr), MetaImage
(.mha, .mhd with a zlib .zraw), MRC (float32 and int16 with a permuted axis order), a DICOM series
(three slices, explicit VR little endian, int16 with rescale slope/intercept) and an OME-TIFF
(Z 3, C 2, T 2). Every volume holds v = 100*k + 10*j + i (+ 1000*c + 5000*t for OME-TIFF), so a
value tells its own position.

    python3 scripts/gen_volumes.py            # writes testdata/volumes/
"""
import gzip
import os
import struct
import sys
import zlib

import numpy as np

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "testdata", "volumes")
NX, NY, NZ = 4, 3, 2


def field(dtype):
    k, j, i = np.meshgrid(np.arange(NZ), np.arange(NY), np.arange(NX), indexing="ij")
    return (100 * k + 10 * j + i).astype(dtype)  # x fastest when raveled


def nrrd():
    v = field(np.int16)
    head = ("NRRD0004\n# auraFX test volume\ntype: short\ndimension: 3\n"
            f"sizes: {NX} {NY} {NZ}\nspacings: 0.5 1 2\naxis mins: 1 2 3\nendian: little\nencoding: raw\n\n")
    open(os.path.join(OUT, "raw.nrrd"), "wb").write(head.encode() + v.astype("<i2").tobytes())
    f = field(np.float32)
    head = ("NRRD0005\ntype: float\ndimension: 3\n"
            f"sizes: {NX} {NY} {NZ}\nspace: right-anterior-superior\n"
            "space directions: (0,0.5,0) (-1,0,0) (0,0,2)\nspace origin: (10,20,30)\n"
            "kinds: domain domain domain\nendian: big\nencoding: gzip\n\n")
    open(os.path.join(OUT, "gzip.nrrd"), "wb").write(head.encode() + gzip.compress(f.astype(">f4").tobytes()))
    # Detached header with a vector (3 components on the first axis).
    vec = np.stack([field(np.float64), -field(np.float64), np.ones((NZ, NY, NX))], axis=-1)
    open(os.path.join(OUT, "vector.raw"), "wb").write(vec.astype("<f8").tobytes())
    head = ("NRRD0004\ntype: double\ndimension: 4\n"
            f"sizes: 3 {NX} {NY} {NZ}\nkinds: 3-vector domain domain domain\nspacings: nan 1 1 1\n"
            "endian: little\nencoding: raw\ndata file: vector.raw\n\n")
    open(os.path.join(OUT, "vector.nhdr"), "w").write(head)


def metaimage():
    v = field(np.uint16)
    head = ("ObjectType = Image\nNDims = 3\nBinaryData = True\nBinaryDataByteOrderMSB = False\n"
            "CompressedData = False\nTransformMatrix = 1 0 0 0 1 0 0 0 1\nOffset = -1 -2 -3\n"
            f"ElementSpacing = 0.25 0.5 1\nDimSize = {NX} {NY} {NZ}\nElementType = MET_USHORT\n"
            "ElementDataFile = LOCAL\n")
    open(os.path.join(OUT, "local.mha"), "wb").write(head.encode() + v.astype("<u2").tobytes())
    f = field(np.float32)
    z = zlib.compress(f.astype("<f4").tobytes())
    head = ("ObjectType = Image\nNDims = 3\nBinaryData = True\nBinaryDataByteOrderMSB = False\n"
            f"CompressedData = True\nCompressedDataSize = {len(z)}\nOffset = 0 0 0\n"
            f"ElementSpacing = 1 1 1\nDimSize = {NX} {NY} {NZ}\nElementType = MET_FLOAT\n"
            "ElementDataFile = zipped.zraw\n")
    open(os.path.join(OUT, "zipped.mhd"), "w").write(head)
    open(os.path.join(OUT, "zipped.zraw"), "wb").write(z)


def mrc_header(nx, ny, nz, mode, cell, mapcrs, origin=(0.0, 0.0, 0.0)):
    h = bytearray(1024)
    # nx ny nz mode, nxstart nystart nzstart, mx my mz, cell lengths and angles, axis order.
    struct.pack_into("<4i", h, 0, nx, ny, nz, mode)
    struct.pack_into("<3i", h, 16, 0, 0, 0)
    # mx, my, mz: samples along x, y, z (the cell holds them).
    sizes = [0, 0, 0]
    for k, m in enumerate(mapcrs):
        sizes[m - 1] = (nx, ny, nz)[k]
    struct.pack_into("<3i", h, 28, *sizes)
    struct.pack_into("<3f", h, 40, *cell)
    struct.pack_into("<3f", h, 52, 90.0, 90.0, 90.0)
    struct.pack_into("<3i", h, 64, *mapcrs)
    struct.pack_into("<3f", h, 196, *origin)
    h[208:212] = b"MAP "
    h[212:216] = bytes([0x44, 0x44, 0, 0])
    return bytes(h)


def mrc():
    v = field(np.float32)
    h = mrc_header(NX, NY, NZ, 2, (NX * 1.5, NY * 1.5, NZ * 1.5), (1, 2, 3), origin=(3.0, 4.0, 5.0))
    open(os.path.join(OUT, "float.mrc"), "wb").write(h + v.astype("<f4").tobytes())
    # int16 with columns along y, rows along x, sections along z: stored v[k][i][j].
    s = field(np.int16).transpose(0, 2, 1)
    h = mrc_header(NY, NX, NZ, 1, (NX * 1.0, NY * 1.0, NZ * 1.0), (2, 1, 3))
    open(os.path.join(OUT, "permuted.mrc"), "wb").write(h + s.astype("<i2").tobytes())


def dicom_element(group, elem, vr, value):
    if isinstance(value, str):
        value = value.encode()
        if len(value) % 2:
            value += b"\0" if vr == b"UI" else b" "
    long_vr = vr in (b"OB", b"OW", b"SQ", b"UN", b"UT", b"OF", b"OD", b"OL", b"UC", b"UR")
    if long_vr:
        return struct.pack("<HH2sHI", group, elem, vr, 0, len(value)) + value
    return struct.pack("<HH2sH", group, elem, vr, len(value)) + value


def dicom():
    series = "1.2.826.0.1.3680043.2.1125.1.42"
    stored = field(np.int16)  # stored = (value + 1024) / 2, so value = 2 * stored - 1024
    os.makedirs(os.path.join(OUT, "dicom"), exist_ok=True)
    for k in range(NZ + 1):
        sl = stored[min(k, NZ - 1)] if k < NZ else stored[0] + 7
        # Row j of the picture is the field's row j (rows run along the column direction, +y).
        pixels = sl.astype("<i2").tobytes()
        meta_body = (dicom_element(0x0002, 0x0001, b"OB", b"\0\1")
                     + dicom_element(0x0002, 0x0002, b"UI", "1.2.840.10008.5.1.4.1.1.2")
                     + dicom_element(0x0002, 0x0003, b"UI", f"{series}.{k + 1}")
                     + dicom_element(0x0002, 0x0010, b"UI", "1.2.840.10008.1.2.1"))
        meta = dicom_element(0x0002, 0x0000, b"UL", struct.pack("<I", len(meta_body))) + meta_body
        body = (dicom_element(0x0008, 0x0060, b"CS", "CT")
                + dicom_element(0x0018, 0x0050, b"DS", "2.5")
                # A sequence of undefined length to skip.
                + struct.pack("<HH2sHI", 0x0008, 0x1140, b"SQ", 0, 0xFFFFFFFF)
                + struct.pack("<HHI", 0xFFFE, 0xE000, 0xFFFFFFFF)
                + dicom_element(0x0008, 0x1150, b"UI", "1.2.3")
                + struct.pack("<HHI", 0xFFFE, 0xE00D, 0)
                + struct.pack("<HHI", 0xFFFE, 0xE0DD, 0)
                + dicom_element(0x0020, 0x000E, b"UI", series if k < NZ else series + ".9")
                + dicom_element(0x0020, 0x0013, b"IS", str(NZ - k))
                # Positions out of order on purpose: the reader sorts along the normal.
                + dicom_element(0x0020, 0x0032, b"DS", f"-5\\-6\\{7 + 2.5 * k}")
                + dicom_element(0x0020, 0x0037, b"DS", "1\\0\\0\\0\\1\\0")
                + dicom_element(0x0028, 0x0002, b"US", struct.pack("<H", 1))
                + dicom_element(0x0028, 0x0004, b"CS", "MONOCHROME2")
                + dicom_element(0x0028, 0x0010, b"US", struct.pack("<H", NY))
                + dicom_element(0x0028, 0x0011, b"US", struct.pack("<H", NX))
                + dicom_element(0x0028, 0x0030, b"DS", "0.75\\0.5")
                + dicom_element(0x0028, 0x0100, b"US", struct.pack("<H", 16))
                + dicom_element(0x0028, 0x0101, b"US", struct.pack("<H", 16))
                + dicom_element(0x0028, 0x0102, b"US", struct.pack("<H", 15))
                + dicom_element(0x0028, 0x0103, b"US", struct.pack("<H", 1))
                + dicom_element(0x0028, 0x1052, b"DS", "-1024")
                + dicom_element(0x0028, 0x1053, b"DS", "2")
                + dicom_element(0x7FE0, 0x0010, b"OW", pixels))
        name = f"slice{(k * 7) % 3}.dcm" if k < NZ else "other_series.dcm"
        open(os.path.join(OUT, "dicom", name), "wb").write(b"\0" * 128 + b"DICM" + meta + body)


def tiff(path, planes, description):
    """Baseline multi-page TIFF, uint16, one strip per page, little endian."""
    h, w = planes[0].shape
    out = bytearray(b"II*\0" + struct.pack("<I", 0))
    datas = [p.astype("<u2").tobytes() for p in planes]
    desc = description.encode() + b"\0"
    # Data and description first, IFDs after.
    data_pos = []
    for d in datas:
        data_pos.append(len(out))
        out += d
    desc_pos = len(out)
    out += desc
    if len(out) % 2:
        out += b"\0"
    ifd_pos = []
    for n, d in enumerate(datas):
        entries = [(256, 4, 1, w), (257, 4, 1, h), (258, 3, 1, 16), (259, 3, 1, 1), (262, 3, 1, 1)]
        if n == 0:
            entries.append((270, 2, len(desc), desc_pos))
        entries += [(273, 4, 1, data_pos[n]), (277, 3, 1, 1), (278, 4, 1, h), (279, 4, 1, len(d)), (339, 3, 1, 1)]
        entries.sort()
        ifd_pos.append(len(out))
        out += struct.pack("<H", len(entries))
        for tag, typ, count, val in entries:
            if typ == 3 and count == 1:
                out += struct.pack("<HHIHH", tag, typ, count, val, 0)
            else:
                out += struct.pack("<HHII", tag, typ, count, val)
        out += struct.pack("<I", 0)
    # Link the IFDs.
    for n, pos in enumerate(ifd_pos):
        count = struct.unpack_from("<H", out, pos)[0]
        nxt = ifd_pos[n + 1] if n + 1 < len(ifd_pos) else 0
        struct.pack_into("<I", out, pos + 2 + 12 * count, nxt)
    struct.pack_into("<I", out, 4, ifd_pos[0])
    open(path, "wb").write(bytes(out))


def ome():
    sz, sc, st = NZ + 1, 2, 2
    planes = []
    # DimensionOrder XYCZT: c fastest, then z, then t.
    for t in range(st):
        for z in range(sz):
            for c in range(sc):
                j, i = np.meshgrid(np.arange(NY), np.arange(NX), indexing="ij")
                v = 100 * z + 10 * j + i + 1000 * c + 5000 * t
                planes.append(v[::-1])  # rows top first
    xml = ('<?xml version="1.0" encoding="UTF-8"?><OME xmlns="http://www.openmicroscopy.org/Schemas/OME/2016-06">'
           '<Image ID="Image:0" Name="test"><Pixels ID="Pixels:0" DimensionOrder="XYCZT" Type="uint16" '
           f'SizeX="{NX}" SizeY="{NY}" SizeZ="{sz}" SizeC="{sc}" SizeT="{st}" '
           'PhysicalSizeX="0.2" PhysicalSizeY="0.3" PhysicalSizeZ="1.5" TimeIncrement="2.0">'
           '<Channel ID="Channel:0:0" Name="DAPI" SamplesPerPixel="1"/>'
           '<Channel ID="Channel:0:1" Name="GFP" SamplesPerPixel="1"/>'
           f'<TiffData IFD="0" PlaneCount="{sz * sc * st}"/></Pixels></Image></OME>')
    tiff(os.path.join(OUT, "cells.ome.tif"), planes, xml)


def phantom():
    """A 64 x 64 x 40 CT-like phantom (Hounsfield units) as a DICOM series in dicom_phantom/: a
    head-shaped ellipsoid of soft tissue (40 HU) in a skull shell (1000 HU), two ventricles (5 HU)
    and a dense lesion (80 HU) in air (-1000 HU); 1 mm pixels, 2 mm slices."""
    n, nz = 64, 40
    series = "1.2.826.0.1.3680043.2.1125.2.7"
    folder = os.path.join(OUT, "dicom_phantom")
    os.makedirs(folder, exist_ok=True)
    y, x = np.meshgrid(np.arange(n) - 31.5, np.arange(n) - 31.5, indexing="ij")
    for k in range(nz):
        z = (k - 19.5) * 2.0
        r = (x / 28) ** 2 + (y / 24) ** 2 + (z / 36) ** 2
        hu = np.full((n, n), -1000.0)
        hu[r < 1.0] = 1000.0
        hu[r < 0.85] = 40.0
        for cx in (-7, 7):
            hu[((x - cx) / 4) ** 2 + ((y + 3) / 9) ** 2 + (z / 12) ** 2 < 1] = 5.0
        hu[((x - 12) / 5) ** 2 + ((y - 10) / 5) ** 2 + ((z - 8) / 6) ** 2 < 1] = 80.0
        stored = np.round((hu + 1024) / 2).astype("<i2")
        meta_body = (dicom_element(0x0002, 0x0001, b"OB", b"\0\1")
                     + dicom_element(0x0002, 0x0002, b"UI", "1.2.840.10008.5.1.4.1.1.2")
                     + dicom_element(0x0002, 0x0003, b"UI", f"{series}.{k + 1}")
                     + dicom_element(0x0002, 0x0010, b"UI", "1.2.840.10008.1.2.1"))
        meta = dicom_element(0x0002, 0x0000, b"UL", struct.pack("<I", len(meta_body))) + meta_body
        body = (dicom_element(0x0008, 0x0060, b"CS", "CT")
                + dicom_element(0x0018, 0x0050, b"DS", "2")
                + dicom_element(0x0020, 0x000E, b"UI", series)
                + dicom_element(0x0020, 0x0013, b"IS", str(k + 1))
                + dicom_element(0x0020, 0x0032, b"DS", f"-32\\-32\\{z:.1f}")
                + dicom_element(0x0020, 0x0037, b"DS", "1\\0\\0\\0\\1\\0")
                + dicom_element(0x0028, 0x0002, b"US", struct.pack("<H", 1))
                + dicom_element(0x0028, 0x0004, b"CS", "MONOCHROME2")
                + dicom_element(0x0028, 0x0010, b"US", struct.pack("<H", n))
                + dicom_element(0x0028, 0x0011, b"US", struct.pack("<H", n))
                + dicom_element(0x0028, 0x0030, b"DS", "1\\1")
                + dicom_element(0x0028, 0x0100, b"US", struct.pack("<H", 16))
                + dicom_element(0x0028, 0x0101, b"US", struct.pack("<H", 16))
                + dicom_element(0x0028, 0x0102, b"US", struct.pack("<H", 15))
                + dicom_element(0x0028, 0x0103, b"US", struct.pack("<H", 1))
                + dicom_element(0x0028, 0x1052, b"DS", "-1024")
                + dicom_element(0x0028, 0x1053, b"DS", "2")
                + dicom_element(0x7FE0, 0x0010, b"OW", stored.tobytes()))
        open(os.path.join(folder, f"ct{k:03d}.dcm"), "wb").write(b"\0" * 128 + b"DICM" + meta + body)


def main():
    os.makedirs(OUT, exist_ok=True)
    phantom()
    nrrd()
    metaimage()
    mrc()
    dicom()
    ome()
    print("wrote", os.path.normpath(OUT))


if __name__ == "__main__":
    sys.exit(main())
