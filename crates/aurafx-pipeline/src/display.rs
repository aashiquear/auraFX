//! Per-node display properties.

use aurafx_filters::Association;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Representation {
    #[default]
    Surface,
    SurfaceWithEdges,
    Wireframe,
    Points,
    Outline,
    /// GPU ray-cast volume (image data directly; other meshes are resampled).
    Volume,
    /// Surface with a vector field drawn as streaks (line integral convolution).
    SurfaceLic,
    /// Atoms as spheres and bonds as cylinders (molecules: points with atomic numbers, bonds
    /// as lines).
    Molecule,
}

/// How the Molecule representation draws atoms and bonds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MoleculeStyle {
    /// Atoms at a fraction of their van der Waals radius, bonds as thin cylinders.
    #[default]
    BallAndStick,
    /// Atoms at their full van der Waals radius, no bonds.
    VanDerWaals,
    /// Atoms and bonds as tubes of one width.
    Liquorice,
}

impl MoleculeStyle {
    pub const ALL: [MoleculeStyle; 3] = [MoleculeStyle::BallAndStick, MoleculeStyle::VanDerWaals, MoleculeStyle::Liquorice];
    pub fn label(self) -> &'static str {
        match self {
            MoleculeStyle::BallAndStick => "Ball and Stick",
            MoleculeStyle::VanDerWaals => "Van der Waals",
            MoleculeStyle::Liquorice => "Liquorice",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        let l = s.to_ascii_lowercase().replace(['_', '-'], " ");
        Self::ALL.into_iter().find(|m| m.label().to_ascii_lowercase() == l || (l == "vdw" && *m == MoleculeStyle::VanDerWaals))
    }
}

impl Representation {
    pub const ALL: [Representation; 8] = [
        Representation::Surface,
        Representation::SurfaceWithEdges,
        Representation::Wireframe,
        Representation::Points,
        Representation::Outline,
        Representation::Volume,
        Representation::SurfaceLic,
        Representation::Molecule,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Representation::Surface => "Surface",
            Representation::SurfaceWithEdges => "Surface With Edges",
            Representation::Wireframe => "Wireframe",
            Representation::Points => "Points",
            Representation::Outline => "Outline",
            Representation::Volume => "Volume",
            Representation::SurfaceLic => "Surface LIC",
            Representation::Molecule => "Molecule",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        let l = s.to_ascii_lowercase().replace(['_', '-'], " ");
        Self::ALL.into_iter().find(|r| r.label().to_ascii_lowercase() == l)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorBy {
    pub assoc: Association,
    pub name: String,
    /// `None` = magnitude for vectors.
    pub component: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DisplayProps {
    pub representation: Representation,
    /// `None` = solid color.
    pub color_by: Option<ColorBy>,
    pub colormap: String,
    /// `None` = automatic from data.
    pub range: Option<(f64, f64)>,
    pub log_scale: bool,
    /// Arrays of 8-bit unsigned values with 3 or 4 components (RGB, RGBA) are drawn as their own
    /// colors instead of through the color map (when coloring by the whole array).
    pub direct_colors: bool,
    pub solid_color: [f32; 3],
    pub opacity: f32,
    pub point_size: f32,
    pub points_as_spheres: bool,
    pub line_width: f32,
    pub show_scalar_bar: bool,
    pub lighting: bool,
    pub edge_color: [f32; 3],
    /// Volume rendering: overall opacity scale (higher = more opaque).
    pub volume_opacity: f32,
    /// Volume rendering: values below this fraction of the range are fully transparent.
    pub volume_threshold: f32,
    /// Draw a labelled axes grid around the object's bounds.
    pub axes_grid: bool,
    /// Edited color map control points; when set they replace the named preset.
    pub colormap_stops: Option<Vec<(f32, [f32; 3])>>,
    /// Run the color map backwards.
    pub colormap_reverse: bool,
    /// Number of flat color bands (0 = continuous).
    pub colormap_bands: u32,
    /// Curved (higher-order) cells: 0 = straight through the corners, 1 = through the nodes,
    /// each further level halves the pieces (ParaView's "Nonlinear Subdivision Level").
    pub nonlinear_subdivision: u32,
    /// For objects on a server: draw them there (images come back) or here (geometry comes back).
    pub draw_on: DrawOn,
    /// Surface LIC: the vector array (None: the first 3-component point array).
    pub lic_vectors: Option<String>,
    /// Surface LIC: how strongly the streaks darken the surface (0..1).
    pub lic_strength: f32,
    /// Surface LIC: streak half-length in pixels.
    pub lic_length: u32,
    /// Material: 0 = plastic-like, 1 = metal (colored reflections, no diffuse light).
    pub metallic: f32,
    /// Material: 0 = mirror-like highlights and reflections, 1 = matte.
    pub roughness: f32,
    /// Material: strength of highlights on non-metals.
    pub specular: f32,
    /// Image file (PNG, JPEG) shown on the surface through its texture coordinates.
    pub texture: Option<String>,
    /// Molecule representation: style, atom size factor (of the van der Waals radius, ball and
    /// stick), bond radius (Å) and bond orders drawn as parallel cylinders.
    pub molecule_style: MoleculeStyle,
    pub atom_scale: f32,
    pub bond_radius: f32,
    pub bond_orders: bool,
}

/// Roughness whose highlights match the classic shading (Blinn-Phong power 60).
pub const DEFAULT_ROUGHNESS: f32 = 0.4238;

/// Blinn-Phong highlight power for a roughness.
pub fn specular_power(roughness: f32) -> f32 {
    2.0 / roughness.clamp(0.02, 1.0).powi(4) - 2.0
}

/// Where an object on a server is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum DrawOn {
    /// Here, unless even its surface is too big to transfer.
    #[default]
    Auto,
    /// Always here (its surface comes over, however big).
    Local,
    /// Always on the server (only images come over).
    Server,
}

impl Default for DisplayProps {
    fn default() -> Self {
        Self {
            representation: Representation::Surface,
            color_by: None,
            colormap: "Viridis".into(),
            range: None,
            log_scale: false,
            direct_colors: true,
            solid_color: [0.85, 0.85, 0.85],
            opacity: 1.0,
            point_size: 4.0,
            points_as_spheres: true,
            line_width: 1.0,
            show_scalar_bar: true,
            lighting: true,
            edge_color: [0.0, 0.0, 0.0],
            volume_opacity: 1.0,
            volume_threshold: 0.0,
            axes_grid: false,
            colormap_stops: None,
            colormap_reverse: false,
            colormap_bands: 0,
            nonlinear_subdivision: 1,
            draw_on: DrawOn::Auto,
            lic_vectors: None,
            texture: None,
            molecule_style: MoleculeStyle::BallAndStick,
            atom_scale: 0.3,
            bond_radius: 0.075,
            bond_orders: true,
            lic_strength: 0.8,
            lic_length: 20,
            metallic: 0.0,
            roughness: DEFAULT_ROUGHNESS,
            specular: 0.12,
        }
    }
}

impl DisplayProps {
    /// The color map actually used: the preset or edited stops, reversed and banded as asked.
    pub fn effective_colormap(&self) -> crate::colormap::ColorMap {
        let mut m = match &self.colormap_stops {
            Some(stops) if stops.len() >= 2 => crate::colormap::ColorMap::new(self.colormap.clone(), stops.clone()),
            _ => crate::colormap::get(&self.colormap),
        };
        if self.colormap_reverse {
            m = m.reversed();
        }
        if self.colormap_bands >= 2 {
            m = m.banded(self.colormap_bands);
        }
        m
    }

    /// Changes whenever the effective color map does (for caching lookup textures).
    pub fn colormap_key(&self) -> String {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (t, c) in self.effective_colormap().stops {
            t.to_bits().hash(&mut h);
            c.map(|x| x.to_bits()).hash(&mut h);
        }
        format!("{}#{:x}", self.colormap, h.finish())
    }
}

/// A directional light of the 3D views.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    /// Direction towards the light. With `camera_relative`, in the camera's frame (x right,
    /// y up, z towards the viewer), so the light moves with the view; else in world coordinates.
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub camera_relative: bool,
    /// Casts shadows (when shadows are on; the first such light is used).
    pub shadows: bool,
}

impl Default for Light {
    fn default() -> Self {
        Self { direction: [0.3, 0.5, 1.0], color: [1.0, 1.0, 1.0], intensity: 0.6, camera_relative: true, shadows: true }
    }
}

impl Light {
    /// ParaView-like light kit: key light (upper right), fill (lower left), head light.
    pub fn kit() -> Vec<Light> {
        vec![
            Light::default(),
            Light { direction: [-0.6, -0.3, 1.0], intensity: 0.25, shadows: false, ..Default::default() },
            Light { direction: [0.0, 0.0, 1.0], intensity: 0.2, shadows: false, ..Default::default() },
        ]
    }

    /// The single light of earlier versions.
    pub fn single() -> Vec<Light> {
        vec![Light { intensity: 0.7, ..Default::default() }]
    }
}

/// Tone mapping of the final picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneMapping {
    #[default]
    None,
    Reinhard,
    Aces,
}

/// Where the ray tracer runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RayDevice {
    /// The GPU when the scene fits in its buffers, else the CPU.
    #[default]
    Auto,
    Gpu,
    Cpu,
}

/// Lighting and screen effects of the 3D views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lighting {
    /// Key, fill and head lights (else one light). Ignored when `lights` is set.
    pub light_kit: bool,
    /// Custom lights (empty: the light kit or the single light).
    pub lights: Vec<Light>,
    /// Ambient light (None: 0.15 with several lights, 0.3 with one).
    pub ambient: Option<f32>,
    /// The first light with `shadows` casts shadows.
    pub shadows: bool,
    /// Eye-Dome Lighting: shade by depth differences (point clouds, dense meshes).
    pub edl: bool,
    pub edl_strength: f32,
    /// Screen-space ambient occlusion: darken creases and cavities.
    pub ssao: bool,
    pub ssao_strength: f32,
    /// Occlusion radius as a fraction of the size of the visible objects.
    pub ssao_radius: f32,
    /// Fast approximate anti-aliasing of the final picture.
    pub fxaa: bool,
    pub tone_mapping: ToneMapping,
    pub exposure: f32,
    /// Path-trace opaque surfaces (refines while the view is still).
    pub ray_tracing: bool,
    /// Samples per pixel the ray tracer stops at.
    pub ray_samples: u32,
    /// Diffuse bounces after the first hit.
    pub ray_bounces: u32,
    /// Where to path-trace.
    pub ray_device: RayDevice,
    /// Environment map lighting the scene: "" (none), "sky", "studio", "white" or an image file
    /// (.hdr, .png, .jpg; equirectangular, +Z up). It replaces the ambient light and adds
    /// reflections.
    pub environment: String,
    pub environment_intensity: f32,
    /// Show the environment behind the objects instead of the background color.
    pub show_environment: bool,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            light_kit: true,
            lights: Vec::new(),
            ambient: None,
            shadows: false,
            edl: false,
            edl_strength: 1.0,
            ssao: false,
            ssao_strength: 1.0,
            ssao_radius: 0.06,
            fxaa: false,
            tone_mapping: ToneMapping::None,
            exposure: 1.0,
            ray_tracing: false,
            ray_samples: 128,
            ray_bounces: 3,
            ray_device: RayDevice::Auto,
            environment: String::new(),
            environment_intensity: 1.0,
            show_environment: false,
        }
    }
}

impl Lighting {
    /// The lights in effect and the ambient term. With an environment (which lights the
    /// scene too) the built-in lights are dimmed by half; custom lights stay as set.
    pub fn effective_lights(&self) -> (Vec<Light>, f32) {
        let mut lights = if !self.lights.is_empty() {
            self.lights.clone()
        } else if self.light_kit {
            Light::kit()
        } else {
            Light::single()
        };
        if self.lights.is_empty() && !self.environment.trim().is_empty() {
            for l in &mut lights {
                l.intensity *= 0.5;
            }
        }
        let ambient = self.ambient.unwrap_or(if lights.len() > 1 { 0.15 } else { 0.3 });
        (lights, ambient)
    }

    /// What a server applies to the pictures it draws: lights and shadows only (the screen
    /// effects run here, over the composited picture).
    pub fn for_server(&self) -> Lighting {
        Lighting {
            light_kit: self.light_kit,
            lights: self.lights.clone(),
            ambient: self.ambient,
            shadows: self.shadows,
            environment: self.environment.clone(),
            environment_intensity: self.environment_intensity,
            show_environment: false,
            ..Lighting { edl: false, ssao: false, fxaa: false, tone_mapping: ToneMapping::None, exposure: 1.0, ray_tracing: false, ..Default::default() }
        }
    }
}
