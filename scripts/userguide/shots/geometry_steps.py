# The Geometry source's helpers: the Insert menus under the commands and the steps from
# geometry to a mesh, here after a mesh command (the hexahedra are shown, with their edges).
# The Pipeline panel is hidden to give the Properties their full height.
import aurafx as afx
part = afx.geometry(commands="""brick x 4 y 2 z 1
cylinder radius 0.5 height 2
subtract body 2 from body 1
mesh body 1 size 0.2
""", output="Hex mesh")
part.set(representation="Surface With Edges", color_by="BlockId")
part.activate()
afx.hide_panel("Pipeline")
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
