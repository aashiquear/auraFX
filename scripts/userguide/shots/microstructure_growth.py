# Synthetic Microstructure, automata and growth (no file needed). Top row: a cellular automaton
# after 40 steps of annealing, an activator-inhibitor pattern, a reaction-diffusion labyrinth.
# Bottom row: diffusion-limited clusters grown from 20 seeds (each cluster in its own color), a
# pipe network, a branching tree colored by the radius of its branches.
import aurafx as afx
shown = []
def place(src, dx=0.0, dz=0.0, color=(0.82, 0.52, 0.28), by=None, lower=2, cmap=None):
    """The solid of a microstructure (its cells of phase `lower` and above), moved aside."""
    src.hide()
    t = src.threshold(array="Phases", lower=lower)
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
def finish(direction=(0.6, -1.0, 0.7)):
    for n in afx.nodes():
        n.hide()
    for m in shown:
        m.show()
    shown[-1].activate()
    afx.view(list(direction), up=[0, 0, 1]); afx.reset_camera()
    afx.screenshot(OUT)
place(afx.cellular_automaton_structure(steps=40), dz=80, color=(0.80, 0.50, 0.26))
place(afx.activator_inhibitor_pattern(fraction=0.35), dx=80, dz=80, color=(0.36, 0.56, 0.78))
place(afx.reaction_diffusion_pattern(pattern="Labyrinth", fraction=0.4), dx=160, dz=80, color=(0.45, 0.66, 0.42))
place(afx.aggregation_cluster(fraction=0.08), by="FeatureIds", cmap="Turbo")
place(afx.pipe_network(), dx=80, color=(0.62, 0.48, 0.72))
place(afx.branching_network(attractors=4000), dx=160, by="BranchRadius", cmap="Viridis")
finish()
