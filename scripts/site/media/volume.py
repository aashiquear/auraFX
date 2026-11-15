# Volume rendering: the wavelet volume (testdata/wavelet.vti) drawn as a volume; a turntable.
import aurafx as afx
w = afx.load(ROOT + "/testdata/wavelet.vti")
w.set(representation="Volume", color_by="RTData", cmap="Inferno", volume_threshold=0.45)
afx.view("iso"); afx.reset_camera()
afx.save_animation(OUT, poster=OUT[:-4] + '.jpg', fps=20, orbit=80)
