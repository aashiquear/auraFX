# Mates and motion: a lid hinged on a box. The box is fixed, the two hinge knuckles are
# concentric, the fronts are flush, and an angle mate between the box's top and the lid's
# underside opens the lid by 40 degrees. The Mates section of the model tree lists them, with
# a slider on the angle.
import aurafx as afx
m = afx.cad_model(name="Hinged box")
m.update()
e = m.edit_cad()
e.clear("Hinged box")
e.component_add(name="Box")
e.component_add(name="Lid", at=(0, 0, 40))
e.component_op("Box", "add", kind="box", name="Body", size=[60, 40, 20], at=[0, 0, 10])
e.component_op("Box", "add", kind="cylinder", name="Knuckle", radius=3, height=12, at=[-30, 6, 20], rotate=[90, 0, 0])
e.component_op("Lid", "add", kind="box", name="Plate", size=[60, 40, 3], at=[30, 0, 1.5])
e.component_op("Lid", "add", kind="cylinder", name="Knuckle", radius=3, height=12, at=[0, -6, 0], rotate=[90, 0, 0])
e.mate("fixed", "Box")
e.mate("concentric", "Box/Knuckle side", "Lid/Knuckle side", name="Hinge")
e.mate("coincident", "Box/Body y-", "Lid/Plate y-", flip=True, name="Fronts flush")
e.mate("angle", "Box/Body z+", "Lid/Plate z-", value=40, name="Opening")
e.evaluate()
e.show_tab("Assemble")
afx.view([0.6, -1.0, 0.5], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")
afx.hide_panel("Properties"); afx.hide_panel("Information")
