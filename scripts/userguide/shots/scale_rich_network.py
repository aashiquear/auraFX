# Synthetic Microstructure, Scale-Rich Network with its standard settings (no file needed): 500
# ligaments in the plane, each thinner than the one before, at a volume fraction of 0.4.
import aurafx as afx
n = afx.scale_rich_network()
n.hide()
t = n.threshold(array="Phases", lower=2)
t.set(color_by=None, solid_color=(0.20, 0.33, 0.52))
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT)
