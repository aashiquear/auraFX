# Synthetic Microstructure, Haversian Bone with its standard settings (no file needed), seen
# along the bone axis and colored by phase: interstitial bone (1), the bone of the osteons (2),
# cement lines (3) and canals (4).
import aurafx as afx
b = afx.haversian_bone()
b.set(color_by="Phases", cmap="Viridis")
afx.view("+z"); afx.reset_camera()
afx.screenshot(OUT)
