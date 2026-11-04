"""Generate testdata/fcc_melt.dump: a 6912-atom FCC lattice heating up over 20 frames."""
import math, random, sys
random.seed(1)
a, n = 1.6, 12
base = [(i*a+dx*a, j*a+dy*a, k*a+dz*a) for i in range(n) for j in range(n) for k in range(n)
        for (dx, dy, dz) in [(0,0,0), (0.5,0.5,0), (0.5,0,0.5), (0,0.5,0.5)]]
L = n * a
out = sys.argv[1] if len(sys.argv) > 1 else "testdata/fcc_melt.dump"
with open(out, "w") as f:
    for t in range(20):
        amp = 0.02 + 0.03 * t
        f.write(f"ITEM: TIMESTEP\n{t*100}\nITEM: NUMBER OF ATOMS\n{len(base)}\nITEM: BOX BOUNDS pp pp pp\n0 {L}\n0 {L}\n0 {L}\nITEM: ATOMS id type x y z vx vy vz c_pe\n")
        for i, (x, y, z) in enumerate(base):
            vx, vy, vz = [random.gauss(0, amp*5) for _ in range(3)]
            dx, dy, dz = [random.gauss(0, amp) for _ in range(3)]
            r = math.dist((x, y, z), (L/2, L/2, L/2))
            pe = -6.0 + 0.002*r*r + (amp*10)*random.random()
            f.write(f"{i+1} {1 if i % 4 else 2} {x+dx:.4f} {y+dy:.4f} {z+dz:.4f} {vx:.4f} {vy:.4f} {vz:.4f} {pe:.4f}\n")
