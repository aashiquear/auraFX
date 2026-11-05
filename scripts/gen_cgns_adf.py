"""Copy a CGNS file into ADF storage with the CGNS library bundled in ParaView (pvpython):
    pvpython scripts/gen_cgns_adf.py in.cgns out.cgns   (ADFTYPE=2 writes HDF5 instead)"""
import ctypes, sys, glob, os
lib = ctypes.CDLL(glob.glob(os.path.expanduser("~/Downloads/ParaView-6.1.1-MPI-Linux-Python3.12-x86_64/lib/libvtkcgns-pv6.1.so"))[0])
def die(what):
    lib.vtkcgns_cgio_error_message.argtypes=[ctypes.c_char_p]
    b=ctypes.create_string_buffer(256); lib.vtkcgns_cgio_error_message(b); sys.exit(f"{what}: {b.value}")
src, dst = sys.argv[1], sys.argv[2]
a=ctypes.c_int(); b=ctypes.c_int()
if lib.vtkcgns_cgio_open_file(src.encode(), ord('r'), 0, ctypes.byref(a)): die("open src")
if os.path.exists(dst): os.remove(dst)
if lib.vtkcgns_cgio_open_file(dst.encode(), ord("w"), int(os.environ.get("ADFTYPE","1")), ctypes.byref(b)): die("open dst")
if lib.vtkcgns_cgio_copy_file(a, b, 1): die("copy")
lib.vtkcgns_cgio_close_file(a); lib.vtkcgns_cgio_close_file(b)
print("ok", dst, os.path.getsize(dst))
