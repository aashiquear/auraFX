# What fits this data: the command palette opened on a volume with values, nothing typed yet.
# The filters suggested for it come first, each with its reason.
import aurafx as afx
w = afx.wavelet()
w.set(color_by="RTData")
afx.view("iso"); afx.reset_camera()
afx.show_panel("palette")
