# Resample AMR: the AMR Gaussian Pulse source (three boxes on two levels, drawn as outlines)
# resampled onto a 40 x 40 x 40 grid of points and shown as a volume.
import aurafx as afx
p = afx.amr_gaussian_pulse()
p.set(representation="Outline", color_by=None, solid_color=(0.1, 0.1, 0.15))
r = p.resample_amr(samples=(40, 40, 40))
r.set(representation="Volume", color_by="Gaussian-Pulse", cmap="Inferno")
p.show(); r.activate(); r.info()
afx.view([1.0, -1.2, 0.8], up=[0, 0, 1]); afx.reset_camera()
