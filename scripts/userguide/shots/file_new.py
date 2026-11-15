# File > New with work that is not saved: the question before the workspace is cleared.
import aurafx as afx
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
c = d.contour(array="Temp", values=[400.0, 600.0]); c.set(color_by="Temp")
afx.view("iso"); afx.reset_camera()
afx.new(ask=True)
