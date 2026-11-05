//! View › Lighting: lights, shadows and screen effects of the 3D views.

use aurafx_pipeline::{Light, Lighting, ToneMapping};

/// Azimuth and elevation (degrees) of a light direction. Camera-relative lights: azimuth
/// around the view's up axis (0 = towards the viewer), elevation above the view's horizontal.
/// Fixed lights: azimuth around +Z (0 = +X), elevation above the XY plane.
fn angles(l: &Light) -> (f32, f32) {
    let d = l.direction;
    let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt().max(1e-9);
    let (x, y, z) = (d[0] / n, d[1] / n, d[2] / n);
    if l.camera_relative {
        (x.atan2(z).to_degrees(), y.clamp(-1.0, 1.0).asin().to_degrees())
    } else {
        (y.atan2(x).to_degrees(), z.clamp(-1.0, 1.0).asin().to_degrees())
    }
}

fn set_angles(l: &mut Light, az: f32, el: f32) {
    let (a, e) = (az.to_radians(), el.to_radians());
    l.direction = if l.camera_relative { [e.cos() * a.sin(), e.sin(), e.cos() * a.cos()] } else { [e.cos() * a.cos(), e.cos() * a.sin(), e.sin()] };
}

/// The controls; returns true when something changed.
pub fn lighting_ui(ui: &mut egui::Ui, l: &mut Lighting) -> bool {
    let before = l.clone();
    ui.set_min_width(300.0);
    let mut custom = !l.lights.is_empty();
    if ui.checkbox(&mut custom, "Custom lights").on_hover_text("Edit the lights (off: the light kit or one head light)").changed() {
        l.lights = if custom {
            if l.light_kit {
                Light::kit()
            } else {
                Light::single()
            }
        } else {
            Vec::new()
        };
    }
    if !custom {
        ui.checkbox(&mut l.light_kit, "Light kit").on_hover_text("Key, fill and head lights (off: one light)");
    } else {
        let mut remove = None;
        let count = l.lights.len();
        for (k, light) in l.lights.iter_mut().enumerate() {
            ui.push_id(k, |ui| {
                ui.horizontal(|ui| {
                    ui.label(format!("Light {}", k + 1));
                    let mut rgb = light.color;
                    if ui.color_edit_button_rgb(&mut rgb).changed() {
                        light.color = rgb;
                    }
                    ui.add(egui::DragValue::new(&mut light.intensity).range(0.0..=3.0).speed(0.01)).on_hover_text("Intensity");
                    if count > 1 && ui.small_button(crate::icons::X).on_hover_text("Remove").clicked() {
                        remove = Some(k);
                    }
                });
                ui.horizontal(|ui| {
                    let (mut az, mut el) = angles(light);
                    let a = ui.add(egui::DragValue::new(&mut az).range(-180.0..=180.0).suffix("°").speed(1.0)).on_hover_text("Azimuth");
                    let e = ui.add(egui::DragValue::new(&mut el).range(-90.0..=90.0).suffix("°").speed(1.0)).on_hover_text("Elevation");
                    if a.changed() || e.changed() {
                        set_angles(light, az, el);
                    }
                    let mut rel = light.camera_relative;
                    if ui.checkbox(&mut rel, "follows camera").on_hover_text("Moves with the view (off: fixed in the scene)").changed() {
                        // Keep the same angles in the new frame.
                        light.camera_relative = rel;
                        set_angles(light, az, el);
                    }
                    ui.checkbox(&mut light.shadows, "shadows");
                });
            });
        }
        if let Some(k) = remove {
            l.lights.remove(k);
        }
        ui.horizontal(|ui| {
            if ui.button(format!("{} Add light", crate::icons::PLUS)).clicked() {
                l.lights.push(Light { intensity: 0.4, shadows: false, ..Default::default() });
            }
            let mut amb = l.ambient.unwrap_or(if l.lights.len() > 1 { 0.15 } else { 0.3 });
            ui.label("ambient");
            if ui.add(egui::DragValue::new(&mut amb).range(0.0..=1.0).speed(0.01)).changed() {
                l.ambient = Some(amb);
            }
        });
    }
    ui.separator();
    ui.checkbox(&mut l.shadows, "Shadows").on_hover_text("Shadows from the key light (custom lights: the first one with shadows ticked)");
    ui.horizontal(|ui| {
        ui.checkbox(&mut l.ssao, "Ambient occlusion").on_hover_text("Darken creases, cavities and contact areas (screen-space)");
        if l.ssao {
            ui.add(egui::Slider::new(&mut l.ssao_strength, 0.2..=3.0).show_value(false)).on_hover_text("Strength");
            ui.add(egui::Slider::new(&mut l.ssao_radius, 0.005..=0.2).logarithmic(true).show_value(false)).on_hover_text("Radius");
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut l.edl, "Eye-Dome Lighting")
            .on_hover_text("Shade by depth differences: outlines and depth cues for point clouds, particles and dense meshes");
        if l.edl {
            ui.add(egui::Slider::new(&mut l.edl_strength, 0.1..=4.0).logarithmic(true).show_value(false)).on_hover_text("Strength");
        }
    });
    ui.checkbox(&mut l.fxaa, "FXAA").on_hover_text("Smooth remaining jagged edges (thin lines, textures)");
    ui.horizontal(|ui| {
        ui.label("Tone mapping");
        egui::ComboBox::from_id_salt("tone mapping")
            .selected_text(match l.tone_mapping {
                ToneMapping::None => "none",
                ToneMapping::Reinhard => "Reinhard",
                ToneMapping::Aces => "ACES filmic",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut l.tone_mapping, ToneMapping::None, "none");
                ui.selectable_value(&mut l.tone_mapping, ToneMapping::Reinhard, "Reinhard");
                ui.selectable_value(&mut l.tone_mapping, ToneMapping::Aces, "ACES filmic");
            });
        ui.add(egui::DragValue::new(&mut l.exposure).range(0.1..=4.0).speed(0.01).prefix("exposure ")).on_hover_text("Brightness before tone mapping");
    });
    if ui.button("Reset lighting").clicked() {
        *l = Lighting::default();
    }
    *l != before
}

/// Apply `afx.lighting(...)` options (a JSON object of `Lighting` fields) to `l`.
pub fn apply_json(l: &mut Lighting, json: &str) -> anyhow::Result<()> {
    let patch: serde_json::Value = serde_json::from_str(json)?;
    let mut v = serde_json::to_value(&*l)?;
    let (Some(obj), Some(p)) = (v.as_object_mut(), patch.as_object()) else { anyhow::bail!("expected an object") };
    for (k, val) in p {
        anyhow::ensure!(obj.contains_key(k), "unknown lighting option {k:?}");
        obj.insert(k.clone(), val.clone());
    }
    *l = serde_json::from_value(v)?;
    Ok(())
}
