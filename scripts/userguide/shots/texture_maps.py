# Texture mapping: a sphere and a cylinder with texture coordinates from Texture Map to Sphere
# and Texture Map to Cylinder, showing the made-up planet map (testdata/images/planet.jpg);
# Properties shows the globe's Texture setting.
import aurafx as afx
img = ROOT + "/testdata/images/planet.jpg"
cyl = afx.cylinder(center=(0, 0, 0), height=2.4, radius=0.45, resolution=96)
c = cyl.texture_map_to_cylinder(prevent_seam=False)
c.set(texture=img)
cyl.hide()
ball = afx.sphere(center=(0, 2.9, 0), radius=1.0, theta=128, phi=96)
g = ball.texture_map_to_sphere(prevent_seam=False)
g.set(texture=img)
ball.hide()
g.activate()
afx.view([-1.0, 0.35, 0.4], up=[0, 0, 1]); afx.reset_camera()
