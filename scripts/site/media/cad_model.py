# CAD modeling: the flange template follows its dimensions as they change (the outside diameter,
# the bolt circle and the number of bolt holes), every face keeping its name; colored by face.
import math
import aurafx as afx
m = afx.cad_model(template="flange")
# The exact kernel: crisp edges on every frame.
m.set(color_by="FaceId", cmap="Turbo", kernel="B-rep")
afx.view([0.7, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()
n = 48


def cue(t):
    s = 0.5 - 0.5 * math.cos(2 * math.pi * t)
    od = 90 + 40 * s
    m.set(od=od, bolt_circle=round(od * 0.75, 1), holes=int(6 + round(4 * s)))


afx.save_animation(OUT, fps=14, frames=n, width=720, height=450, cue=cue)
