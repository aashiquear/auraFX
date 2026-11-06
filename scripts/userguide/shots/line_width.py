# Line width: the wavelet's isosurfaces as a wireframe 2 pixels wide, colored by RTData,
# inside the bounding box drawn 6 pixels wide.
import aurafx as afx
w = afx.load(ROOT + "/testdata/wavelet.vti")
w.set(representation="Outline", color_by=None, solid_color=(0.15, 0.15, 0.2), line_width=6)
c = w.contour(array="RTData", values=[120.0, 200.0])
c.set(representation="Wireframe", color_by="RTData", cmap="Cool to Warm", line_width=2)
w.show(); c.show(); c.activate()
afx.view([-1.0, -0.8, 0.6], up=[0, 0, 1]); afx.reset_camera()
