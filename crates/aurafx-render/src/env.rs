//! Environment maps: an equirectangular picture of the surroundings (+Z up) that lights the
//! scene (ambient light from its spherical-harmonic irradiance, reflections) and can be shown
//! as the background. Built in: "sky", "studio", "white"; or any .hdr/.png/.jpg file.

use anyhow::{bail, Context, Result};
use glam::Vec3;
use std::f32::consts::PI;

/// Built-in environments.
pub const BUILT_IN: [&str; 3] = ["sky", "studio", "white"];

#[derive(Clone)]
pub struct Environment {
    pub name: String,
    pub width: usize,
    pub height: usize,
    /// Linear RGB, row by row from the top (+Z).
    pub pixels: Vec<Vec3>,
    /// Irradiance spherical harmonics (9 RGB coefficients, already convolved with the cosine
    /// lobe and divided by π, so `irradiance(n)` is the diffuse light a white surface reflects).
    pub sh: [Vec3; 9],
}

/// Unit direction for equirectangular coordinates (u, v in 0..1; v = 0 at +Z).
pub fn direction(u: f32, v: f32) -> Vec3 {
    let phi = (u - 0.5) * 2.0 * PI;
    let theta = v * PI;
    Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos())
}

/// Equirectangular coordinates of a direction.
pub fn uv(d: Vec3) -> (f32, f32) {
    let d = d.normalize_or_zero();
    (d.y.atan2(d.x) / (2.0 * PI) + 0.5, d.z.clamp(-1.0, 1.0).acos() / PI)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn procedural(name: &str) -> Option<(usize, usize, Vec<Vec3>)> {
    let (w, h) = (512, 256);
    let f: Box<dyn Fn(Vec3) -> Vec3> = match name {
        "white" => Box::new(|_| Vec3::ONE),
        "sky" => {
            let sun = direction(0.5 + 30.0 / 360.0, (90.0 - 40.0) / 180.0);
            Box::new(move |d: Vec3| {
                let up = d.z;
                let sky = Vec3::new(0.62, 0.78, 1.0).lerp(Vec3::new(0.22, 0.42, 0.85), smoothstep(0.0, 0.7, up));
                let ground = Vec3::new(0.30, 0.28, 0.25).lerp(Vec3::new(0.18, 0.17, 0.16), smoothstep(0.0, -0.6, up));
                let mut c = ground.lerp(sky, smoothstep(-0.02, 0.02, up));
                // The sun: a small bright disc with a glow.
                let cos = d.dot(sun);
                c += Vec3::new(1.0, 0.95, 0.85) * (40.0 * smoothstep(0.9993, 0.9996, cos) + 0.6 * smoothstep(0.96, 1.0, cos).powi(4));
                c
            })
        }
        "studio" => Box::new(|d: Vec3| {
            // Dark room, a large soft light overhead and two side panels.
            let mut c = Vec3::splat(0.08) + Vec3::splat(0.06) * smoothstep(-0.2, 0.3, d.z);
            let panel = |center: Vec3, size: f32, power: f32| power * smoothstep(size.cos(), (size * 0.8).cos(), d.dot(center.normalize()));
            c += Vec3::splat(panel(Vec3::Z, 0.6, 1.3));
            c += Vec3::new(1.0, 0.97, 0.92) * panel(Vec3::new(1.0, 1.2, 0.5), 0.35, 1.4);
            c += Vec3::new(0.9, 0.95, 1.0) * panel(Vec3::new(-1.0, 0.8, 0.3), 0.3, 0.8);
            c
        }),
        _ => return None,
    };
    let mut px = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            px.push(f(direction((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32)));
        }
    }
    Some((w, h, px))
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

impl Environment {
    /// A built-in environment by name, or an image file (.hdr is linear; other formats sRGB).
    pub fn load(spec: &str) -> Result<Environment> {
        let (w, h, pixels) = match procedural(&spec.to_ascii_lowercase()) {
            Some(p) => p,
            None => {
                let path = std::path::Path::new(spec);
                if !path.exists() {
                    bail!("no environment {spec:?}: use {} or an image file", BUILT_IN.join(", "));
                }
                let img = image::open(path).with_context(|| format!("reading {}", path.display()))?;
                let hdr = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("hdr"));
                let rgb = img.to_rgb32f();
                let (mut w, mut h) = (rgb.width() as usize, rgb.height() as usize);
                let mut px: Vec<Vec3> = rgb
                    .pixels()
                    .map(|p| if hdr { Vec3::new(p[0], p[1], p[2]) } else { Vec3::new(srgb_to_linear(p[0]), srgb_to_linear(p[1]), srgb_to_linear(p[2])) })
                    .collect();
                // Keep it at most 2048 wide (halving by box filter).
                while w > 2048 && h > 1 {
                    let (nw, nh) = (w / 2, h / 2);
                    let mut half = Vec::with_capacity(nw * nh);
                    for y in 0..nh {
                        for x in 0..nw {
                            let a = |dx, dy| px[(2 * y + dy) * w + 2 * x + dx];
                            half.push((a(0, 0) + a(1, 0) + a(0, 1) + a(1, 1)) * 0.25);
                        }
                    }
                    (w, h, px) = (nw, nh, half);
                }
                (w, h, px)
            }
        };
        let sh = project_sh(w, h, &pixels);
        Ok(Environment { name: spec.to_string(), width: w, height: h, pixels, sh })
    }

    /// Radiance coming from direction `d` (bilinear).
    pub fn sample(&self, d: Vec3) -> Vec3 {
        let (u, v) = uv(d);
        let x = u * self.width as f32 - 0.5;
        let y = (v * self.height as f32 - 0.5).clamp(0.0, (self.height - 1) as f32);
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let wrap = |x: f32| (x as i64).rem_euclid(self.width as i64) as usize;
        let row = |y: f32| (y as usize).min(self.height - 1);
        let p = |x: f32, y: f32| self.pixels[row(y) * self.width + wrap(x)];
        let a = p(x0, y0).lerp(p(x0 + 1.0, y0), fx);
        let b = p(x0, y0 + 1.0).lerp(p(x0 + 1.0, y0 + 1.0), fx);
        a.lerp(b, fy)
    }

    /// Diffuse light reaching a surface facing `n` (white, Lambertian).
    pub fn irradiance(&self, n: Vec3) -> Vec3 {
        let b = sh_basis(n.normalize_or_zero());
        let mut e = Vec3::ZERO;
        for k in 0..9 {
            e += self.sh[k] * b[k];
        }
        e.max(Vec3::ZERO)
    }

    /// Mip levels for the GPU: halved by box filtering down to 1 pixel high; RGBA16F texels.
    pub fn mips(&self) -> Vec<(usize, usize, Vec<half::f16>)> {
        let mut out = Vec::new();
        let (mut w, mut h, mut px) = (self.width, self.height, self.pixels.clone());
        loop {
            out.push((
                w,
                h,
                px.iter()
                    .flat_map(|c| {
                        [half::f16::from_f32(c.x.min(60000.0)), half::f16::from_f32(c.y.min(60000.0)), half::f16::from_f32(c.z.min(60000.0)), half::f16::ONE]
                    })
                    .collect(),
            ));
            if w <= 1 || h <= 1 {
                break;
            }
            let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
            let mut next = Vec::with_capacity(nw * nh);
            for y in 0..nh {
                for x in 0..nw {
                    let a = |dx: usize, dy: usize| px[(2 * y + dy).min(h - 1) * w + (2 * x + dx).min(w - 1)];
                    next.push((a(0, 0) + a(1, 0) + a(0, 1) + a(1, 1)) * 0.25);
                }
            }
            (w, h, px) = (nw, nh, next);
        }
        out
    }
}

/// Real spherical harmonics up to l = 2.
fn sh_basis(d: Vec3) -> [f32; 9] {
    let (x, y, z) = (d.x, d.y, d.z);
    [
        0.282095,
        0.488603 * y,
        0.488603 * z,
        0.488603 * x,
        1.092548 * x * y,
        1.092548 * y * z,
        0.315392 * (3.0 * z * z - 1.0),
        1.092548 * x * z,
        0.546274 * (x * x - y * y),
    ]
}

/// Project radiance onto SH and convolve with the clamped cosine (Ramamoorthi & Hanrahan),
/// divided by π so the result is reflected light for albedo 1.
fn project_sh(w: usize, h: usize, px: &[Vec3]) -> [Vec3; 9] {
    let mut c = [Vec3::ZERO; 9];
    for y in 0..h {
        let theta = (y as f32 + 0.5) / h as f32 * PI;
        let dw = (2.0 * PI / w as f32) * (PI / h as f32) * theta.sin();
        for x in 0..w {
            let d = direction((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            let b = sh_basis(d);
            let l = px[y * w + x];
            for k in 0..9 {
                c[k] += l * (b[k] * dw);
            }
        }
    }
    let band = [PI, 2.0 * PI / 3.0, 2.0 * PI / 3.0, 2.0 * PI / 3.0, PI / 4.0, PI / 4.0, PI / 4.0, PI / 4.0, PI / 4.0];
    for k in 0..9 {
        c[k] *= band[k] / PI;
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_room_irradiance_is_one() {
        let e = Environment::load("white").unwrap();
        for n in [Vec3::Z, Vec3::X, Vec3::new(1.0, -2.0, 0.5).normalize()] {
            let i = e.irradiance(n);
            assert!((i - Vec3::ONE).abs().max_element() < 0.01, "{i}");
        }
        assert!((e.sample(Vec3::Y) - Vec3::ONE).length() < 1e-5);
    }

    #[test]
    fn sky_is_brighter_above() {
        let e = Environment::load("sky").unwrap();
        assert!(e.irradiance(Vec3::Z).element_sum() > 2.0 * e.irradiance(-Vec3::Z).element_sum());
        assert!(e.sample(Vec3::Z).z > e.sample(Vec3::Z).x, "blue sky");
        let (u, v) = uv(Vec3::new(0.3, -0.4, 0.5));
        assert!((direction(u, v) - Vec3::new(0.3, -0.4, 0.5).normalize()).length() < 1e-5);
        assert!(Environment::load("no such place").is_err());
    }
}
