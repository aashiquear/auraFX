//! Keyframe animation: camera keyframes and numeric parameter tracks over a normalized
//! animation time `t` in [0, 1], optionally stepping through the data's time steps.

use crate::session::{NodeId, Session};
use crate::state::CameraState;
use aurafx_filters::params::ParamValue;
use glam::DVec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraKey {
    pub t: f64,
    pub camera: CameraState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamKey {
    pub t: f64,
    pub value: ParamValue,
}

/// A numeric parameter (float, int or 3-vector) of one pipeline object, keyed over time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParamTrack {
    pub node: NodeId,
    pub key: String,
    pub keys: Vec<ParamKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Animation {
    pub camera: Vec<CameraKey>,
    pub tracks: Vec<ParamTrack>,
    /// Also step through the data's time steps over the animation.
    pub animate_time: bool,
    /// Number of frames in an exported keyframe animation.
    pub frames: usize,
}

impl Default for Animation {
    fn default() -> Self {
        Self { camera: vec![], tracks: vec![], animate_time: true, frames: 120 }
    }
}

/// Segment of sorted keys around `t`: (index before, index after, local fraction).
fn segment(ts: &[f64], t: f64) -> Option<(usize, usize, f64)> {
    match ts.len() {
        0 => None,
        1 => Some((0, 0, 0.0)),
        n => {
            if t <= ts[0] {
                return Some((0, 0, 0.0));
            }
            if t >= ts[n - 1] {
                return Some((n - 1, n - 1, 0.0));
            }
            let i = ts.windows(2).position(|w| t >= w[0] && t <= w[1]).unwrap_or(n - 2);
            let span = ts[i + 1] - ts[i];
            Some((i, i + 1, if span > 0.0 { (t - ts[i]) / span } else { 0.0 }))
        }
    }
}

fn slerp(a: DVec3, b: DVec3, u: f64) -> DVec3 {
    let (a, b) = (a.normalize_or_zero(), b.normalize_or_zero());
    let dot = a.dot(b).clamp(-1.0, 1.0);
    let theta = dot.acos();
    if theta < 1e-6 {
        return a.lerp(b, u).normalize_or_zero();
    }
    if (std::f64::consts::PI - theta).abs() < 1e-6 {
        // Opposite directions: go round through any perpendicular.
        let perp = a.any_orthonormal_vector();
        let half = if u < 0.5 { slerp(a, perp, u * 2.0) } else { slerp(perp, b, (u - 0.5) * 2.0) };
        return half;
    }
    (a * ((1.0 - u) * theta).sin() + b * (u * theta).sin()) / theta.sin()
}

/// Interpolate two cameras: the focal point moves linearly, the view direction turns on the
/// sphere around it, and the distance changes geometrically (so zooms feel even).
pub fn interpolate_camera(a: &CameraState, b: &CameraState, u: f64) -> CameraState {
    let (ta, tb) = (DVec3::from(a.target), DVec3::from(b.target));
    let (da, db) = (DVec3::from(a.eye) - ta, DVec3::from(b.eye) - tb);
    let target = ta.lerp(tb, u);
    let (la, lb) = (da.length().max(1e-30), db.length().max(1e-30));
    let dist = la * (lb / la).powf(u);
    let dir = slerp(da, db, u);
    let eye = target + dir * dist;
    let up = slerp(DVec3::from(a.up), DVec3::from(b.up), u);
    let up = (up - dir * up.dot(dir)).normalize_or(DVec3::from(a.up));
    let oh = a.ortho_height.max(1e-30) * (b.ortho_height.max(1e-30) / a.ortho_height.max(1e-30)).powf(u);
    CameraState {
        eye: eye.to_array(),
        target: target.to_array(),
        up: up.to_array(),
        fov_y_deg: a.fov_y_deg + (b.fov_y_deg - a.fov_y_deg) * u,
        orthographic: a.orthographic,
        ortho_height: oh,
    }
}

pub fn interpolate_value(a: &ParamValue, b: &ParamValue, u: f64) -> Option<ParamValue> {
    let l = |x: f64, y: f64| x + (y - x) * u;
    Some(match (a, b) {
        (ParamValue::Float(x), ParamValue::Float(y)) => ParamValue::Float(l(*x, *y)),
        (ParamValue::Int(x), ParamValue::Int(y)) => ParamValue::Int(l(*x as f64, *y as f64).round() as i64),
        (ParamValue::Float(x), ParamValue::Int(y)) => ParamValue::Float(l(*x, *y as f64)),
        (ParamValue::Int(x), ParamValue::Float(y)) => ParamValue::Float(l(*x as f64, *y)),
        (ParamValue::Vec3(x), ParamValue::Vec3(y)) => ParamValue::Vec3([l(x[0], y[0]), l(x[1], y[1]), l(x[2], y[2])]),
        _ => return None,
    })
}

/// Whether a parameter value can be keyframed.
pub fn animatable(v: &ParamValue) -> bool {
    match v {
        ParamValue::Float(x) => x.is_finite(),
        ParamValue::Int(_) => true,
        ParamValue::Vec3(x) => x.iter().all(|c| c.is_finite()),
        _ => false,
    }
}

impl ParamTrack {
    pub fn value_at(&self, t: f64) -> Option<ParamValue> {
        let ts: Vec<f64> = self.keys.iter().map(|k| k.t).collect();
        let (i, j, u) = segment(&ts, t)?;
        interpolate_value(&self.keys[i].value, &self.keys[j].value, u)
    }
}

impl Animation {
    pub fn is_empty(&self) -> bool {
        self.camera.is_empty() && self.tracks.iter().all(|t| t.keys.is_empty())
    }

    pub fn camera_at(&self, t: f64) -> Option<CameraState> {
        let ts: Vec<f64> = self.camera.iter().map(|k| k.t).collect();
        let (i, j, u) = segment(&ts, t)?;
        Some(interpolate_camera(&self.camera[i].camera, &self.camera[j].camera, u))
    }

    /// Add (or replace, at the same time) a camera keyframe.
    pub fn key_camera(&mut self, t: f64, camera: CameraState) {
        self.camera.retain(|k| (k.t - t).abs() > 1e-9);
        self.camera.push(CameraKey { t, camera });
        self.camera.sort_by(|a, b| a.t.total_cmp(&b.t));
    }

    /// Add (or replace) a keyframe of `node.key`.
    pub fn key_param(&mut self, node: NodeId, key: &str, t: f64, value: ParamValue) {
        let idx = match self.tracks.iter().position(|tr| tr.node == node && tr.key == key) {
            Some(i) => i,
            None => {
                self.tracks.push(ParamTrack { node, key: key.to_string(), keys: vec![] });
                self.tracks.len() - 1
            }
        };
        let tr = &mut self.tracks[idx];
        tr.keys.retain(|k| (k.t - t).abs() > 1e-9);
        tr.keys.push(ParamKey { t, value });
        tr.keys.sort_by(|a, b| a.t.total_cmp(&b.t));
    }
}

impl Session {
    /// Set a parameter without an undo entry (used while animating).
    pub fn set_param_quiet(&mut self, id: NodeId, key: &str, value: ParamValue) {
        if let Some(n) = self.nodes.get_mut(&id) {
            if n.params.get(key) != Some(&value) {
                n.params.set(key, value);
                self.mark_dirty(id);
            }
        }
    }

    /// Put the pipeline in the state of animation time `t` (0..1). Returns the camera to use.
    pub fn apply_animation(&mut self, t: f64) -> Option<CameraState> {
        let t = t.clamp(0.0, 1.0);
        let anim = self.animation.clone();
        for tr in &anim.tracks {
            if let Some(v) = tr.value_at(t) {
                self.set_param_quiet(tr.node, &tr.key, v);
            }
        }
        if anim.animate_time && self.times.len() > 1 {
            let idx = (t * (self.times.len() - 1) as f64).round() as usize;
            if idx != self.time_index {
                self.set_time_index(idx);
            }
        }
        anim.camera_at(t)
    }

    /// Current values of every animated parameter (to restore after an export).
    pub fn animated_params(&self) -> Vec<(NodeId, String, ParamValue)> {
        self.animation
            .tracks
            .iter()
            .filter_map(|tr| self.nodes.get(&tr.node).and_then(|n| n.params.get(&tr.key)).map(|v| (tr.node, tr.key.clone(), v.clone())))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cam(eye: [f64; 3]) -> CameraState {
        CameraState { eye, target: [0.0; 3], up: [0.0, 0.0, 1.0], fov_y_deg: 30.0, orthographic: false, ortho_height: 1.0 }
    }

    #[test]
    fn camera_turns_on_a_sphere() {
        let mut a = Animation::default();
        a.key_camera(0.0, cam([10.0, 0.0, 0.0]));
        a.key_camera(1.0, cam([0.0, 10.0, 0.0]));
        let m = a.camera_at(0.5).unwrap();
        let e = DVec3::from(m.eye);
        assert!((e.length() - 10.0).abs() < 1e-9, "stays at distance 10: {e}");
        assert!((e.x - e.y).abs() < 1e-9 && e.x > 7.0);
        assert_eq!(a.camera_at(-1.0).unwrap().eye, [10.0, 0.0, 0.0]);
        assert_eq!(a.camera_at(2.0).unwrap().eye, [0.0, 10.0, 0.0]);
    }

    #[test]
    fn distance_is_geometric() {
        let m = interpolate_camera(&cam([1.0, 0.0, 0.0]), &cam([100.0, 0.0, 0.0]), 0.5);
        assert!((m.eye[0] - 10.0).abs() < 1e-9);
    }

    #[test]
    fn param_tracks() {
        let mut a = Animation::default();
        a.key_param(3, "value", 0.0, ParamValue::Float(0.0));
        a.key_param(3, "value", 0.5, ParamValue::Float(10.0));
        a.key_param(3, "value", 1.0, ParamValue::Float(0.0));
        let tr = &a.tracks[0];
        assert_eq!(tr.value_at(0.25), Some(ParamValue::Float(5.0)));
        assert_eq!(tr.value_at(0.75), Some(ParamValue::Float(5.0)));
        a.key_param(3, "value", 0.5, ParamValue::Float(20.0));
        assert_eq!(a.tracks[0].keys.len(), 3, "same time replaces");
        a.key_param(4, "origin", 0.0, ParamValue::Vec3([0.0; 3]));
        a.key_param(4, "origin", 1.0, ParamValue::Vec3([2.0, 4.0, 6.0]));
        assert_eq!(a.tracks[1].value_at(0.5), Some(ParamValue::Vec3([1.0, 2.0, 3.0])));
        let ints = interpolate_value(&ParamValue::Int(0), &ParamValue::Int(10), 0.26);
        assert_eq!(ints, Some(ParamValue::Int(3)));
    }
}
