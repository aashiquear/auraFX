# Mesh Quality on the inclusion mesh: the summary table (each hexahedron metric's range, mean,
# acceptable range and cells outside it) in the Spreadsheet, with the mesh colored by scaled
# Jacobian in the view; harmonic grains this time.
import aurafx as afx
m = afx.inclusion_mesh(count=6, radius=0.08, cells=20, seed=3, shape="Harmonic grain")
q = m.mesh_quality()
q.set(color_by="ScaledJacobian", cmap="Viridis (matplotlib)")
t = m.mesh_quality(output="Summary table")
for n in afx.nodes():
    n.hide()
q.show()
t.activate()
afx.show_panel("Spreadsheet")
afx.view([1.0, -0.8, 0.6], up=[0, 0, 1]); afx.reset_camera()
