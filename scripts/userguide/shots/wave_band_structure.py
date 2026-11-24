# Wave Band Structure (no file needed): a plane array of stiff heavy discs (Young's modulus 30,
# density 8) in a soft light matrix (1 and 1), and its first eight bands along the path from
# the centre of the zone to the middle of an edge, a corner and back.
import aurafx as afx
cell = afx.hard_spheres(cells=[32, 32, 1], radius=10, fraction=0.3, method="Dense packing")
cell.set(color_by="Phases", cmap="Cool to Warm")
bands = cell.wave_band_structure(youngs=[1, 30], density=[1, 8], waves=4, bands=8, points=12)
afx.hide_panel("Charts")
bands.plot("Band 1", "Band 2", "Band 3", "Band 4", "Band 5", "Band 6", "Band 7", "Band 8", x="Path", style="lines", title="Wave bands of discs in a soft matrix")
cell.show()
cell.activate()
afx.view("+z"); afx.reset_camera()
