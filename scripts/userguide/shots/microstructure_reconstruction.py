# Stretch Microstructure and Reconstruct Microstructure (no file needed), left to right:
# overlapping spheres at a volume fraction of 0.25, the same stretched twice along Z, and a
# medium of 48 x 48 x 48 cells rebuilt from the statistics of the first.
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
s = afx.overlapping_spheres(radius=5, fraction=0.25, seed=4)
place(s, color=(0.80, 0.50, 0.26))
place(s.stretch_microstructure(stretch=[1, 1, 2]), dx=64, color=(0.36, 0.56, 0.78))
place(s.reconstruct_microstructure(cells=[48, 48, 48], sweeps=25), dx=128, color=(0.42, 0.66, 0.44))
finish()
