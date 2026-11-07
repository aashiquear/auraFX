# Polycrystal crystal plasticity written by auraFX: 30 grains, 860 elements, Aluminium.
# Orientations: orientations_points.csv (each element's centre, nearest point). The x- nodes
# are held along x, the x+ nodes pulled to a strain of 0.02 at t = 1.
# The slip and hardening parameters are a starting point: set them for the material.

[Global]
  backend    = numpy
  use_gpu    = false
  precision  = float64
  seed       = 0
  output_dir = solver
[]

[Problem]
  type          = composed
  dimension     = 3
  finite_strain = true
[]

[Mesh]
  type   = file
  path   = mesh.e
  format = exodus
  scale  = 1.0
  verify = true
[]

[Variables]
  [./u]
    family     = fem
    components = 3
  []
[]

[Functions]
  [./pull]
    kind       = parsed
    expression = 2*t
  []
[]

[Materials]
  [./cp]
    type                  = dislocation_density_cp
    C11                   = 108200
    C12                   = 61300
    C44                   = 28500
    mu                    = 26000
    gamma0                = 1e-05
    rate_exponent_n       = 8.33333333333
    tau_c0                = 5.29147
    q_lat                 = 1.4
    alpha0                = 0.4
    burgers               = 0.000286
    rho0                  = 47.9872
    k1                    = 0.198833
    annihilation_diameter = 0.00343758
    [./crystal]
      structure    = fcc
      euler_angles = '0 0 0'
    []
  []
[]

[Kernels]
  [./sd_cp]
    type          = stress_divergence_cp
    variable      = u
    material      = cp
    dim           = 3
    finite_strain = true
    gnd           = false
  []
[]

[OrientationField]
  [./source]
    type          = point_file
    path          = orientations_points.csv
    interpolation = nearest
  []
[]

[BCs]
  [./hold]
    variable   = u
    kind       = dirichlet
    node_set   = x-
    components = [0]
    values     = [0.0]
  []
  [./pull]
    variable   = u
    kind       = dirichlet
    node_set   = x+
    components = [0]
    values     = [1.0]
    function   = pull
  []
  [./hold_y]
    variable   = u
    kind       = dirichlet
    node_set   = y-
    components = [1]
    values     = [0.0]
  []
  [./hold_z]
    variable   = u
    kind       = dirichlet
    node_set   = z-
    components = [2]
    values     = [0.0]
  []
[]

[Executioner]
  type             = cp_quasi_static
  n_steps          = 40
  t_end            = 1
  newton_tol       = 0.001
  newton_rel_tol   = 0
  max_iter         = 50
  max_cutbacks     = 10
  linear_method    = direct
  snapshots_every  = 10
[]

[Output]
  formats         = 'vtk csv json'
  save_checkpoint = false
  fields          = 'displacement stress strain equivalent_plastic_strain slip_activity mean_rho'
[]
