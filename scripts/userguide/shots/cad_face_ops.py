# Operations on faces, as three parts side by side: a bolt circle laid out on the top face of a plate
# (Pattern on a face), a block whose side faces are drafted by 6 degrees (Draft faces), and a
# half part mirrored about its flat face (Mirror about a face).
import aurafx as afx
m = afx.cad_model(name="Face operations")
m.cad.clear("Face operations")
m.cad.component_add(name="Bolt circle", at=(0, 0, 0))
m.cad.component_op("Bolt circle", "add", kind="box", name="Plate", size=[50, 50, 8], at=[0, 0, 4])
m.cad.component_op("Bolt circle", "face_pattern", face="Plate z+", layout="circular", count=6, radius=17, diameter=5, depth=8, name="Bolts")
m.cad.component_op("Bolt circle", "face_pattern", face="Plate z+", kind="boss", layout="grid", counts=[2, 2], spacing=[10, 10], offset=[-5, -5], diameter=6, height=5, name="Pins")
m.cad.component_add(name="Drafted block", at=(62, 0, 0))
m.cad.component_op("Drafted block", "add", kind="box", name="Block", size=[34, 34, 34], at=[0, 0, 17])
m.cad.component_op("Drafted block", "draft", faces=[[17, 0, 17], [-17, 0, 17], [0, 17, 17], [0, -17, 17]], angle=6, neutral_origin=[0, 0, 0], neutral_normal=[0, 0, 1], name="Draft")
m.cad.component_add(name="Mirrored half", at=(124, 0, 0))
m.cad.component_op("Mirrored half", "add", kind="box", name="Half", size=[22, 30, 12], at=[11, 0, 6])
m.cad.component_op("Mirrored half", "add", kind="cylinder", name="Peg", radius=6, height=16, at=[16, 0, 12])
m.cad.component_op("Mirrored half", "mirror_plane", origin=[0, 0, 0], normal=[1, 0, 0], name="Mirror")
m.set(color_by="FaceId", cmap="Turbo")
afx.view([0.7, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
