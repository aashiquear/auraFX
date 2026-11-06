//! The eleven Laue classes and their proper rotations.

use crate::rotation::{axis_angle_to_quat, mul, positive, Quat};
use std::sync::OnceLock;

/// A crystal's Laue class (its rotational point symmetry plus inversion).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Laue {
    /// -1
    Triclinic,
    /// 2/m (2-fold axis along y)
    Monoclinic,
    /// mmm
    Orthorhombic,
    /// 4/m
    TetragonalLow,
    /// 4/mmm
    TetragonalHigh,
    /// -3
    TrigonalLow,
    /// -3m (2-fold axes along a1 = x and at 60 and 120 degrees)
    TrigonalHigh,
    /// 6/m
    HexagonalLow,
    /// 6/mmm
    HexagonalHigh,
    /// m-3
    CubicLow,
    /// m-3m
    CubicHigh,
}

impl Laue {
    pub const ALL: [Laue; 11] = [
        Laue::Triclinic,
        Laue::Monoclinic,
        Laue::Orthorhombic,
        Laue::TetragonalLow,
        Laue::TetragonalHigh,
        Laue::TrigonalLow,
        Laue::TrigonalHigh,
        Laue::HexagonalLow,
        Laue::HexagonalHigh,
        Laue::CubicLow,
        Laue::CubicHigh,
    ];

    /// The number stored in `CrystalStructures` arrays (the numbering `.dream3d` files use):
    /// hexagonal 6/mmm 0, cubic m-3m 1, hexagonal 6/m 2, cubic m-3 3, triclinic 4,
    /// monoclinic 5, orthorhombic 6, tetragonal 4/m 7, tetragonal 4/mmm 8, trigonal -3 9,
    /// trigonal -3m 10. Unknown phases are 999.
    pub fn code(self) -> u32 {
        match self {
            Laue::HexagonalHigh => 0,
            Laue::CubicHigh => 1,
            Laue::HexagonalLow => 2,
            Laue::CubicLow => 3,
            Laue::Triclinic => 4,
            Laue::Monoclinic => 5,
            Laue::Orthorhombic => 6,
            Laue::TetragonalLow => 7,
            Laue::TetragonalHigh => 8,
            Laue::TrigonalLow => 9,
            Laue::TrigonalHigh => 10,
        }
    }

    pub const UNKNOWN_CODE: u32 = 999;

    pub fn from_code(c: u32) -> Option<Laue> {
        Laue::ALL.into_iter().find(|l| l.code() == c)
    }

    /// The symmetry number of a TSL/EDAX `.ang` phase header (43 cubic m-3m, 23 cubic m-3,
    /// 62 hexagonal 6/mmm, 6 hexagonal 6/m, 42 tetragonal 4/mmm, 4 tetragonal 4/m, 32 trigonal
    /// -3m, 3 trigonal -3, 22 orthorhombic, 2 monoclinic, 1 triclinic).
    pub fn from_tsl_symmetry(s: i64) -> Option<Laue> {
        Some(match s {
            43 => Laue::CubicHigh,
            23 => Laue::CubicLow,
            62 => Laue::HexagonalHigh,
            6 => Laue::HexagonalLow,
            42 => Laue::TetragonalHigh,
            4 => Laue::TetragonalLow,
            32 => Laue::TrigonalHigh,
            3 => Laue::TrigonalLow,
            22 => Laue::Orthorhombic,
            2 | 20 | 21 => Laue::Monoclinic,
            1 => Laue::Triclinic,
            _ => return None,
        })
    }

    pub fn tsl_symmetry(self) -> i64 {
        match self {
            Laue::CubicHigh => 43,
            Laue::CubicLow => 23,
            Laue::HexagonalHigh => 62,
            Laue::HexagonalLow => 6,
            Laue::TetragonalHigh => 42,
            Laue::TetragonalLow => 4,
            Laue::TrigonalHigh => 32,
            Laue::TrigonalLow => 3,
            Laue::Orthorhombic => 22,
            Laue::Monoclinic => 2,
            Laue::Triclinic => 1,
        }
    }

    /// The Laue group number of an Oxford `.ctf` phase line (1 triclinic ... 11 cubic m-3m).
    pub fn from_oxford(n: i64) -> Option<Laue> {
        Some(match n {
            1 => Laue::Triclinic,
            2 => Laue::Monoclinic,
            3 => Laue::Orthorhombic,
            4 => Laue::TetragonalLow,
            5 => Laue::TetragonalHigh,
            6 => Laue::TrigonalLow,
            7 => Laue::TrigonalHigh,
            8 => Laue::HexagonalLow,
            9 => Laue::HexagonalHigh,
            10 => Laue::CubicLow,
            11 => Laue::CubicHigh,
            _ => return None,
        })
    }

    pub fn label(self) -> &'static str {
        match self {
            Laue::Triclinic => "Triclinic -1",
            Laue::Monoclinic => "Monoclinic 2/m",
            Laue::Orthorhombic => "Orthorhombic mmm",
            Laue::TetragonalLow => "Tetragonal 4/m",
            Laue::TetragonalHigh => "Tetragonal 4/mmm",
            Laue::TrigonalLow => "Trigonal -3",
            Laue::TrigonalHigh => "Trigonal -3m",
            Laue::HexagonalLow => "Hexagonal 6/m",
            Laue::HexagonalHigh => "Hexagonal 6/mmm",
            Laue::CubicLow => "Cubic m-3",
            Laue::CubicHigh => "Cubic m-3m",
        }
    }

    pub fn from_label(s: &str) -> Option<Laue> {
        let t = s.trim().to_ascii_lowercase();
        Laue::ALL.into_iter().find(|l| l.label().eq_ignore_ascii_case(&t) || l.label().to_ascii_lowercase().split_whitespace().last() == Some(t.as_str()))
    }

    pub fn is_hexagonal(self) -> bool {
        matches!(self, Laue::HexagonalHigh | Laue::HexagonalLow | Laue::TrigonalHigh | Laue::TrigonalLow)
    }

    /// The proper rotations of the class, as quaternions with `w >= 0` (identity first).
    pub fn operators(self) -> &'static [Quat] {
        static CACHE: OnceLock<Vec<Vec<Quat>>> = OnceLock::new();
        let all = CACHE.get_or_init(|| Laue::ALL.iter().map(|l| closure(&l.generators())).collect());
        &all[Laue::ALL.iter().position(|l| *l == self).unwrap()]
    }

    fn generators(self) -> Vec<Quat> {
        use std::f64::consts::PI;
        let rot = |axis: [f64; 3], n: f64| axis_angle_to_quat(axis, 2.0 * PI / n);
        let (x, y, z, d) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 1.0, 1.0]);
        match self {
            Laue::Triclinic => vec![],
            Laue::Monoclinic => vec![rot(y, 2.0)],
            Laue::Orthorhombic => vec![rot(x, 2.0), rot(y, 2.0)],
            Laue::TetragonalLow => vec![rot(z, 4.0)],
            Laue::TetragonalHigh => vec![rot(z, 4.0), rot(x, 2.0)],
            Laue::TrigonalLow => vec![rot(z, 3.0)],
            Laue::TrigonalHigh => vec![rot(z, 3.0), rot(x, 2.0)],
            Laue::HexagonalLow => vec![rot(z, 6.0)],
            Laue::HexagonalHigh => vec![rot(z, 6.0), rot(x, 2.0)],
            Laue::CubicLow => vec![rot(x, 2.0), rot(y, 2.0), rot(d, 3.0)],
            Laue::CubicHigh => vec![rot(z, 4.0), rot(d, 3.0)],
        }
    }
}

/// The group generated by `gens` (rotations identified up to the sign of the quaternion).
fn closure(gens: &[Quat]) -> Vec<Quat> {
    let same = |a: &Quat, b: &Quat| a.iter().zip(b).all(|(p, q)| (p - q).abs() < 1e-9);
    let mut out: Vec<Quat> = vec![[1.0, 0.0, 0.0, 0.0]];
    let mut k = 0;
    while k < out.len() {
        for g in gens {
            let p = clean(positive(mul(*g, out[k])));
            if !out.iter().any(|o| same(o, &p)) {
                out.push(p);
            }
        }
        k += 1;
    }
    out
}

/// Snap values within rounding of 0, ±1/2, ±1/√2, ±√3/2, ±1 to them, so the operators are exact.
fn clean(q: Quat) -> Quat {
    let exact = [0.0, 0.5, std::f64::consts::FRAC_1_SQRT_2, 3f64.sqrt() * 0.5, 1.0];
    positive(q.map(|v| {
        let a = v.abs();
        match exact.iter().find(|e| (a - **e).abs() < 1e-12) {
            Some(e) => e.copysign(v),
            None => v,
        }
    }))
}
