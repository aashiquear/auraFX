# Smooth Surface (no file needed). Left pair: an octet lattice as cells, and the surface drawn
# from its struts three times finer than the cells. Right pair: hard spheres as cells, and
# their exact surface.
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
lattice = afx.strut_lattice(cells=[32, 32, 32], cell_size=32, fraction=0.15)
place(lattice, color=(0.70, 0.70, 0.74))
skin = lattice.microstructure_surface(refine=3)
a = skin.transform(translate=(1.3 * 36, 0, 0)); a.set(color_by=None, solid_color=(0.36, 0.56, 0.78)); shown.append(a)
spheres = afx.hard_spheres(cells=[32, 32, 32], radius=6, fraction=0.3, periodic=False)
place(spheres, dx=76, color=(0.70, 0.70, 0.74))
ball = spheres.microstructure_surface(refine=3)
b = ball.transform(translate=(1.3 * 112, 0, 0)); b.set(color_by=None, solid_color=(0.82, 0.52, 0.28)); shown.append(b)
finish()
