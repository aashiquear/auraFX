//! Trackball camera.

use aurafx_data::Bounds;
use glam::{DMat4, DQuat, DVec3, Mat4};

#[derive(Debug, Clone)]
pub struct Camera {
    pub eye: DVec3,
    pub target: DVec3,
    pub up: DVec3,
    pub fov_y_deg: f64,
    pub orthographic: bool,
    /// Half-height of the ortho view volume.
    pub ortho_height: f64,
    pub near: f64,
    pub far: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            eye: DVec3::new(0.0, 0.0, 5.0),
            target: DVec3::ZERO,
            up: DVec3::Y,
            fov_y_deg: 30.0,
            orthographic: false,
            ortho_height: 1.0,
            near: 0.01,
            far: 100.0,
        }
    }
}

impl Camera {
    pub fn distance(&self) -> f64 {
        (self.eye - self.target).length()
    }

    pub fn forward(&self) -> DVec3 {
        (self.target - self.eye).normalize_or_zero()
    }

    pub fn right(&self) -> DVec3 {
        self.forward().cross(self.up).normalize_or_zero()
    }

    /// Fit the view to `bounds`, keeping the current view direction.
    pub fn reset_to_bounds(&mut self, bounds: &Bounds) {
        if bounds.is_empty() {
            return;
        }
        let c = DVec3::from_array(bounds.center());
        let radius = (bounds.diagonal() * 0.5).max(1e-6);
        let dir = {
            let d = self.eye - self.target;
            if d.length() < 1e-12 {
                DVec3::new(0.0, 0.0, 1.0)
            } else {
                d.normalize()
            }
        };
        let dist = radius / (self.fov_y_deg.to_radians() * 0.5).sin() * 1.05;
        self.target = c;
        self.eye = c + dir * dist;
        self.ortho_height = radius * 1.05;
        self.update_clip(bounds);
    }

    pub fn update_clip(&mut self, bounds: &Bounds) {
        if bounds.is_empty() {
            return;
        }
        let radius = (bounds.diagonal() * 0.5).max(1e-6);
        let c = DVec3::from_array(bounds.center());
        let d = (self.eye - c).length();
        self.near = ((d - radius) * 0.5).max(radius * 1e-4).max(1e-6);
        self.far = (d + radius) * 2.0 + radius;
    }

    /// Look from `direction` (pointing from the target toward the eye) with `up`.
    pub fn set_view(&mut self, direction: [f64; 3], up: [f64; 3], bounds: &Bounds) {
        let dist = self.distance().max(1e-6);
        let dir = DVec3::from_array(direction).normalize_or_zero();
        self.eye = self.target + dir * dist;
        self.up = DVec3::from_array(up).normalize_or_zero();
        if !bounds.is_empty() {
            self.reset_to_bounds(bounds);
        }
    }

    /// Rotate around the target: `dx`, `dy` in normalized screen units (-1..1 ≈ full drag).
    pub fn rotate(&mut self, dx: f64, dy: f64) {
        let yaw = DQuat::from_axis_angle(self.up.normalize_or_zero(), -dx * std::f64::consts::PI);
        let right = self.right();
        let pitch = DQuat::from_axis_angle(right, -dy * std::f64::consts::PI);
        let q = yaw * pitch;
        let rel = self.eye - self.target;
        self.eye = self.target + q * rel;
        self.up = (q * self.up).normalize_or_zero();
    }

    /// Roll about the view axis (radians).
    pub fn roll(&mut self, angle: f64) {
        let q = DQuat::from_axis_angle(self.forward(), angle);
        self.up = (q * self.up).normalize_or_zero();
    }

    /// Pan in screen plane; `dx`, `dy` in normalized units of the viewport height.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        let h = if self.orthographic { 2.0 * self.ortho_height } else { 2.0 * self.distance() * (self.fov_y_deg.to_radians() * 0.5).tan() };
        let right = self.right();
        let up = right.cross(self.forward()).normalize_or_zero();
        let delta = right * (-dx * h) + up * (dy * h);
        self.eye += delta;
        self.target += delta;
    }

    /// Zoom by factor (>1 zooms in).
    pub fn zoom(&mut self, factor: f64) {
        if self.orthographic {
            self.ortho_height /= factor;
        } else {
            let rel = self.eye - self.target;
            self.eye = self.target + rel / factor;
        }
    }

    pub fn view_matrix(&self) -> DMat4 {
        // Right-handed look-at.
        let f = (self.target - self.eye).normalize_or_zero();
        let s = f.cross(self.up).normalize_or_zero();
        let u = s.cross(f);
        DMat4::from_cols(
            glam::DVec4::new(s.x, u.x, -f.x, 0.0),
            glam::DVec4::new(s.y, u.y, -f.y, 0.0),
            glam::DVec4::new(s.z, u.z, -f.z, 0.0),
            glam::DVec4::new(-s.dot(self.eye), -u.dot(self.eye), f.dot(self.eye), 1.0),
        )
    }

    /// Projection with wgpu's 0..1 depth range.
    pub fn proj_matrix(&self, aspect: f64) -> DMat4 {
        let (n, fa) = (self.near, self.far);
        if self.orthographic {
            let h = self.ortho_height;
            let w = h * aspect;
            DMat4::from_cols(
                glam::DVec4::new(1.0 / w, 0.0, 0.0, 0.0),
                glam::DVec4::new(0.0, 1.0 / h, 0.0, 0.0),
                glam::DVec4::new(0.0, 0.0, -1.0 / (fa - n), 0.0),
                glam::DVec4::new(0.0, 0.0, -n / (fa - n), 1.0),
            )
        } else {
            let f = 1.0 / (self.fov_y_deg.to_radians() * 0.5).tan();
            DMat4::from_cols(
                glam::DVec4::new(f / aspect, 0.0, 0.0, 0.0),
                glam::DVec4::new(0.0, f, 0.0, 0.0),
                glam::DVec4::new(0.0, 0.0, fa / (n - fa), -1.0),
                glam::DVec4::new(0.0, 0.0, n * fa / (n - fa), 0.0),
            )
        }
    }

    /// View-projection matrix relative to `origin` (f32 precision trick for large coordinates).
    pub fn view_proj_f32(&self, aspect: f64, origin: DVec3) -> Mat4 {
        let m = self.proj_matrix(aspect) * self.view_matrix() * DMat4::from_translation(origin);
        m.as_mat4()
    }

    /// Camera-space pixel size at the target distance (world units per pixel).
    pub fn world_per_pixel(&self, viewport_height_px: f64) -> f64 {
        let h = if self.orthographic { 2.0 * self.ortho_height } else { 2.0 * self.distance() * (self.fov_y_deg.to_radians() * 0.5).tan() };
        h / viewport_height_px.max(1.0)
    }
}
