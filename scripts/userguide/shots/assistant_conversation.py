import aurafx as afx
# A saved conversation shown in the Assistant panel (copy buttons, new / history / delete).
d = afx.load(ROOT + "/testdata/disk_out_ref.ex2")
s = d.slice(normal=[0, 0, 1]); s.set(color_by="Temp")
afx.view("iso"); afx.reset_camera()
afx.conversation(ROOT + "/testdata/conversation_example.json")
