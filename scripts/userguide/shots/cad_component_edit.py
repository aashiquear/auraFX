# A component of an assembly edited on its own: the breadcrumb Assembly > Cap at the top of
# the model tree, the cap's own features in the tree, and a hole and a boss added to it with
# the same tools as in a part of its own. The rest of the assembly is hidden meanwhile.
import aurafx as afx
m = afx.cad_model(name="Assembly")
m.update()
e = m.edit_cad()
e.clear("Box with a cap")
e.component_add(name="Base", at=(0, 0, 0))
e.component_add(name="Cap", at=(0, 0, 24))
e.component_op("Base", "add", kind="box", name="Block", size=[50, 40, 20], at=[0, 0, 10])
e.component_op("Cap", "add", kind="box", name="Slab", size=[50, 40, 6], at=[0, 0, 3])
e.edit_component("Cap")
e.hole(diameter=10, depth=20, kind="counterbore", cb_diameter=18, cb_depth=2, at=(0, 0, 6), name="Bore")
e.extrude(e.sketch(face="Slab z+").circle(r=4, center=(16, 12)), depth=5, name="Boss")
e.evaluate()
e.show_tab("Features")
e.select("Bore")
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")
afx.hide_panel("Information")
