# The assistant panel with the camera box ticked (a picture of the 3D view goes with the message);
# the Wavelet source shown as a contour, no file needed. Nothing is sent: a whole-window shot.
import aurafx as afx
w = afx.wavelet()
w.hide()
c = w.contour(values=[160])
c.set(color_by="RTData", cmap="Turbo")
afx.view([0.7, -1.0, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.show_panel("Assistant")
afx._cad_app_command("assistant_view_on")
