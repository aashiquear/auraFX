"""Tension of a polycrystal with the JAX-FEM finite element library.

Written by auraFX: @GRAINS@ grains, @ELEMENTS@ @ELE_TYPE@ elements, @MATERIAL@.

Files beside this script:
  nodes.txt           x y z of every node
  elements.txt        the node numbers (from 0) of every element
  element_grains.txt  the grain (from 1) of every element
  grain_axes.txt      row g - 1: the three cubic axes of grain g in the sample frame (9 numbers)

Every grain is linear elastic with the cubic stiffness of the material, turned to the grain's
orientation. The face at the low end of the loading axis is held along that axis, the faces at
the low ends of the other two axes are held along their own normals, and the face at the high
end is moved to the strain below in equal steps. Each step's displacement is written to
results/u_NNN.vtu, and results/u.pvd lists them as a time series.

This is a starting point: replace `stress` in `Polycrystal.get_tensor_map` (and keep state per
quadrature point in `internal_vars`) for another material law.

Run:  python run_jaxfem.py        (needs JAX-FEM: https://github.com/deepmodeling/jax-fem)
"""
import os

import numpy as onp
import jax.numpy as np

from jax_fem.generate_mesh import Mesh
from jax_fem.problem import Problem
from jax_fem.solver import solver
from jax_fem.utils import save_sol

HERE = os.path.dirname(os.path.abspath(__file__))
OUT = os.path.join(HERE, "results")

ELE_TYPE = "@ELE_TYPE@"
AXIS = @AXIS@          # the loading axis: 0, 1, 2 for x, y, z
STRAIN = @STRAIN@      # the strain at the end
STEPS = @STEPS@
# Cubic elastic constants of @MATERIAL@ in MPa (lengths in the mesh's units).
C11, C12, C44 = @C11@, @C12@, @C44@
ANISOTROPY = C11 - C12 - 2.0 * C44
# The linear solver of JAX-FEM: {} is its default (iterative, on the GPU when there is one);
# {"petsc_solver": {}} or {"spsolve_solver": {}} are alternatives.
LINEAR_SOLVER = {}


def read(name, dtype=float):
    return onp.loadtxt(os.path.join(HERE, name), dtype=dtype, ndmin=2)


class Polycrystal(Problem):
    def custom_init(self, element_axes):
        # The crystal axes at every quadrature point of every element.
        quads = self.fes[0].num_quads
        self.internal_vars = (np.repeat(np.asarray(element_axes)[:, None, :, :], quads, axis=1),)

    def get_tensor_map(self):
        def stress(u_grad, axes):
            strain = 0.5 * (u_grad + u_grad.T)
            sigma = C12 * np.trace(strain) * np.eye(3) + 2.0 * C44 * strain
            for a in range(3):
                r = axes[a]
                sigma = sigma + ANISOTROPY * (r @ strain @ r) * np.outer(r, r)
            return sigma

        return stress


def face(axis, value, tol):
    return lambda point: np.isclose(point[axis], value, atol=tol)


def constant(value):
    return lambda point: value


def main():
    points = read("nodes.txt")
    cells = read("elements.txt", dtype=int)
    grains = read("element_grains.txt", dtype=int).ravel()
    axes = read("grain_axes.txt").reshape(-1, 3, 3)
    element_axes = axes[grains - 1]

    mesh = Mesh(points, cells, ele_type=ELE_TYPE)
    low, high = points.min(axis=0), points.max(axis=0)
    length = high[AXIS] - low[AXIS]
    tol = 1e-6 * float((high - low).max())
    others = [a for a in range(3) if a != AXIS]
    # Held faces first, the moved face last.
    bcs = [
        [face(AXIS, low[AXIS], tol), face(others[0], low[others[0]], tol), face(others[1], low[others[1]], tol), face(AXIS, high[AXIS], tol)],
        [AXIS, others[0], others[1], AXIS],
        [constant(0.0), constant(0.0), constant(0.0), constant(0.0)],
    ]
    problem = Polycrystal(mesh, vec=3, dim=3, ele_type=ELE_TYPE, dirichlet_bc_info=bcs, additional_info=(element_axes,))

    os.makedirs(OUT, exist_ok=True)
    fe = problem.fes[0]
    sol_list = [np.zeros((fe.num_total_nodes, fe.vec))]
    files = []
    for step in range(STEPS + 1):
        if step > 0:
            move = STRAIN * length * step / STEPS
            bcs[2][3] = constant(move)
            fe.update_Dirichlet_boundary_conditions(bcs)
            options = dict(LINEAR_SOLVER)
            options["initial_guess"] = sol_list
            sol_list = solver(problem, solver_options=options)
            print(f"step {step} of {STEPS}: the {'xyz'[AXIS]}+ face moved by {move:g}")
        name = f"u_{step:03d}.vtu"
        save_sol(fe, sol_list[0], os.path.join(OUT, name), cell_infos=[("grain_id", grains)])
        files.append((step / STEPS, name))

    with open(os.path.join(OUT, "u.pvd"), "w") as f:
        f.write('<?xml version="1.0"?>\n<VTKFile type="Collection" version="0.1">\n  <Collection>\n')
        for time, name in files:
            f.write(f'    <DataSet timestep="{time:g}" part="0" file="{name}"/>\n')
        f.write("  </Collection>\n</VTKFile>\n")
    print(f"Wrote {len(files)} steps to {OUT}; open results/u.pvd")


if __name__ == "__main__":
    main()
