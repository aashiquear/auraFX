# Streamed reading: testdata/wavelet.vti read on this computer in four pieces, one after
# another, contoured; the Open on Server window shows the choice that does this.
import aurafx as afx
d = afx.load(ROOT + "/testdata/wavelet.vti", stream=4)
c = d.contour(array="RTData", values=[120, 180])
c.set(color_by="RTData", cmap="Viridis")
d.hide()
afx.view("iso"); afx.reset_camera()
afx.show_panel("remote 4 stream " + ROOT + "/testdata")
