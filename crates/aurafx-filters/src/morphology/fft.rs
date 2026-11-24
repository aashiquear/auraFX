//! Fourier transforms of fields on the grid: any length along every axis (powers of two
//! directly, the others through a chirp transform), the three axes one after another.

use rayon::prelude::*;
use std::f64::consts::PI;

/// A complex number: real and imaginary parts.
pub type C = [f64; 2];

#[inline]
pub fn mul(a: C, b: C) -> C {
    [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]]
}

/// The transform of one length.
pub struct Plan {
    n: usize,
    /// The power of two the work is done at (`n` itself when it is one).
    m: usize,
    /// exp(-2 pi i k / m), k below m / 2.
    roots: Vec<C>,
    /// For the other lengths: exp(-i pi k^2 / n) and the transform of its conjugate, wrapped.
    chirp: Vec<C>,
    chirp_hat: Vec<C>,
}

fn radix2(a: &mut [C], roots: &[C], inverse: bool) {
    let m = a.len();
    let mut j = 0;
    for i in 1..m {
        let mut bit = m >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            a.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= m {
        let step = m / len;
        for start in (0..m).step_by(len) {
            for k in 0..len / 2 {
                let mut w = roots[k * step];
                if inverse {
                    w[1] = -w[1];
                }
                let (u, v) = (a[start + k], mul(a[start + k + len / 2], w));
                a[start + k] = [u[0] + v[0], u[1] + v[1]];
                a[start + k + len / 2] = [u[0] - v[0], u[1] - v[1]];
            }
        }
        len <<= 1;
    }
}

impl Plan {
    pub fn new(n: usize) -> Plan {
        let n = n.max(1);
        let pow2 = n.is_power_of_two();
        let m = if pow2 { n } else { (2 * n - 1).next_power_of_two() };
        let roots: Vec<C> = (0..m / 2)
            .map(|k| {
                let a = -2.0 * PI * k as f64 / m as f64;
                [a.cos(), a.sin()]
            })
            .collect();
        let (mut chirp, mut chirp_hat) = (Vec::new(), Vec::new());
        if !pow2 {
            chirp = (0..n)
                .map(|k| {
                    let a = -PI * ((k * k) % (2 * n)) as f64 / n as f64;
                    [a.cos(), a.sin()]
                })
                .collect();
            chirp_hat = vec![[0.0; 2]; m];
            for k in 0..n {
                let c = [chirp[k][0], -chirp[k][1]];
                chirp_hat[k] = c;
                if k > 0 {
                    chirp_hat[m - k] = c;
                }
            }
            radix2(&mut chirp_hat, &roots, false);
        }
        Plan { n, m, roots, chirp, chirp_hat }
    }

    /// In place; the inverse divides by the length. `work` is scratch space kept between calls.
    pub fn run(&self, x: &mut [C], inverse: bool, work: &mut Vec<C>) {
        let n = self.n;
        if n == 1 {
            return;
        }
        if self.chirp.is_empty() {
            radix2(x, &self.roots, inverse);
        } else {
            // The inverse as the conjugate of the transform of the conjugate.
            if inverse {
                x.iter_mut().for_each(|v| v[1] = -v[1]);
            }
            work.clear();
            work.resize(self.m, [0.0; 2]);
            for k in 0..n {
                work[k] = mul(x[k], self.chirp[k]);
            }
            radix2(work, &self.roots, false);
            for (w, c) in work.iter_mut().zip(&self.chirp_hat) {
                *w = mul(*w, *c);
            }
            radix2(work, &self.roots, true);
            let s = 1.0 / self.m as f64;
            for k in 0..n {
                let v = mul(work[k], self.chirp[k]);
                x[k] = [v[0] * s, v[1] * s];
            }
            if inverse {
                x.iter_mut().for_each(|v| v[1] = -v[1]);
            }
        }
        if inverse {
            let s = 1.0 / n as f64;
            x.iter_mut().for_each(|v| {
                v[0] *= s;
                v[1] *= s;
            });
        }
    }
}

/// The transform of a field on a grid of `n` cells (x fastest).
pub struct Fft3 {
    pub n: [usize; 3],
    plans: [Plan; 3],
}

impl Fft3 {
    pub fn new(n: [usize; 3]) -> Fft3 {
        Fft3 { n, plans: [Plan::new(n[0]), Plan::new(n[1]), Plan::new(n[2])] }
    }
    pub fn len(&self) -> usize {
        self.n[0] * self.n[1] * self.n[2]
    }
    pub fn run(&self, data: &mut [C], inverse: bool) {
        let [nx, ny, nz] = self.n;
        assert_eq!(data.len(), nx * ny * nz);
        if nx > 1 {
            data.par_chunks_mut(nx).for_each_init(Vec::new, |work, row| self.plans[0].run(row, inverse, work));
        }
        if ny > 1 {
            data.par_chunks_mut(nx * ny).for_each_init(
                || (vec![[0.0; 2]; ny], Vec::new()),
                |(col, work), slab| {
                    for x in 0..nx {
                        for y in 0..ny {
                            col[y] = slab[x + nx * y];
                        }
                        self.plans[1].run(col, inverse, work);
                        for y in 0..ny {
                            slab[x + nx * y] = col[y];
                        }
                    }
                },
            );
        }
        if nz > 1 {
            let plane = nx * ny;
            let shared: &[C] = data;
            let cols: Vec<Vec<C>> = (0..plane)
                .into_par_iter()
                .map_init(Vec::new, |work, q| {
                    let mut col: Vec<C> = (0..nz).map(|z| shared[q + plane * z]).collect();
                    self.plans[2].run(&mut col, inverse, work);
                    col
                })
                .collect();
            data.par_chunks_mut(plane).enumerate().for_each(|(z, slab)| {
                for (q, v) in slab.iter_mut().enumerate() {
                    *v = cols[q][z];
                }
            });
        }
    }
    pub fn forward(&self, real: &[f64]) -> Vec<C> {
        let mut d: Vec<C> = real.iter().map(|v| [*v, 0.0]).collect();
        self.run(&mut d, false);
        d
    }
    /// The signed frequency index of the cell `c` along each axis (-n/2 .. n/2).
    pub fn index(&self, c: usize) -> [i64; 3] {
        let p = [c % self.n[0], (c / self.n[0]) % self.n[1], c / (self.n[0] * self.n[1])];
        std::array::from_fn(|k| if p[k] > self.n[k] / 2 { p[k] as i64 - self.n[k] as i64 } else { p[k] as i64 })
    }
}

/// The eigenvalues of a symmetric matrix (`a` holds its lower triangle, row by row in a full
/// square; it is destroyed), smallest first: reduction to three diagonals by reflections,
/// then the shifted QL iteration.
pub fn symmetric_eigenvalues(a: &mut [f64], n: usize) -> Vec<f64> {
    let at = |i: usize, j: usize| i * n + j;
    let (mut d, mut e) = (vec![0.0; n], vec![0.0; n]);
    for i in (1..n).rev() {
        let l = i - 1;
        let mut h = 0.0;
        if l > 0 {
            let scale: f64 = (0..=l).map(|k| a[at(i, k)].abs()).sum();
            if scale == 0.0 {
                e[i] = a[at(i, l)];
            } else {
                for k in 0..=l {
                    a[at(i, k)] /= scale;
                    h += a[at(i, k)] * a[at(i, k)];
                }
                let f = a[at(i, l)];
                let g = if f >= 0.0 { -h.sqrt() } else { h.sqrt() };
                e[i] = scale * g;
                h -= f * g;
                a[at(i, l)] = f - g;
                let mut ff = 0.0;
                for j in 0..=l {
                    let mut g = 0.0;
                    for k in 0..=j {
                        g += a[at(j, k)] * a[at(i, k)];
                    }
                    for k in j + 1..=l {
                        g += a[at(k, j)] * a[at(i, k)];
                    }
                    e[j] = g / h;
                    ff += e[j] * a[at(i, j)];
                }
                let hh = ff / (h + h);
                for j in 0..=l {
                    let f = a[at(i, j)];
                    let g = e[j] - hh * f;
                    e[j] = g;
                    for k in 0..=j {
                        a[at(j, k)] -= f * e[k] + g * a[at(i, k)];
                    }
                }
            }
        } else {
            e[i] = a[at(i, l)];
        }
        d[i] = h;
    }
    for i in 0..n {
        d[i] = a[at(i, i)];
    }
    for i in 1..n {
        e[i - 1] = e[i];
    }
    if n > 0 {
        e[n - 1] = 0.0;
    }
    for l in 0..n {
        let mut iter = 0;
        loop {
            let mut m = l;
            while m + 1 < n {
                let dd = d[m].abs() + d[m + 1].abs();
                if e[m].abs() <= f64::EPSILON * dd {
                    break;
                }
                m += 1;
            }
            if m == l || iter > 200 {
                break;
            }
            iter += 1;
            let mut g = (d[l + 1] - d[l]) / (2.0 * e[l]);
            let mut r = g.hypot(1.0);
            g = d[m] - d[l] + e[l] / (g + if g >= 0.0 { r } else { -r });
            let (mut s, mut c, mut p) = (1.0, 1.0, 0.0);
            let mut early = false;
            for i in (l..m).rev() {
                let f = s * e[i];
                let b = c * e[i];
                r = f.hypot(g);
                e[i + 1] = r;
                if r == 0.0 {
                    d[i + 1] -= p;
                    e[m] = 0.0;
                    early = true;
                    break;
                }
                s = f / r;
                c = g / r;
                g = d[i + 1] - p;
                r = (d[i] - g) * s + 2.0 * c * b;
                p = s * r;
                d[i + 1] = g + p;
                g = c * r - b;
            }
            if early {
                continue;
            }
            d[l] -= p;
            e[l] = g;
            e[m] = 0.0;
        }
    }
    d.sort_by(f64::total_cmp);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_of_any_length_invert_and_match_the_sum() {
        for n in [1usize, 2, 8, 12, 15, 17] {
            let plan = Plan::new(n);
            let x: Vec<C> = (0..n).map(|k| [(k as f64 * 0.7).sin() + 0.3, (k as f64 * 1.3).cos()]).collect();
            let mut y = x.clone();
            let mut work = Vec::new();
            plan.run(&mut y, false, &mut work);
            for k in 0..n {
                let mut s = [0.0; 2];
                for (j, v) in x.iter().enumerate() {
                    let a = -2.0 * PI * (j * k) as f64 / n as f64;
                    let t = mul(*v, [a.cos(), a.sin()]);
                    s = [s[0] + t[0], s[1] + t[1]];
                }
                assert!((s[0] - y[k][0]).abs() < 1e-9 && (s[1] - y[k][1]).abs() < 1e-9, "n {n} k {k}");
            }
            plan.run(&mut y, true, &mut work);
            for k in 0..n {
                assert!((x[k][0] - y[k][0]).abs() < 1e-10 && (x[k][1] - y[k][1]).abs() < 1e-10);
            }
        }
        let f = Fft3::new([6, 5, 4]);
        let v: Vec<f64> = (0..120).map(|c| ((c * 37) % 11) as f64).collect();
        let mut d = f.forward(&v);
        assert!((d[0][0] - v.iter().sum::<f64>()).abs() < 1e-9);
        f.run(&mut d, true);
        assert!(v.iter().zip(&d).all(|(a, b)| (a - b[0]).abs() < 1e-9 && b[1].abs() < 1e-9));
    }

    #[test]
    fn eigenvalues_of_a_known_matrix() {
        // The full matrix min(i, j), i and j from 1: 1 / (4 sin^2((2k - 1) pi / (4n + 2))).
        let n = 12;
        let mut a = vec![0.0; n * n];
        for i in 0..n {
            for j in 0..n {
                a[i * n + j] = (i.min(j) + 1) as f64;
            }
        }
        let ev = symmetric_eigenvalues(&mut a, n);
        for (k, v) in ev.iter().rev().enumerate() {
            let want = 0.25 / ((2 * k + 1) as f64 * PI / (4 * n + 2) as f64).sin().powi(2);
            assert!((v - want).abs() < 1e-9 * want.max(1.0), "{v} {want}");
        }
    }
}
