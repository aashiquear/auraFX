# 3D Glyphs: arrows along the gradient of a small wavelet, sized by the gradient's magnitude and
# colored by the value, drawn by the GPU from one arrow.
import aurafx as afx
w = afx.wavelet(half_size=6)
g = w.gradient(array="RTData")
g.set(representation="3D Glyphs", glyph_shape="arrow", glyph_orient="RTData_gradient",
      glyph_scale_by="RTData_gradient", color_by="RTData", cmap="Turbo")
w.hide()
afx.view([1.0, -0.8, 0.6], up=[0, 0, 1]); afx.reset_camera()
