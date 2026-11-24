# Local Thickness (no file needed). Left: the struts of an open-cell foam colored by their
# local thickness. Right: the pores of the same foam on a cut through its middle, colored by
# the pore size.
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
foam = afx.open_cell_foam(foam_cells=40, fraction=0.15)
struts = foam.local_thickness(phase=2)
place(struts, by="LocalThickness", cmap="Viridis")
pores = foam.local_thickness(phase=1)
pores.hide()
cut = pores.clip(origin=(32, 32, 32), normal=(0, 1, 0)); cut.hide()
place(cut, dx=70, lower=1, upper=1, by="LocalThickness", cmap="Plasma")
finish()
