//! Crystal mechanics of single orientations: slip systems of FCC, BCC and HCP crystals,
//! Schmid factors, full-constraint Taylor factors, directional Young's moduli from the elastic
//! constants, and a table of materials.
//!
//! Directions are Cartesian crystal directions (hexagonal: a1 along x, c along z, c/a given);
//! a loading direction `d` in the sample has crystal coordinates `g d` (crate conventions).

use crate::laue::Laue;
use crate::rotation::{rotate, Quat};

pub type V3 = [f64; 3];

fn unit(v: V3) -> V3 {
    let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    v.map(|x| x / n)
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lattice {
    Fcc,
    Bcc,
    Hcp,
}

/// A family of slip systems: its name and one representative (plane normal, slip direction),
/// written in Miller (cubic) or Miller-Bravais (hexagonal: plane h k i l, direction u v t w)
/// indices.
#[derive(Clone, Copy, Debug)]
pub struct Family {
    pub name: &'static str,
    pub lattice: Lattice,
    pub plane: [f64; 4],
    pub dir: [f64; 4],
}

pub const FAMILIES: [Family; 8] = [
    Family { name: "{111}<110>", lattice: Lattice::Fcc, plane: [1.0, 1.0, 1.0, 0.0], dir: [1.0, -1.0, 0.0, 0.0] },
    Family { name: "{110}<111>", lattice: Lattice::Bcc, plane: [1.0, 1.0, 0.0, 0.0], dir: [1.0, -1.0, 1.0, 0.0] },
    Family { name: "{112}<111>", lattice: Lattice::Bcc, plane: [1.0, 1.0, 2.0, 0.0], dir: [1.0, 1.0, -1.0, 0.0] },
    Family { name: "{123}<111>", lattice: Lattice::Bcc, plane: [1.0, 2.0, 3.0, 0.0], dir: [1.0, 1.0, -1.0, 0.0] },
    Family { name: "Basal <a>", lattice: Lattice::Hcp, plane: [0.0, 0.0, 0.0, 1.0], dir: [2.0, -1.0, -1.0, 0.0] },
    Family { name: "Prismatic <a>", lattice: Lattice::Hcp, plane: [1.0, 0.0, -1.0, 0.0], dir: [-1.0, 2.0, -1.0, 0.0] },
    Family { name: "Pyramidal <a>", lattice: Lattice::Hcp, plane: [1.0, 0.0, -1.0, 1.0], dir: [-1.0, 2.0, -1.0, 0.0] },
    Family { name: "Pyramidal <c+a>", lattice: Lattice::Hcp, plane: [1.0, 0.0, -1.0, 1.0], dir: [-1.0, -1.0, 2.0, 3.0] },
];

/// One slip system: unit slip direction `s`, unit plane normal `n`, the index of its family.
#[derive(Clone, Copy, Debug)]
pub struct SlipSystem {
    pub s: V3,
    pub n: V3,
    pub family: usize,
}

/// Cartesian plane normal and direction of a family's representative (`ca` = c/a).
fn cartesian(f: &Family, ca: f64) -> (V3, V3) {
    match f.lattice {
        Lattice::Hcp => {
            let [h, k, _i, l] = f.plane;
            let n = [h, (h + 2.0 * k) / 3f64.sqrt(), l / ca];
            let [u, v, t, w] = f.dir;
            // u a1 + v a2 + t a3 + w c with a1 = (1, 0, 0), a2 = (-1/2, sqrt3/2, 0), a3 = (-1/2, -sqrt3/2, 0).
            let s = [u - 0.5 * v - 0.5 * t, 3f64.sqrt() / 2.0 * (v - t), w * ca];
            (unit(n), unit(s))
        }
        _ => (unit([f.plane[0], f.plane[1], f.plane[2]]), unit([f.dir[0], f.dir[1], f.dir[2]])),
    }
}

/// The slip systems of the chosen families (indices into [`FAMILIES`]): every symmetry
/// equivalent of each representative, (n, s) and (-n, -s) counted once.
pub fn slip_systems(families: &[usize], ca: f64) -> Vec<SlipSystem> {
    let mut out: Vec<SlipSystem> = Vec::new();
    for &fi in families {
        let f = &FAMILIES[fi];
        let (n0, s0) = cartesian(f, ca);
        assert!(dot(n0, s0).abs() < 1e-9, "{}: direction not in the plane", f.name);
        let laue = if f.lattice == Lattice::Hcp { Laue::HexagonalHigh } else { Laue::CubicHigh };
        for op in laue.operators() {
            let (n, s) = (rotate(*op, n0), rotate(*op, s0));
            let same = |a: V3, b: V3| (0..3).all(|k| (a[k] - b[k]).abs() < 1e-9);
            let neg = |a: V3| a.map(|x| -x);
            let dup = out.iter().any(|o| o.family == fi && ((same(o.n, n) && same(o.s, s)) || (same(o.n, neg(n)) && same(o.s, neg(s))) || (same(o.n, n) && same(o.s, neg(s))) || (same(o.n, neg(n)) && same(o.s, s))));
            if !dup {
                out.push(SlipSystem { s, n, family: fi });
            }
        }
    }
    out
}

/// The largest Schmid factor |(d.s)(d.n)| for loading along sample direction `d`, and its
/// system.
pub fn schmid(q: Quat, d: V3, systems: &[SlipSystem]) -> (f64, usize) {
    let c = rotate(q, unit(d));
    let mut best = (0.0, 0);
    for (k, sys) in systems.iter().enumerate() {
        let m = (dot(c, sys.s) * dot(c, sys.n)).abs();
        if m > best.0 {
            best = (m, k);
        }
    }
    best
}

fn vec5(t: &[[f64; 3]; 3]) -> [f64; 5] {
    [t[0][0], t[1][1], t[0][1], t[0][2], t[1][2]]
}

fn inv5(a: [[f64; 5]; 5]) -> Option<[[f64; 5]; 5]> {
    let mut m = a;
    let mut inv = [[0.0; 5]; 5];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for c in 0..5 {
        let p = (c..5).max_by(|x, y| m[*x][c].abs().total_cmp(&m[*y][c].abs()))?;
        if m[p][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, p);
        inv.swap(c, p);
        let d = m[c][c];
        for k in 0..5 {
            m[c][k] /= d;
            inv[c][k] /= d;
        }
        for r in 0..5 {
            if r != c {
                let f = m[r][c];
                if f != 0.0 {
                    for k in 0..5 {
                        m[r][k] -= f * m[c][k];
                        inv[r][k] -= f * inv[c][k];
                    }
                }
            }
        }
    }
    Some(inv)
}

/// Precomputed bases for full-constraint Taylor factors: every set of five systems whose
/// symmetric Schmid tensors are independent, with the inverse of their 5 x 5 matrix.
pub struct TaylorBases {
    combos: Vec<[usize; 5]>,
    inv: Vec<[[f64; 5]; 5]>,
    weight: Vec<f64>,
}

impl TaylorBases {
    /// `weight[k]`: the relative critical resolved shear stress of system k (1 for all: the
    /// classical Taylor factor).
    pub fn new(systems: &[SlipSystem], weight: &[f64]) -> Result<TaylorBases, String> {
        let n = systems.len();
        if n > 30 {
            return Err(format!("{n} slip systems: at most 30 for the Taylor factor"));
        }
        let p: Vec<[f64; 5]> = systems
            .iter()
            .map(|s| {
                let t: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| 0.5 * (s.s[i] * s.n[j] + s.s[j] * s.n[i])));
                vec5(&t)
            })
            .collect();
        let mut combos = Vec::new();
        let mut inv = Vec::new();
        let mut c = [0usize, 1, 2, 3, 4];
        if n < 5 {
            return Err("fewer than five slip systems: no full-constraint Taylor factor".into());
        }
        loop {
            // Columns = systems.
            let b: [[f64; 5]; 5] = std::array::from_fn(|r| std::array::from_fn(|k| p[c[k]][r]));
            let det_ok = inv5(b);
            if let Some(m) = det_ok {
                // Determinant check as the toolkit (|det| > 1e-8): through the pivots' product.
                if det5(&b).abs() > 1e-8 {
                    combos.push(c);
                    inv.push(m);
                }
            }
            // Next combination.
            let mut i = 4;
            loop {
                if c[i] < n - 5 + i {
                    c[i] += 1;
                    for j in i + 1..5 {
                        c[j] = c[j - 1] + 1;
                    }
                    break;
                }
                if i == 0 {
                    return if combos.is_empty() { Err("the slip systems give no five independent ones".into()) } else { Ok(TaylorBases { combos, inv, weight: weight.to_vec() }) };
                }
                i -= 1;
            }
        }
    }

    /// Full-constraint Taylor factor M = min sum w|gamma| / eps_vM for uniaxial straining
    /// along sample direction `d`.
    pub fn taylor(&self, q: Quat, d: V3) -> f64 {
        let c = rotate(q, unit(d));
        let dm: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| 1.5 * c[i] * c[j] - if i == j { 0.5 } else { 0.0 }));
        let b = vec5(&dm);
        let mut best = f64::INFINITY;
        for (combo, m) in self.combos.iter().zip(&self.inv) {
            let mut s = 0.0;
            for (r, row) in m.iter().enumerate() {
                let g: f64 = (0..5).map(|k| row[k] * b[k]).sum();
                s += self.weight[combo[r]] * g.abs();
                if s >= best {
                    break;
                }
            }
            best = best.min(s);
        }
        best
    }
}

fn det5(a: &[[f64; 5]; 5]) -> f64 {
    let mut m = *a;
    let mut det = 1.0;
    for c in 0..5 {
        let p = (c..5).max_by(|x, y| m[*x][c].abs().total_cmp(&m[*y][c].abs())).unwrap();
        if m[p][c] == 0.0 {
            return 0.0;
        }
        if p != c {
            m.swap(c, p);
            det = -det;
        }
        det *= m[c][c];
        for r in c + 1..5 {
            let f = m[r][c] / m[c][c];
            for k in c..5 {
                m[r][k] -= f * m[c][k];
            }
        }
    }
    det
}

/// The 6 x 6 stiffness (Voigt) of a crystal: cubic from C11, C12, C44; hexagonal from C11,
/// C12, C13, C33, C44 (C66 = (C11 - C12) / 2).
pub fn stiffness(lattice: Lattice, c: &[f64]) -> [[f64; 6]; 6] {
    let mut m = [[0.0; 6]; 6];
    match lattice {
        Lattice::Hcp => {
            let (c11, c12, c13, c33, c44) = (c[0], c[1], c[2], c[3], c[4]);
            m[0][0] = c11;
            m[1][1] = c11;
            m[2][2] = c33;
            m[0][1] = c12;
            m[1][0] = c12;
            m[0][2] = c13;
            m[2][0] = c13;
            m[1][2] = c13;
            m[2][1] = c13;
            m[3][3] = c44;
            m[4][4] = c44;
            m[5][5] = 0.5 * (c11 - c12);
        }
        _ => {
            let (c11, c12, c44) = (c[0], c[1], c[2]);
            for i in 0..3 {
                for j in 0..3 {
                    m[i][j] = if i == j { c11 } else { c12 };
                }
                m[i + 3][i + 3] = c44;
            }
        }
    }
    m
}

/// The compliance (inverse of a Voigt stiffness).
pub fn compliance(c: &[[f64; 6]; 6]) -> Option<[[f64; 6]; 6]> {
    let mut m = *c;
    let mut inv = [[0.0; 6]; 6];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..6 {
        let p = (col..6).max_by(|x, y| m[*x][col].abs().total_cmp(&m[*y][col].abs()))?;
        if m[p][col].abs() < 1e-300 {
            return None;
        }
        m.swap(col, p);
        inv.swap(col, p);
        let d = m[col][col];
        for k in 0..6 {
            m[col][k] /= d;
            inv[col][k] /= d;
        }
        for r in 0..6 {
            if r != col {
                let f = m[r][col];
                for k in 0..6 {
                    m[r][k] -= f * m[col][k];
                    inv[r][k] -= f * inv[col][k];
                }
            }
        }
    }
    Some(inv)
}

/// Young's modulus along crystal direction `c` from a Voigt compliance: 1 / (a^T S a) with
/// a = (c1^2, c2^2, c3^2, c2 c3, c1 c3, c1 c2) (the Voigt compliance's shear entries carry the
/// factors of the engineering shear strains).
pub fn youngs_modulus(s: &[[f64; 6]; 6], c: V3) -> f64 {
    let c = unit(c);
    let a = [c[0] * c[0], c[1] * c[1], c[2] * c[2], c[1] * c[2], c[0] * c[2], c[0] * c[1]];
    let mut v = 0.0;
    for i in 0..6 {
        for j in 0..6 {
            v += a[i] * s[i][j] * a[j];
        }
    }
    1.0 / v
}

/// A material: lattice, elastic constants (GPa), shear modulus (GPa), Burgers vector (m), c/a,
/// default slip families with relative critical resolved shear stresses, and the toolkit's
/// strength parameters where known.
#[derive(Clone, Debug)]
pub struct Material {
    pub name: &'static str,
    pub lattice: Lattice,
    pub elastic: &'static [f64],
    pub shear_modulus_gpa: f64,
    pub burgers_m: f64,
    pub c_over_a: f64,
    pub families: &'static [(usize, f64)],
    /// Base yield strength (MPa) of a random texture without dislocations; 0 when not known.
    pub sigma_base_mpa: f64,
    pub taylor_alpha: f64,
    pub source: &'static str,
}

pub const MATERIALS: [Material; 8] = [
    Material { name: "IN718", lattice: Lattice::Fcc, elastic: &[250.0, 160.0, 120.0], shear_modulus_gpa: 77.2, burgers_m: 2.54e-10, c_over_a: 1.0, families: &[(0, 1.0)], sigma_base_mpa: 647.6, taylor_alpha: 0.3, source: "AM orientation toolkit, materials/in718.json (NIST AM Bench 2022 IN718)" },
    Material { name: "CoNi GammaPrint-700", lattice: Lattice::Fcc, elastic: &[250.0, 160.0, 120.0], shear_modulus_gpa: 80.0, burgers_m: 2.54e-10, c_over_a: 1.0, families: &[(0, 1.0)], sigma_base_mpa: 400.0, taylor_alpha: 0.3, source: "AM orientation toolkit, materials/coni.json" },
    Material { name: "Nickel", lattice: Lattice::Fcc, elastic: &[246.5, 147.3, 124.7], shear_modulus_gpa: 76.0, burgers_m: 2.49e-10, c_over_a: 1.0, families: &[(0, 1.0)], sigma_base_mpa: 0.0, taylor_alpha: 0.3, source: "elastic constants: Simmons and Wang (1971)" },
    Material { name: "Copper", lattice: Lattice::Fcc, elastic: &[168.4, 121.4, 75.4], shear_modulus_gpa: 48.0, burgers_m: 2.56e-10, c_over_a: 1.0, families: &[(0, 1.0)], sigma_base_mpa: 0.0, taylor_alpha: 0.3, source: "elastic constants: Simmons and Wang (1971)" },
    Material { name: "Aluminium", lattice: Lattice::Fcc, elastic: &[108.2, 61.3, 28.5], shear_modulus_gpa: 26.0, burgers_m: 2.86e-10, c_over_a: 1.0, families: &[(0, 1.0)], sigma_base_mpa: 0.0, taylor_alpha: 0.3, source: "elastic constants: Simmons and Wang (1971)" },
    Material { name: "Iron (BCC)", lattice: Lattice::Bcc, elastic: &[231.4, 134.7, 116.4], shear_modulus_gpa: 82.0, burgers_m: 2.48e-10, c_over_a: 1.0, families: &[(1, 1.0), (2, 1.0)], sigma_base_mpa: 0.0, taylor_alpha: 0.3, source: "elastic constants: Simmons and Wang (1971)" },
    Material { name: "Titanium (HCP)", lattice: Lattice::Hcp, elastic: &[162.4, 92.0, 69.0, 180.7, 46.7], shear_modulus_gpa: 44.0, burgers_m: 2.95e-10, c_over_a: 1.587, families: &[(4, 1.0), (5, 0.7), (6, 1.0), (7, 3.0)], sigma_base_mpa: 0.0, taylor_alpha: 0.3, source: "elastic constants: Simmons and Wang (1971); relative resolved shear stresses typical of alpha titanium" },
    Material { name: "Magnesium (HCP)", lattice: Lattice::Hcp, elastic: &[59.4, 25.6, 21.4, 61.6, 16.4], shear_modulus_gpa: 17.0, burgers_m: 3.21e-10, c_over_a: 1.624, families: &[(4, 1.0), (5, 10.0), (6, 10.0), (7, 15.0)], sigma_base_mpa: 0.0, taylor_alpha: 0.3, source: "elastic constants: Simmons and Wang (1971); relative resolved shear stresses typical of magnesium" },
];

/// The Taylor factor of a random FCC polycrystal (Kocks), the toolkit's reference.
pub const M_RANDOM_FCC: f64 = 3.06;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn systems_and_moduli() {
        assert_eq!(slip_systems(&[0], 1.0).len(), 12);
        assert_eq!(slip_systems(&[1], 1.0).len(), 12);
        assert_eq!(slip_systems(&[2], 1.0).len(), 12);
        assert_eq!(slip_systems(&[3], 1.0).len(), 24);
        assert_eq!(slip_systems(&[4], 1.587).len(), 3);
        assert_eq!(slip_systems(&[5], 1.587).len(), 3);
        assert_eq!(slip_systems(&[6], 1.587).len(), 6);
        assert_eq!(slip_systems(&[7], 1.587).len(), 12);
        // Cubic modulus along <100> and <111> (Voigt route vs the closed forms).
        let (c11, c12, c44) = (250.0, 160.0, 120.0);
        let s = compliance(&stiffness(Lattice::Fcc, &[c11, c12, c44])).unwrap();
        let det = (c11 - c12) * (c11 + 2.0 * c12);
        let (s11, s12, s44) = ((c11 + c12) / det, -c12 / det, 1.0 / c44);
        let e111 = 1.0 / (s11 - 2.0 * (s11 - s12 - 0.5 * s44) / 3.0);
        assert!((youngs_modulus(&s, [1.0, 0.0, 0.0]) - 1.0 / s11).abs() < 1e-9);
        assert!((youngs_modulus(&s, [1.0, 1.0, 1.0]) - e111).abs() < 1e-9);
        // Hexagonal: along c it is 1/s33.
        let h = compliance(&stiffness(Lattice::Hcp, &[162.4, 92.0, 69.0, 180.7, 46.7])).unwrap();
        assert!((youngs_modulus(&h, [0.0, 0.0, 1.0]) - 1.0 / h[2][2]).abs() < 1e-9);
        // Transversely isotropic: the same in every basal direction.
        assert!((youngs_modulus(&h, [1.0, 0.0, 0.0]) - youngs_modulus(&h, [1.0, 1.0, 0.0])).abs() < 1e-9);
    }
}
