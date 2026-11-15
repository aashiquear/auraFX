# An assembly in the CAD environment: parts placed beside each other (Insert part on the Assemble
# tab), each kept with its own features and faces, moved and turned in the feature tree, and
# "Check fit" listing the parts that overlap.
import aurafx as afx
m = afx.cad_model(name="Housing")
m.update()
e = m.edit_cad()
e.component_add(name="Lid", template="bracket", at=(0, 0, 14))
e.component_add(name="Spare flange", template="flange", at=(46, 8, 0), rotate=(0, 0, 20))
e.evaluate()
e.show_tab("Assemble")
e.select_component("Spare flange")
e.check_fit()
afx.view([0.9, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
m.set(color_by="FaceId", cmap="Turbo")
afx.hide_panel("Properties"); afx.hide_panel("Information")
