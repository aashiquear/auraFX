import aurafx as afx
# Half of a box mesh turned into tetrahedra, with pyramids where it meets the hexahedra; each
# element shrunk so the kinds show (colored by ElementType: hexahedra, pyramids, tetrahedra).
m = afx.box_mesh(size=(1, 1, 0.5), cells=(4, 4, 2))
mixed = m.hex_to_tets(expression="coordsX < 0.5")
s = mixed.shrink(factor=0.8)
s.set(color_by="ElementType", cmap="Cool to Warm")
mixed.hide()
afx.view("iso"); afx.reset_camera()
afx.screenshot(OUT)
