//! Drawing on a server: the render hook `aurafx-server` plugs into the remote protocol.

use crate::{headless, headless_with_samples, tessellate::tessellate, Camera, MeshData, NodeDraw, RenderSettings, Renderer};
use anyhow::Result;
use aurafx_data::DataSet;
use aurafx_pipeline::remote::{RenderHook, RenderJob};
use aurafx_pipeline::{DisplayProps, NodeId};
use glam::DVec3;
use std::collections::HashMap;
use std::sync::Arc;

/// Draws server-side outputs with a headless GPU device.
pub struct ServerRenderer {
    renderer: Renderer,
    /// Tessellated outputs: (output pointer, display key, mesh).
    meshes: HashMap<NodeId, (usize, String, Arc<MeshData>)>,
}

impl ServerRenderer {
    /// A renderer on this machine's GPU, or None when there is none.
    pub fn new() -> Option<Self> {
        let renderer = headless(16, 16).ok()?;
        Some(Self { renderer, meshes: HashMap::new() })
    }

    /// For a worker of a parallel server: no multisampling, so the pictures of neighbouring
    /// pieces fit together without seams (the coordinator supersamples instead).
    pub fn for_pieces() -> Option<Self> {
        let renderer = headless_with_samples(16, 16, 1).ok()?;
        Some(Self { renderer, meshes: HashMap::new() })
    }

    pub fn gpu(&self) -> String {
        self.renderer.adapter_info.clone()
    }
}

pub fn camera_of(c: &aurafx_pipeline::remote::RenderCamera) -> Camera {
    Camera {
        eye: DVec3::from_array(c.eye),
        target: DVec3::from_array(c.target),
        up: DVec3::from_array(c.up),
        fov_y_deg: c.fov_y_deg,
        orthographic: c.orthographic,
        ortho_height: c.ortho_height,
        near: c.near,
        far: c.far,
    }
}

pub fn render_camera(c: &Camera) -> aurafx_pipeline::remote::RenderCamera {
    aurafx_pipeline::remote::RenderCamera {
        eye: c.eye.to_array(),
        target: c.target.to_array(),
        up: c.up.to_array(),
        fov_y_deg: c.fov_y_deg,
        orthographic: c.orthographic,
        ortho_height: c.ortho_height,
        near: c.near,
        far: c.far,
    }
}

impl RenderHook for ServerRenderer {
    fn render(&mut self, outputs: &[(NodeId, Arc<DataSet>, DisplayProps)], job: &RenderJob) -> Result<(Vec<u8>, Vec<f32>)> {
        self.renderer.resize(job.width.max(1), job.height.max(1));
        let mut draws = Vec::with_capacity(outputs.len());
        for (id, data, display) in outputs {
            let ptr = Arc::as_ptr(data) as usize;
            let key = format!("{display:?}");
            let mesh = match self.meshes.get(id).filter(|(p, k, _)| *p == ptr && *k == key) {
                Some((_, _, m)) => m.clone(),
                None => {
                    let m = Arc::new(tessellate(data, display));
                    self.meshes.insert(*id, (ptr, key, m.clone()));
                    m
                }
            };
            draws.push(NodeDraw { id: *id, mesh, display: display.clone(), overlay: false });
        }
        let live: Vec<NodeId> = outputs.iter().map(|o| o.0).collect();
        self.meshes.retain(|id, _| live.contains(id));
        self.renderer.render(&camera_of(&job.camera), &draws, &RenderSettings { lighting: job.lighting.for_server(), ..Default::default() });
        let (_, _, color) = self.renderer.read_pixels()?;
        let depth = self.renderer.read_depth()?;
        Ok((color, depth))
    }
}
