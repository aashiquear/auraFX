# LAMMPS atoms from a polycrystal: a periodic Voronoi microstructure of six grains (no file needed)
# written as a LAMMPS case (aluminium, 8 Å per voxel); the atoms.dump of the case colored by grain.
import os, tempfile
import aurafx as afx
v = afx.voronoi_grains(cells=[10, 10, 10], grains=6, periodic=True)
v.hide()
folder = os.path.join(tempfile.gettempdir(), "afx_shot_lammps_polycrystal")
afx.simulation_case(v, "lammps", folder, material="Aluminium", angstrom_per_unit=8)
atoms = afx.load(os.path.join(folder, "atoms.dump"))
atoms.set(color_by="grain", cmap="Turbo", representation="Points", point_size=4)
afx.view([1.0, -0.8, 0.6], up=[0, 0, 1]); afx.reset_camera()
afx.screenshot(OUT)
