#!/usr/bin/env python3
"""Set the SDK version an iOS executable says it was built with (its LC_BUILD_VERSION).

iOS decides some behaviour by that number. From the iOS 27 SDK on, an app that has not adopted
the scene life cycle is stopped at launch; the window library auraFX uses (winit) still starts
the app the earlier way, so the packaging scripts stamp the executable as built with the iOS 26
SDK, for which the system keeps the earlier behaviour. Remove this once winit adopts scenes.

usage: ios_sdk_stamp.py EXECUTABLE [SDK_VERSION, default 26.0]   (before signing)
"""
import struct
import sys

LC_BUILD_VERSION = 0x32


def stamp(path, version):
    parts = [int(x) for x in version.split(".")] + [0, 0]
    sdk = parts[0] << 16 | parts[1] << 8 | parts[2]
    with open(path, "r+b") as f:
        head = f.read(32)
        magic, _cpu, _sub, _type, ncmds, _size, _flags, _res = struct.unpack("<8I", head)
        if magic != 0xFEEDFACF:
            sys.exit(f"{path}: not a thin 64-bit Mach-O file")
        off = 32
        for _ in range(ncmds):
            f.seek(off)
            cmd, size = struct.unpack("<2I", f.read(8))
            if cmd == LC_BUILD_VERSION:
                platform, minos, old, _ntools = struct.unpack("<4I", f.read(16))
                f.seek(off + 16)
                f.write(struct.pack("<I", sdk))
                show = lambda v: f"{v >> 16}.{(v >> 8) & 255}.{v & 255}"
                print(f"{path}: platform {platform}, minimum {show(minos)}, SDK {show(old)} -> {show(sdk)}")
                return
            off += size
    sys.exit(f"{path}: no LC_BUILD_VERSION load command")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    stamp(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else "26.0")
