# Units: the synthetic sea-surface temperature (testdata/netcdf/globe_sst.nc) whose sst variable
# says its units are K; the legend and the Information panel show them.
import aurafx as afx
d = afx.load(ROOT + "/testdata/netcdf/globe_sst.nc")
d.set(color_by="sst", cmap="Cool to Warm")
afx.view([1.0, 0.6, 0.5], up=[0, 0, 1]); afx.reset_camera()
afx.show_panel("Information")
