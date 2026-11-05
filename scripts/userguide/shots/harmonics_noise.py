# Procedural data: the Spatio Temporal Harmonics source (an image volume of travelling waves
# with time steps of its own) at its sixth step, and Perlin noise on a plane, warped by it.
import aurafx as afx
h = afx.spatio_temporal_harmonics_grid()
h.set(color_by="SpatioTemporalHarmonics", cmap="Cool to Warm")
afx.time(5)
p = afx.plane(origin=(14, -10, -10), point1=(34, -10, -10), point2=(14, 10, -10), xres=200, yres=200)
n = p.perlin_noise(frequency=(0.25, 0.25, 0.25), amplitude=4)
w = n.warp_by_scalar(array="PerlinNoise", scale=1.0)
w.set(color_by="PerlinNoise", cmap="Viridis (matplotlib)")
p.hide(); n.hide()
afx.view([0.55, -1.0, 0.75], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT, width=900)
