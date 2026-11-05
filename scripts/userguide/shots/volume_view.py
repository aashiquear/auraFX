import aurafx as afx
w = afx.load(ROOT + "/testdata/wavelet.vti")
w.set(representation="Volume", color_by="RTData", cmap="Cool to Warm", volume_threshold=0.3)
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT, width=800)
