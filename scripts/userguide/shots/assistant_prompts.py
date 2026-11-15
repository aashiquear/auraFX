import aurafx as afx
afx.set_theme("light")
import aurafx as afx
# A saved prompt put in the assistant's box for the selected object.
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
d.set(color_by="Temp", cmap="Cool to Warm")
afx.view("iso"); afx.reset_camera()
d.activate()
afx.assistant_prompt("Find problems")
