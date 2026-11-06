# The Geometry source in the window: its commands in Properties, the Bodies tree with how each
# body was made and its faces, and the result drawn with its face colors.
import aurafx as afx
part = afx.geometry(commands="""# A block with a hole and a rounded boss.
brick x 4 y 2 z 1
cylinder radius 0.5 height 2
subtract body 2 from body 1
outline 1 rectangle x 1 y 1
outline 1 fillet radius 0.3
extrude outline 1 height 1
move body 3 x 1.4 z 0.5
unite body 1 3
face 6 name 'top'
""", resolution=96)
part.set(color_by="FaceId", cmap="Turbo")
part.activate()
afx.view([0.8, -1.0, 0.8], up=[0, 0, 1]); afx.reset_camera()
