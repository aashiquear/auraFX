# Effective Stiffness and Effective Conductivity (no file needed). Left: stiff spheres (Young's
# modulus 20) in a matrix (1) stretched along X, cut through the middle and colored by the
# von Mises stress. Right: a gyroid sheet that conducts 100 times better than its pores, under
# a gradient along X, colored by the flux along X.
import aurafx as afx
shown = []
def place(src, dx=0.0, dz=0.0, color=(0.82, 0.52, 0.28), by=None, lower=2, upper=None, cmap=None, array="Phases"):
    """The cells of a microstructure whose `array` lies from `lower` (to `upper`), moved aside."""
    src.hide()
    t = src.threshold(array=array, lower=lower) if upper is None else src.threshold(array=array, lower=lower, upper=upper)
    t.hide()
    m = t.transform(translate=(1.3 * dx, 0, 1.2 * dz))
    if by:
        if cmap:
            m.set(color_by=by, cmap=cmap)
        else:
            m.set(color_by=by)
    else:
        m.set(color_by=None, solid_color=color)
    shown.append(m)
    return m
def finish(direction=(0.6, -1.0, 0.7), up=(0, 0, 1)):
    for n in afx.nodes():
        n.hide()
    for m in shown:
        m.show()
    shown[-1].activate()
    afx.view(list(direction), up=list(up)); afx.reset_camera()
    afx.screenshot(OUT)
s = afx.hard_spheres(cells=[48, 48, 48], radius=7, fraction=0.3)
e = s.effective_stiffness(youngs=[1, 20], poisson=[0.3, 0.2], load="Stretch along X")
e.hide()
cut = e.clip(origin=(24, 24, 24), normal=(0, 1, 0)); cut.hide()
v = place(cut, lower=1, by="VonMisesStress", cmap="Inferno")
v.color_by("VonMisesStress", range=(0.0, 4.0))
g = afx.triply_periodic_surface(cells=[48, 48, 48], cell_size=48, fraction=0.3)
k = g.effective_conductivity(conductivities=[0.01, 1.0], axis="X")
m = place(k, dx=56, by="Flux", cmap="Viridis")
m.color_by("Flux", component=0)
finish()
