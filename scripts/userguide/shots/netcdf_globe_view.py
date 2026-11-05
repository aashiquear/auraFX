import aurafx as afx, os
p = os.path.expanduser("~/Downloads/ParaView-6.1.1-MPI-Linux-Python3.12-x86_64/share/paraview-6.1/examples/netCDF/tos_O1_2001-2002.nc")
if os.path.exists(p):
    d = afx.load(p); d.set(color_by="tos", cmap="Cool to Warm")
    afx.view("+x"); afx.reset_camera()
    afx.screenshot(OUT, width=800)
