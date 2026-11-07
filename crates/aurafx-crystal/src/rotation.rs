//! Rotation representations and conversions (see the crate documentation for conventions).

/// A unit quaternion (w, x, y, z).
pub type Quat = [f64; 4];
/// A 3x3 matrix, row major.
pub type Mat3 = [[f64; 3]; 3];

pub const IDENTITY: Quat = [1.0, 0.0, 0.0, 0.0];

/// Hamilton product `a b` (rotation `b` first, then `a`).
pub fn mul(a: Quat, b: Quat) -> Quat {
    [
        a[0] * b[0] - a[1] * b[1] - a[2] * b[2] - a[3] * b[3],
        a[0] * b[1] + a[1] * b[0] + a[2] * b[3] - a[3] * b[2],
        a[0] * b[2] - a[1] * b[3] + a[2] * b[0] + a[3] * b[1],
        a[0] * b[3] + a[1] * b[2] - a[2] * b[1] + a[3] * b[0],
    ]
}

pub fn conj(q: Quat) -> Quat {
    [q[0], -q[1], -q[2], -q[3]]
}

pub fn normalize(q: Quat) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n == 0.0 {
        IDENTITY
    } else {
        q.map(|v| v / n)
    }
}

/// The same rotation with `w >= 0` (and, when `w == 0`, the first nonzero component positive).
pub fn positive(q: Quat) -> Quat {
    let first = q.iter().copied().find(|v| *v != 0.0).unwrap_or(1.0);
    if q[0] < 0.0 || (q[0] == 0.0 && first < 0.0) {
        q.map(|v| -v)
    } else {
        q
    }
}

/// Rotate vector `v` by `q` (active): `q v q*`.
pub fn rotate(q: Quat, v: [f64; 3]) -> [f64; 3] {
    let m = quat_to_matrix(q);
    mat_vec(&m, v)
}

pub fn mat_vec(m: &Mat3, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

pub fn mat_mul(a: &Mat3, b: &Mat3) -> Mat3 {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}

pub fn transpose(a: &Mat3) -> Mat3 {
    std::array::from_fn(|i| std::array::from_fn(|j| a[j][i]))
}

/// The rotation matrix of `q` (equal to the orientation matrix `g`).
pub fn quat_to_matrix(q: Quat) -> Mat3 {
    let [w, x, y, z] = q;
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

/// The quaternion of a rotation matrix (Shepperd's method), `w >= 0`.
pub fn matrix_to_quat(m: &Mat3) -> Quat {
    let tr = m[0][0] + m[1][1] + m[2][2];
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s]
    };
    positive(normalize(q))
}

/// The orientation matrix of Bunge angles (φ1, Φ, φ2), radians.
pub fn euler_to_matrix(e: [f64; 3]) -> Mat3 {
    let (s1, c1) = e[0].sin_cos();
    let (s, c) = e[1].sin_cos();
    let (s2, c2) = e[2].sin_cos();
    [[c1 * c2 - s1 * s2 * c, s1 * c2 + c1 * s2 * c, s2 * s], [-c1 * s2 - s1 * c2 * c, -s1 * s2 + c1 * c2 * c, c2 * s], [s1 * s, -c1 * s, c]]
}

/// Bunge angles of an orientation matrix: φ1 and φ2 in [0, 2π), Φ in [0, π].
pub fn matrix_to_euler(m: &Mat3) -> [f64; 3] {
    let wrap = |a: f64| {
        let t = a.rem_euclid(std::f64::consts::TAU);
        if t >= std::f64::consts::TAU {
            0.0
        } else {
            t
        }
    };
    let cphi = m[2][2].clamp(-1.0, 1.0);
    let phi = cphi.acos();
    if (1.0 - cphi.abs()) > 1e-12 {
        [wrap(m[2][0].atan2(-m[2][1])), phi, wrap(m[0][2].atan2(m[1][2]))]
    } else if cphi > 0.0 {
        // Φ = 0: only φ1 + φ2 is defined; put it all in φ1.
        [wrap(m[0][1].atan2(m[0][0])), 0.0, 0.0]
    } else {
        // Φ = π: only φ1 - φ2 is defined.
        [wrap(m[0][1].atan2(m[0][0])), std::f64::consts::PI, 0.0]
    }
}

/// The quaternion of Bunge angles: the active rotations by -φ2 about z, -Φ about x and -φ1
/// about z, composed.
pub fn euler_to_quat(e: [f64; 3]) -> Quat {
    let half = |a: f64| (a * 0.5).sin_cos();
    let (s1, c1) = half(e[0]);
    let (s, c) = half(e[1]);
    let (s2, c2) = half(e[2]);
    let qz1 = [c1, 0.0, 0.0, -s1];
    let qx = [c, -s, 0.0, 0.0];
    let qz2 = [c2, 0.0, 0.0, -s2];
    positive(mul(mul(qz2, qx), qz1))
}

pub fn quat_to_euler(q: Quat) -> [f64; 3] {
    matrix_to_euler(&quat_to_matrix(q))
}

/// The active rotation by `angle` (radians) about `axis` (need not be unit length).
pub fn axis_angle_to_quat(axis: [f64; 3], angle: f64) -> Quat {
    let n = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
    if n == 0.0 || angle == 0.0 {
        return IDENTITY;
    }
    let (s, c) = (angle * 0.5).sin_cos();
    positive([c, axis[0] / n * s, axis[1] / n * s, axis[2] / n * s])
}

/// Angle (radians, 0..π) and unit axis of a quaternion (axis [0, 0, 1] for no rotation).
pub fn to_axis_angle(q: Quat) -> (f64, [f64; 3]) {
    let q = positive(q);
    let v = (q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if v < 1e-15 {
        return (0.0, [0.0, 0.0, 1.0]);
    }
    (2.0 * v.atan2(q[0]), [q[1] / v, q[2] / v, q[3] / v])
}

/// Rodrigues vector as four values: the unit axis and tan(ω/2) (infinite for 180°).
pub fn quat_to_rodrigues(q: Quat) -> [f64; 4] {
    let (w, n) = to_axis_angle(q);
    [n[0], n[1], n[2], (w * 0.5).tan()]
}

pub fn rodrigues_to_quat(r: [f64; 4]) -> Quat {
    axis_angle_to_quat([r[0], r[1], r[2]], 2.0 * r[3].atan())
}

/// Homochoric vector: the unit axis times (3/4 (ω - sin ω))^(1/3).
pub fn quat_to_homochoric(q: Quat) -> [f64; 3] {
    let (w, n) = to_axis_angle(q);
    let f = (0.75 * (w - w.sin())).cbrt();
    n.map(|v| v * f)
}

pub fn homochoric_to_quat(h: [f64; 3]) -> Quat {
    let r = (h[0] * h[0] + h[1] * h[1] + h[2] * h[2]).sqrt();
    if r < 1e-15 {
        return IDENTITY;
    }
    // Solve (3/4 (ω - sin ω))^(1/3) = r for ω in [0, π] (monotonic) by bisection then Newton.
    let target = r.powi(3) / 0.75;
    let (mut lo, mut hi) = (0.0f64, std::f64::consts::PI);
    for _ in 0..80 {
        let mid = 0.5 * (lo + hi);
        if mid - mid.sin() < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    axis_angle_to_quat(h, 0.5 * (lo + hi))
}

/// A uniformly distributed random rotation from three uniform numbers in [0, 1) (Shoemake).
pub fn random_quat(u1: f64, u2: f64, u3: f64) -> Quat {
    let (a, b) = ((1.0 - u1).sqrt(), u1.sqrt());
    let t = std::f64::consts::TAU;
    positive([b * (t * u3).cos(), a * (t * u2).sin(), a * (t * u2).cos(), b * (t * u3).sin()])
}

/// An orientation stored in an array: (x, y, z, w) quaternions, as files keep them.
pub fn quat_from_xyzw(v: [f64; 4]) -> Quat {
    positive(normalize([v[3], v[0], v[1], v[2]]))
}

pub fn quat_to_xyzw(q: Quat) -> [f64; 4] {
    [q[1], q[2], q[3], q[0]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &[f64], b: &[f64], tol: f64) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < tol)
    }

    #[test]
    fn conversions_round_trip() {
        for e in [[0.3, 0.9, 1.7], [5.9, 2.8, 0.1], [1.0, 0.0, 0.0], [2.0, std::f64::consts::PI, 0.0], [0.0, 1.5707963, 4.0]] {
            let q = euler_to_quat(e);
            let m = euler_to_matrix(e);
            assert!(close(&quat_to_matrix(q).concat(), &m.concat(), 1e-12));
            assert!(close(&matrix_to_quat(&m), &q, 1e-12));
            let back = euler_to_matrix(quat_to_euler(q));
            assert!(close(&back.concat(), &m.concat(), 1e-9));
            let (w, n) = to_axis_angle(q);
            assert!(close(&axis_angle_to_quat(n, w), &q, 1e-12));
            assert!(close(&rodrigues_to_quat(quat_to_rodrigues(q)), &q, 1e-9));
            assert!(close(&homochoric_to_quat(quat_to_homochoric(q)), &q, 1e-9));
        }
        // g maps the sample x axis to the crystal coordinates of that axis.
        let e = [0.5f64, 0.0, 0.0];
        let g = euler_to_matrix(e);
        assert!(close(&mat_vec(&g, [1.0, 0.0, 0.0]), &[e[0].cos(), -e[0].sin(), 0.0], 1e-12));
    }
}
