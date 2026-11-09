# Tensors and derivatives (no file needed): a swirling flow on the Wavelet grid (built with the
# Calculator: a vortex along z), its Compute Derivatives (an isosurface of the Q criterion marks the core,
# colored by vorticity), and Tensor Glyph ellipsoids of its strain rate along a slice.
import aurafx as afx
w = afx.wavelet()
w.hide()
# A vortex along z whose rotation fades with the distance from its axis (and leans a little with z).
v = w.calculator(expression="vec(-(coordsY - 0.15*coordsZ)*exp(-((coordsX)^2+(coordsY - 0.15*coordsZ)^2)/30), coordsX*exp(-((coordsX)^2+(coordsY - 0.15*coordsZ)^2)/30), 0.2)", result="velocity")
v.hide()
d = v.compute_derivatives(array="velocity")
d.hide()
core = d.contour(array="Q_criterion", values=[0.3])
core.set(color_by="velocity_vorticity", cmap="Turbo", opacity=0.55)
s = d.slice(origin=(0, 0, 0), normal=(0, 0, 1))
s.hide()
g = s.tensor_glyph(array="velocity_strain_rate", scale=5, stride=23)
g.set(color_by="principal_value_1", cmap="Cool to Warm")
afx.view([0.6, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
