//! Animation export: step through time (or orbit the camera), capture each rendered frame,
//! and write PNG frames, an animated GIF, or an MP4 (when ffmpeg is installed).

use crate::app::State;
use egui::RichText;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimMode {
    Time,
    Orbit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimFormat {
    Gif,
    Mp4,
    Png,
}

pub struct AnimDialog {
    pub open: bool,
    pub mode: AnimMode,
    pub format: AnimFormat,
    pub fps: f32,
    pub orbit_frames: usize,
}

impl Default for AnimDialog {
    fn default() -> Self {
        Self { open: false, mode: AnimMode::Time, format: AnimFormat::Gif, fps: 10.0, orbit_frames: 72 }
    }
}

/// An export in progress. Frames are captured after the pipeline has settled.
pub struct AnimJob {
    pub out: PathBuf,
    pub dir: PathBuf,
    pub format: AnimFormat,
    pub fps: f32,
    pub mode: AnimMode,
    pub total: usize,
    pub frame: usize,
    /// Frames to wait after a change so the new data is tessellated and rendered.
    pub settle: u32,
    pub start_time_index: usize,
    pub start_camera: Option<aurafx_render::Camera>,
}

pub fn ffmpeg_available() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

impl State {
    pub fn start_animation(&mut self, out: PathBuf, fps: f32, orbit: Option<usize>) {
        let format = match out.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) {
            Some(e) if e == "mp4" => AnimFormat::Mp4,
            Some(e) if e == "png" => AnimFormat::Png,
            _ => AnimFormat::Gif,
        };
        if format == AnimFormat::Mp4 && !ffmpeg_available() {
            self.toast("MP4 needs ffmpeg on the PATH; choose GIF or PNG frames instead", true);
            return;
        }
        let ntimes = self.session.lock().times.len();
        let (mode, total) = match orbit {
            Some(n) => (AnimMode::Orbit, n.max(2)),
            None if ntimes > 1 => (AnimMode::Time, ntimes),
            None => (AnimMode::Orbit, 72),
        };
        let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("frames").to_string();
        let dir = if format == AnimFormat::Png { out.with_file_name(&stem) } else { std::env::temp_dir().join(format!("aurafx-anim-{}", std::process::id())) };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.toast(format!("Cannot create {}: {e}", dir.display()), true);
            return;
        }
        let start_time_index = self.session.lock().time_index;
        if mode == AnimMode::Time {
            self.session.lock().set_time_index(0);
        }
        self.playing = false;
        self.anim_job = Some(AnimJob {
            out,
            dir,
            format,
            fps: fps.max(0.5),
            mode,
            total,
            frame: 0,
            settle: 3,
            start_time_index,
            start_camera: Some(self.viewport.camera.clone()),
        });
        self.status = format!("Recording {total} frames…");
    }

    /// Called after the 3D view rendered this frame.
    pub fn animation_tick(&mut self, ctx: &egui::Context) {
        let Some(job) = self.anim_job.as_mut() else { return };
        ctx.request_repaint();
        if self.session.lock().is_busy() {
            job.settle = 3;
            return;
        }
        if job.settle > 0 {
            job.settle -= 1;
            return;
        }
        let path = job.dir.join(format!("frame_{:05}.png", job.frame));
        if let Err(e) = self.viewport.save_current_png(&path) {
            self.toast(format!("Frame capture failed: {e:#}"), true);
            self.anim_job = None;
            return;
        }
        job.frame += 1;
        if job.frame >= job.total {
            let job = self.anim_job.take().unwrap();
            if job.mode == AnimMode::Time {
                self.session.lock().set_time_index(job.start_time_index);
            }
            if let Some(c) = job.start_camera.clone() {
                self.viewport.camera = c;
            }
            self.finish_animation(job);
            return;
        }
        match job.mode {
            AnimMode::Time => self.session.lock().set_time_index(job.frame),
            AnimMode::Orbit => {
                let step = 1.0 / job.total as f64;
                // `rotate` takes fractions of a half turn.
                self.viewport.camera.rotate(-2.0 * step, 0.0);
            }
        }
        job.settle = 2;
        self.status = format!("Recording frame {}/{}", job.frame + 1, job.total);
    }

    fn finish_animation(&mut self, job: AnimJob) {
        let frames: Vec<PathBuf> = (0..job.total).map(|i| job.dir.join(format!("frame_{i:05}.png"))).collect();
        let out = job.out.clone();
        match job.format {
            AnimFormat::Png => {
                self.toast(format!("Saved {} frames to {}", job.total, job.dir.display()), false);
            }
            AnimFormat::Gif => {
                let tx = self.anim_done_tx.clone();
                let fps = job.fps;
                self.status = "Encoding GIF…".into();
                std::thread::spawn(move || {
                    let r = encode_gif(&frames, &out, fps);
                    let _ = std::fs::remove_dir_all(&job.dir);
                    let _ = tx.send(r.map(|_| out).map_err(|e| format!("{e:#}")));
                });
            }
            AnimFormat::Mp4 => {
                let tx = self.anim_done_tx.clone();
                let fps = job.fps;
                self.status = "Encoding MP4…".into();
                std::thread::spawn(move || {
                    let st = std::process::Command::new("ffmpeg")
                        .args(["-y", "-loglevel", "error", "-framerate", &fps.to_string(), "-i"])
                        .arg(job.dir.join("frame_%05d.png"))
                        .args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p"])
                        .arg(&out)
                        .status();
                    let _ = std::fs::remove_dir_all(&job.dir);
                    let r = match st {
                        Ok(s) if s.success() => Ok(out),
                        Ok(s) => Err(format!("ffmpeg exited with {s}")),
                        Err(e) => Err(format!("ffmpeg: {e}")),
                    };
                    let _ = tx.send(r);
                });
            }
        }
    }

    pub fn animation_poll(&mut self) {
        while let Ok(r) = self.anim_done_rx.try_recv() {
            match r {
                Ok(p) => self.toast(format!("Saved {}", p.display()), false),
                Err(e) => self.toast(format!("Animation export failed: {e}"), true),
            }
        }
        let req = self.session.lock().animation_request.take();
        if let Some((p, fps, orbit)) = req {
            self.start_animation(p, fps, orbit);
        }
    }
}

fn encode_gif(frames: &[PathBuf], out: &std::path::Path, fps: f32) -> anyhow::Result<()> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame};
    let file = std::fs::File::create(out)?;
    let mut enc = GifEncoder::new_with_speed(std::io::BufWriter::new(file), 10);
    enc.set_repeat(Repeat::Infinite)?;
    let delay = Delay::from_numer_denom_ms((1000.0 / fps) as u32, 1);
    for f in frames {
        let mut img = image::open(f)?.into_rgba8();
        // Keep GIFs a reasonable size.
        if img.width() > 960 {
            let h = (img.height() as f64 * 960.0 / img.width() as f64) as u32;
            img = image::imageops::resize(&img, 960, h, image::imageops::FilterType::Triangle);
        }
        enc.encode_frame(Frame::from_parts(img, 0, 0, delay))?;
    }
    Ok(())
}

pub fn dialog_ui(state: &mut State, ctx: &egui::Context) {
    if !state.anim_dialog.open {
        return;
    }
    let mut open = true;
    let mut start: Option<(PathBuf, f32, Option<usize>)> = None;
    let ntimes = state.session.lock().times.len();
    egui::Window::new("Save Animation").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        let d = &mut state.anim_dialog;
        if ntimes < 2 {
            d.mode = AnimMode::Orbit;
        }
        egui::Grid::new("anim").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            ui.label("Animate");
            ui.horizontal(|ui| {
                ui.add_enabled_ui(ntimes > 1, |ui| {
                    ui.radio_value(&mut d.mode, AnimMode::Time, format!("Time steps ({ntimes})"));
                });
                ui.radio_value(&mut d.mode, AnimMode::Orbit, "Orbit camera 360°");
            });
            ui.end_row();
            if d.mode == AnimMode::Orbit {
                ui.label("Frames");
                ui.add(egui::DragValue::new(&mut d.orbit_frames).range(8..=1440));
                ui.end_row();
            }
            ui.label("Frame rate");
            ui.add(egui::DragValue::new(&mut d.fps).range(0.5..=60.0).suffix(" fps"));
            ui.end_row();
            ui.label("Format");
            ui.horizontal(|ui| {
                ui.radio_value(&mut d.format, AnimFormat::Gif, "GIF");
                let ff = ffmpeg_available();
                ui.add_enabled_ui(ff, |ui| {
                    ui.radio_value(&mut d.format, AnimFormat::Mp4, "MP4").on_disabled_hover_text("Install ffmpeg to export MP4");
                });
                ui.radio_value(&mut d.format, AnimFormat::Png, "PNG frames");
            });
            ui.end_row();
        });
        ui.label(RichText::new("Frames are captured at the current 3D view size.").weak().small());
        ui.separator();
        if ui.button(RichText::new("Save…").strong()).clicked() {
            let (ext, label) = match d.format {
                AnimFormat::Gif => ("gif", "Animated GIF"),
                AnimFormat::Mp4 => ("mp4", "MP4 video"),
                AnimFormat::Png => ("png", "PNG frames (a folder is created)"),
            };
            if let Some(p) = rfd::FileDialog::new().add_filter(label, &[ext]).set_file_name(format!("animation.{ext}")).save_file() {
                let p = if p.extension().is_none() { p.with_extension(ext) } else { p };
                let orbit = if d.mode == AnimMode::Orbit { Some(d.orbit_frames) } else { None };
                let fps = d.fps;
                d.open = false;
                start = Some((p, fps, orbit));
            }
        }
    });
    if !open {
        state.anim_dialog.open = false;
    }
    if let Some((p, fps, orbit)) = start {
        state.start_animation(p, fps, orbit);
    }
}
