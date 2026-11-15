# An EBSD scan on a hexagonal grid (testdata/ebsd/hex_grains.ang): read on a square grid of the scan's
# x step, each square cell taking the nearest scan point; colored by Orientation Colors (IPF along z).
import aurafx as afx
m = afx.load(ROOT + "/testdata/ebsd/hex_grains.ang")
m.hide()
ipf = m.orientation_colors()
ipf.set(color_by="IPFColor", representation="Surface With Edges", edge_color=(0.3, 0.3, 0.3))
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)
