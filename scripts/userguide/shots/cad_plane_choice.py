# Choosing a plane: the face picker of "A face…" with the top of a plate chosen (its plane filled,
# strong, as large as the face) and the Front plane shown lightly, as it is while the pointer is
# on its button.
import aurafx as afx
m = afx.cad_model(name="Bracket block")
m.cad.hole(diameter=8, depth=20, name="Bore", at=(5, 3, 12))
m.update()
e = m.edit_cad()
e.evaluate()
e.show_tab("Sketch")
e.sketch_on_face()
e.pick_choose("Base top")
e.preview_plane("XZ")
afx.view([0.8, -1.0, 0.7], up=[0, 0, 1]); afx.reset_camera()
afx.hide_panel("Information")
