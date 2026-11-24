# Microstructure Formation (no file needed): grains that nucleate at a constant rate and grow
# in the plane, at three of the time steps (times 5, 9 and 15), colored by grain; the cells not
# yet transformed are left out.
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
for k, time in enumerate([5, 9, 15]):
    g = afx.nucleation_growth_grains(cells=[160, 160, 1], rate=0.0006, time=time, seed=5)
    place(g, dx=132 * k, lower=1, upper=1, by="FeatureIds", cmap="Turbo")
finish(direction=(0, 0, 1), up=(0, 1, 0))
