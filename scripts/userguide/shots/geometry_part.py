# Geometry from commands (no file needed): a plate with two holes, a revolved and filleted
# boss united with it, a pipe swept along a bent path, and a separate torus, colored by face.
import aurafx as afx
g = afx.mesh.geometry()
g.cmd("brick x 6 y 3 z 0.6")
g.cmd("cylinder radius 0.45 height 2; move body 2 x -2.1; subtract body 2 from body 1")
g.cmd("cylinder radius 0.45 height 2; move body 3 x 2.1; subtract body 3 from body 1")
g.cmd("outline 1 polyline 0 0 1.1 0 1.1 0.5 0.55 0.5 0.55 2.2 0 2.2; outline 1 fillet radius 0.2")
g.cmd("revolve outline 1; move body 4 z 0.05; unite body 1 4")
g.cmd("outline 2 circle radius 0.18; sweep outline 2 path -2.6 1.1 0.3 -1 1.1 0.3 -1 1.1 1.8")
g.cmd("torus major 0.8 minor 0.14; move body 6 x 2.1 z 1.2")
part = g.result(resolution=160)
part.set(color_by="FaceId", cmap="Turbo")
afx.view([0.7, -1.0, 0.75], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
