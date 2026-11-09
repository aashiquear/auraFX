import aurafx as afx
# Step 0 held with Force Time (right) beside the impact at a later step (left).
c = afx.load(ROOT + "/testdata/can.ex2")
c.set(color_by="EQPS")
f = c.force_time(step=0)
f.set(color_by="EQPS")
f.send_to_view("right", keep=False)
afx.active_view(0)
c.show()
afx.time(40)
afx.link_views(True)
afx.view("-y"); afx.reset_camera()
