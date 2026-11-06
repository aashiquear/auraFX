# EBSD orientation map: the synthetic nickel scan (testdata/ebsd/ni_map.ang, from
# scripts/gen_ebsd.py) colored by Orientation Colors (inverse pole figure key along the map
# normal z), with the cubic IPF color key beside it, enlarged and moved next to the map.
import aurafx as afx
m = afx.load(ROOT + "/testdata/ebsd/ni_map.ang")
m.hide()
ipf = m.orientation_colors()
ipf.set(color_by="IPFColor")
ipf.show(); ipf.activate()
key = afx.ipf_color_key(structure="Cubic m-3m", resolution=80)
key.hide()
k = key.transform(scale=(22, 22, 1), translate=(42, 12, 0))
k.set(color_by="IPFColor")
k.show()
afx.view([0, 0, 1], up=[0, 1, 0]); afx.reset_camera()
afx.screenshot(OUT)
