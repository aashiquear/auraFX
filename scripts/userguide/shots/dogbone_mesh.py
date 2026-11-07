# Dog-bone Mesh (no file needed): a flat tensile specimen 25.4 long with a 8-long, 1.85-wide
# gage, 3.175 fillets and a thickness profile through five stations along the gage (spline),
# two layers through the thickness, colored by block (gage, shoulder, grip).
import aurafx as afx
d = afx.dogbone_mesh(stations=[0, 0.3689, 2, 0.3508, 4, 0.3467, 6, 0.3519, 8, 0.3463], layers=2)
d.set(color_by="ElementBlock", cmap="Cool to Warm", representation="Surface With Edges", edge_color=(0.1, 0.1, 0.12))
afx.view([0.25, -1.0, 0.9], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
