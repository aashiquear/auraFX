# Align Sections: the TSL stack testdata/ebsd/ni_stack.h5ebsd (four slices cut from one map at
# shifted windows, from scripts/gen_ebsd_h5.py) colored by Orientation Colors, as read (left)
# and after Align Sections by misorientation (right). Both are stretched 6 times along z so the
# slices show; the black edges are cells uncovered by the moves.
import aurafx as afx
st = afx.load(ROOT + "/testdata/ebsd/ni_stack.h5ebsd")
st.hide()
b = st.orientation_colors()
b.hide()
before = b.transform(scale=(1, 1, 6))
al = st.align_sections(method="Misorientation")
a = al.orientation_colors()
a.hide()
after = a.transform(scale=(1, 1, 6), translate=(42, 0, 0))
st.hide(); b.hide(); al.hide(); a.hide()
before.show(); after.show(); after.activate()
before.set(color_by="IPFColor")
after.set(color_by="IPFColor")
afx.view([-0.3, -1.0, 0.55], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
