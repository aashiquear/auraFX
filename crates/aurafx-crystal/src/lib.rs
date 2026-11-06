//! Orientation mathematics for crystals.
//!
//! Conventions (the ones EBSD files and `.dream3d` files use):
//! - An orientation is a passive rotation from the **sample** frame to the **crystal** frame:
//!   a direction `s` in sample coordinates has crystal coordinates `g s`.
//! - Euler angles are Bunge (z, x', z''), in radians: `g = Rz(φ2) Rx(Φ) Rz(φ1)` with the passive
//!   rotation matrices `Rz(a) = [[c, s, 0], [-s, c, 0], [0, 0, 1]]`.
//! - A quaternion `q = (w, x, y, z)` is the active rotation whose matrix equals `g`
//!   (`R(q) v = q v q*`), with `w >= 0`. Arrays store quaternions as (x, y, z, w).
//! - A crystal symmetry operator `S` acts on the crystal frame: `S g` describes the same
//!   orientation. The misorientation from orientation 1 to 2 is `g2 g1ᵀ`.
//! - Hexagonal and trigonal crystals have c along z and a1 = [2-1-10] along x.

pub mod ipf;
pub mod laue;
pub mod rotation;

pub use ipf::{euler_color, ipf_color, ipf_sector, IpfSector};
pub use laue::Laue;
pub use rotation::*;

/// The smallest rotation angle (radians, 0..π) taking orientation `q1` to `q2` among the
/// symmetry-equivalent descriptions of crystal symmetry `laue` (the disorientation angle).
pub fn disorientation_angle(q1: Quat, q2: Quat, laue: Laue) -> f64 {
    let d = mul(q2, conj(q1));
    let mut best: f64 = 0.0;
    for s in laue.operators() {
        let w = (s[0] * d[0] - s[1] * d[1] - s[2] * d[2] - s[3] * d[3]).abs();
        best = best.max(w);
    }
    // 2 acos(w), through atan2 for accuracy near 0.
    let w = best.min(1.0);
    2.0 * (1.0 - w * w).max(0.0).sqrt().atan2(w)
}

/// The disorientation of `q2` from `q1`: the angle (radians) and the rotation axis (crystal
/// coordinates of crystal 1, unit vector) of the shortest symmetry-equivalent misorientation.
/// For cubic crystals the axis is brought into the standard triangle (x >= y >= z >= 0) using
/// the switching symmetry; for other groups it is the axis of the shortest equivalent.
pub fn disorientation(q1: Quat, q2: Quat, laue: Laue) -> (f64, [f64; 3]) {
    let d = mul(q2, conj(q1));
    let ops = laue.operators();
    let mut best = (f64::NEG_INFINITY, [1.0, 0.0, 0.0, 0.0]);
    for a in ops {
        for b in ops {
            for m in [mul(mul(*a, d), conj(*b)), conj(mul(mul(*a, d), conj(*b)))] {
                let m = positive(m);
                let key = m[0] - 1e-12 * axis_rank(&m, laue);
                if key > best.0 + 1e-15 {
                    best = (key, m);
                }
            }
        }
    }
    let (angle, axis) = to_axis_angle(best.1);
    (angle, axis)
}

/// Prefers axes in the cubic standard triangle when angles tie (a tie-breaking rank, 0 best).
fn axis_rank(m: &Quat, laue: Laue) -> f64 {
    if !matches!(laue, Laue::CubicHigh | Laue::CubicLow) {
        return 0.0;
    }
    let (x, y, z) = (m[1], m[2], m[3]);
    if x >= y && y >= z && z >= 0.0 {
        0.0
    } else {
        1.0
    }
}

/// The symmetry-equivalent of `q` with the smallest rotation angle (its fundamental-zone
/// representative), with `w >= 0`.
pub fn reduce_to_fundamental_zone(q: Quat, laue: Laue) -> Quat {
    let mut best = (f64::NEG_INFINITY, q);
    for s in laue.operators() {
        let p = positive(mul(*s, q));
        if p[0] > best.0 + 1e-15 {
            best = (p[0], p);
        }
    }
    best.1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_orders() {
        let want = [
            (Laue::Triclinic, 1),
            (Laue::Monoclinic, 2),
            (Laue::Orthorhombic, 4),
            (Laue::TetragonalLow, 4),
            (Laue::TetragonalHigh, 8),
            (Laue::TrigonalLow, 3),
            (Laue::TrigonalHigh, 6),
            (Laue::HexagonalLow, 6),
            (Laue::HexagonalHigh, 12),
            (Laue::CubicLow, 12),
            (Laue::CubicHigh, 24),
        ];
        for (l, n) in want {
            assert_eq!(l.operators().len(), n, "{l:?}");
            // Closed under products.
            for a in l.operators() {
                for b in l.operators() {
                    let p = positive(mul(*a, *b));
                    assert!(l.operators().iter().any(|o| o.iter().zip(&p).all(|(x, y)| (x - y).abs() < 1e-12)), "{l:?} not closed");
                }
            }
        }
    }

    #[test]
    fn disorientation_is_symmetric_and_invariant() {
        let mut rng = 0x9e3779b97f4a7c15u64;
        let mut rnd = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            (rng >> 11) as f64 / (1u64 << 53) as f64
        };
        for l in Laue::ALL {
            for _ in 0..30 {
                let q1 = random_quat(rnd(), rnd(), rnd());
                let q2 = random_quat(rnd(), rnd(), rnd());
                let a = disorientation_angle(q1, q2, l);
                assert!((a - disorientation_angle(q2, q1, l)).abs() < 1e-9);
                for s in l.operators() {
                    assert!((a - disorientation_angle(mul(*s, q1), q2, l)).abs() < 1e-9);
                }
                let (b, _) = disorientation(q1, q2, l);
                assert!((a - b).abs() < 1e-9, "{l:?} {a} {b}");
            }
        }
        // Cubic: never more than 62.8 degrees; the twin relation is 60 degrees about <111>.
        let twin = axis_angle_to_quat([1.0, 1.0, 1.0], 60f64.to_radians());
        let (a, ax) = disorientation(IDENTITY, twin, Laue::CubicHigh);
        assert!((a.to_degrees() - 60.0).abs() < 1e-9);
        assert!(ax.iter().all(|v| (v - 1.0 / 3f64.sqrt()).abs() < 1e-9), "{ax:?}");
    }

    #[test]
    fn fundamental_zone_is_smallest() {
        let q = euler_to_quat([2.1, 1.2, 0.4]);
        for l in Laue::ALL {
            let r = reduce_to_fundamental_zone(q, l);
            for s in l.operators() {
                assert!(positive(mul(*s, q))[0] <= r[0] + 1e-12);
            }
        }
    }
}
