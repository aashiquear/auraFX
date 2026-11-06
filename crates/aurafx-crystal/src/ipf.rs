//! Orientation colors: the inverse pole figure (IPF) key and the Euler-angle key.
//!
//! The IPF color of an orientation for a sample direction `d` is the color of the crystal
//! direction `g d` once it is brought, by the crystal's symmetry and inversion, into the
//! standard stereographic triangle (sector) of its Laue class. In that sector the polar angle χ
//! (from the crystal z axis) and the azimuth η (from x) give red = 1 - χ/χmax(η),
//! and the rest split between green and blue by η across the sector, scaled by χ/χmax(η);
//! each channel is then square-rooted and the three are scaled so the largest is 1. The
//! corners are pure red, green and blue: for cubic crystals [001], [101] and [111]; for
//! hexagonal ones [0001], [2-1-10] and [10-10].

use crate::laue::Laue;
use crate::rotation::{euler_to_quat, rotate, Quat};

/// The standard sector of a Laue class: η from `eta_min` to `eta_max` (radians); χ up to 90°
/// except for cubic classes, where it ends at the plane through [101] and [111].
#[derive(Debug, Clone, Copy)]
pub struct IpfSector {
    pub eta_min: f64,
    pub eta_max: f64,
    pub cubic: bool,
    /// Cubic m-3: the sector spans 90° of η with the χ limit mirrored at 45°.
    pub cubic_low: bool,
}

impl IpfSector {
    /// The largest χ at azimuth `eta`.
    pub fn chi_max(&self, eta: f64) -> f64 {
        if !self.cubic {
            return std::f64::consts::FRAC_PI_2;
        }
        let e = if self.cubic_low && eta > std::f64::consts::FRAC_PI_4 { std::f64::consts::FRAC_PI_2 - eta } else { eta };
        let t = e.tan();
        (1.0 / (2.0 + t * t)).sqrt().acos()
    }

    pub fn contains(&self, p: [f64; 3]) -> Option<(f64, f64)> {
        const EPS: f64 = 1e-9;
        if p[2] < -EPS {
            return None;
        }
        let eta = p[1].atan2(p[0]);
        let eta = if eta < self.eta_min - EPS { eta + std::f64::consts::TAU } else { eta };
        if eta < self.eta_min - EPS || eta > self.eta_max + EPS {
            return None;
        }
        let eta = eta.clamp(self.eta_min, self.eta_max);
        let chi = p[2].clamp(-1.0, 1.0).acos();
        if chi > self.chi_max(eta) + EPS {
            return None;
        }
        Some((eta, chi))
    }
}

pub fn ipf_sector(laue: Laue) -> IpfSector {
    use std::f64::consts::PI;
    // (-3m: the mirrors lie at 30, 90 and 150 degrees, so its sector runs from 30 to 90.)
    let eta_min = if laue == Laue::TrigonalHigh { PI / 6.0 } else { 0.0 };
    let eta_max = match laue {
        Laue::CubicHigh => PI / 4.0,
        Laue::CubicLow => PI / 2.0,
        Laue::HexagonalHigh => PI / 6.0,
        Laue::HexagonalLow => PI / 3.0,
        Laue::TrigonalHigh => PI / 2.0,
        Laue::TrigonalLow => 2.0 * PI / 3.0,
        Laue::TetragonalHigh => PI / 4.0,
        Laue::TetragonalLow => PI / 2.0,
        Laue::Orthorhombic => PI / 2.0,
        Laue::Monoclinic => PI,
        Laue::Triclinic => 2.0 * PI,
    };
    IpfSector { eta_min, eta_max, cubic: matches!(laue, Laue::CubicHigh | Laue::CubicLow), cubic_low: laue == Laue::CubicLow }
}

/// The color of crystal direction `p` (already in the sector) as red, green, blue in 0..1.
pub fn sector_color(sector: &IpfSector, eta: f64, chi: f64) -> [f64; 3] {
    let cm = sector.chi_max(eta);
    let f = if cm > 0.0 { (chi / cm).clamp(0.0, 1.0) } else { 0.0 };
    let span = sector.eta_max - sector.eta_min;
    let b0 = if span > 0.0 { ((eta - sector.eta_min) / span).clamp(0.0, 1.0) } else { 0.0 };
    let mut rgb = [1.0 - f, (1.0 - b0) * f, b0 * f].map(|v: f64| v.max(0.0).sqrt());
    let m = rgb.iter().cloned().fold(0.0, f64::max);
    if m > 0.0 {
        rgb = rgb.map(|v| v / m);
    }
    rgb
}

/// The crystal direction `c` brought into the sector of `laue` (None only for a zero vector).
pub fn to_sector(c: [f64; 3], laue: Laue) -> Option<(f64, f64)> {
    let n = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
    if n == 0.0 {
        return None;
    }
    let c = c.map(|v| v / n);
    let sector = ipf_sector(laue);
    for s in laue.operators() {
        let p = rotate(*s, c);
        for p in [p, p.map(|v| -v)] {
            if let Some(ec) = sector.contains(p) {
                return Some(ec);
            }
        }
    }
    None
}

/// The IPF color (0..255) of orientation `q` for sample direction `d`.
pub fn ipf_color(q: Quat, laue: Laue, d: [f64; 3]) -> [u8; 3] {
    let c = rotate(q, d);
    match to_sector(c, laue) {
        Some((eta, chi)) => sector_color(&ipf_sector(laue), eta, chi).map(|v| (v * 255.0).round() as u8),
        None => [0, 0, 0],
    }
}

/// The Euler-angle color: (φ1 / 2π, Φ / π, φ2 / 2π) as red, green and blue.
pub fn euler_color(e: [f64; 3]) -> [u8; 3] {
    use std::f64::consts::{PI, TAU};
    let c = |v: f64, m: f64| ((v.rem_euclid(m) / m).clamp(0.0, 1.0) * 255.0).round() as u8;
    [c(e[0], TAU), (e[1].clamp(0.0, PI) / PI * 255.0).round() as u8, c(e[2], TAU)]
}

/// Convenience: the IPF color of Bunge angles.
pub fn ipf_color_of_euler(e: [f64; 3], laue: Laue, d: [f64; 3]) -> [u8; 3] {
    ipf_color(euler_to_quat(e), laue, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rotation::{matrix_to_quat, random_quat, Mat3};

    /// An orientation whose crystal direction along sample z is `c`.
    fn facing(c: [f64; 3]) -> Quat {
        let n = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        let z = c.map(|v| v / n);
        let a = if z[0].abs() < 0.9 { [1.0, 0.0, 0.0] } else { [0.0, 1.0, 0.0] };
        let x = {
            let d = a[0] * z[0] + a[1] * z[1] + a[2] * z[2];
            let v = [a[0] - d * z[0], a[1] - d * z[1], a[2] - d * z[2]];
            let m = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            v.map(|t| t / m)
        };
        let y = [z[1] * x[2] - z[2] * x[1], z[2] * x[0] - z[0] * x[2], z[0] * x[1] - z[1] * x[0]];
        // g maps sample axes to crystal directions: its columns are the crystal coordinates of x, y, z.
        let g: Mat3 = std::array::from_fn(|i| [x[i], y[i], z[i]]);
        matrix_to_quat(&g)
    }

    #[test]
    fn corners_are_pure_colors() {
        let z = [0.0, 0.0, 1.0];
        assert_eq!(ipf_color(facing([0.0, 0.0, 1.0]), Laue::CubicHigh, z), [255, 0, 0]);
        assert_eq!(ipf_color(facing([1.0, 0.0, 1.0]), Laue::CubicHigh, z), [0, 255, 0]);
        assert_eq!(ipf_color(facing([1.0, 1.0, 1.0]), Laue::CubicHigh, z), [0, 0, 255]);
        // Symmetry-equivalent directions give the same color.
        assert_eq!(ipf_color(facing([0.0, -1.0, 0.0]), Laue::CubicHigh, z), [255, 0, 0]);
        assert_eq!(ipf_color(facing([-1.0, 1.0, -1.0]), Laue::CubicHigh, z), [0, 0, 255]);
        // Hexagonal: [0001] red, a1 = [2-1-10] (x) green, [10-10] (30 degrees from x) blue.
        assert_eq!(ipf_color(facing([0.0, 0.0, 1.0]), Laue::HexagonalHigh, z), [255, 0, 0]);
        assert_eq!(ipf_color(facing([1.0, 0.0, 0.0]), Laue::HexagonalHigh, z), [0, 255, 0]);
        let b = 30f64.to_radians();
        assert_eq!(ipf_color(facing([b.cos(), b.sin(), 0.0]), Laue::HexagonalHigh, z), [0, 0, 255]);
    }

    #[test]
    fn every_direction_lands_in_the_sector() {
        let mut k = 0u64;
        for l in Laue::ALL {
            for i in 0..200 {
                k += 1;
                let u = |j: u64| ((k * 2654435761 + j * 40503 + i) % 10007) as f64 / 10007.0;
                let q = random_quat(u(1), u(2), u(3));
                let c = rotate(q, [0.3, -0.2, 0.9]);
                assert!(to_sector(c, l).is_some(), "{l:?}");
                // The color is the same for every symmetry equivalent.
                let col = ipf_color(q, l, [0.0, 0.0, 1.0]);
                for s in l.operators() {
                    let col2 = ipf_color(crate::rotation::mul(*s, q), l, [0.0, 0.0, 1.0]);
                    assert!(col.iter().zip(col2).all(|(a, b)| (*a as i32 - b as i32).abs() <= 1), "{l:?} {col:?} {col2:?}");
                }
            }
        }
    }
}
