//! Animation export: step through time (or orbit the camera), capture each rendered frame,
//! and write PNG frames, an animated GIF, or an MP4 (when ffmpeg is installed).

use crate::app::State;
use crate::touch::FitScreen;
use egui::RichText;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimMode {
    Time,
    Orbit,
    /// Camera keyframes and parameter tracks (and optionally time steps).
    Keyframes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimFormat {
    Gif,
    Mp4,
    Webm,
    Png,
}

pub struct AnimDialog {
    pub open: bool,
    pub mode: AnimMode,
    pub format: AnimFormat,
    pub fps: f32,
    pub orbit_frames: usize,
    /// Animation time (0..1) shown in the keyframe editor.
    pub t: f64,
    /// Live preview: (start instant, t at start).
    pub preview: Option<(std::time::Instant, f64)>,
    /// Parameter chosen for a new track.
    pub track_key: String,
    /// What is recorded: 0 the active 3D view, 1 every view as laid out, 2 the chart views,
    /// 3 every 3D view, 4 one 3D view by number.
    pub views: u8,
    /// The 3D view of choice 4 (1 = the first).
    pub view_no: u32,
    /// The frames' size, shape and background.
    pub options: crate::picture_options::PictureOptions,
}

impl Default for AnimDialog {
    fn default() -> Self {
        Self {
            open: false,
            mode: AnimMode::Time,
            format: AnimFormat::Gif,
            fps: 10.0,
            orbit_frames: 72,
            t: 0.0,
            preview: None,
            track_key: String::new(),
            views: 0,
            view_no: 1,
            options: Default::default(),
        }
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
    /// Animated parameter values before the export (restored afterwards).
    pub start_params: Vec<(aurafx_pipeline::NodeId, String, aurafx_filters::params::ParamValue)>,
    /// Record these views as laid out (3D views and charts) instead of the active 3D view.
    pub layout: Option<crate::capture::Views>,
    /// A frame of the layout is being captured.
    pub capturing: bool,
    /// The active 3D view drawn on its own at a size or with a background other than the screen's.
    pub picture: Option<(Option<(u32, u32)>, Option<aurafx_pipeline::session::PictureBackground>)>,
    /// The frame being drawn that way (its result is in the session's `screenshot_results`).
    pub frame_id: u64,
}

fn runs(program: &std::path::Path) -> bool {
    let mut c = std::process::Command::new(program);
    c.arg("-version").stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(0x0800_0000); // no console window
    }
    c.status().map(|s| s.success()).unwrap_or(false)
}

/// Where auraFX keeps programs it downloaded for the user (the video encoder).
pub fn tools_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("AURAFX_DATA_DIR") {
        return Some(PathBuf::from(d).join("tools"));
    }
    directories::ProjectDirs::from("org", "aurafx", "auraFX").map(|d| d.data_dir().join("tools"))
}

fn own_ffmpeg() -> Option<PathBuf> {
    tools_dir().map(|d| d.join(if cfg!(windows) { "ffmpeg.exe" } else { "ffmpeg" }))
}

/// The video encoder: `ffmpeg` on the PATH, else the copy auraFX downloaded. The answer is kept
/// for a few seconds (the Animation window asks every frame).
pub fn ffmpeg_path() -> Option<PathBuf> {
    static CACHE: std::sync::Mutex<Option<(std::time::Instant, Option<PathBuf>)>> = std::sync::Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, p)) = c.as_ref() {
        if at.elapsed() < std::time::Duration::from_secs(3) {
            return p.clone();
        }
    }
    let found = if runs(std::path::Path::new("ffmpeg")) { Some(PathBuf::from("ffmpeg")) } else { own_ffmpeg().filter(|p| p.exists() && runs(p)) };
    *c = Some((std::time::Instant::now(), found.clone()));
    found
}

pub fn ffmpeg_available() -> bool {
    ffmpeg_path().is_some()
}

/// The address of a ready-made encoder for this machine, or None when there is none.
pub fn ffmpeg_download_url() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-x64.gz"),
        ("linux", "aarch64") => Some("https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-linux-arm64.gz"),
        ("macos", "aarch64") => Some("https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-arm64.gz"),
        ("macos", "x86_64") => Some("https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-darwin-x64.gz"),
        ("windows", "x86_64") => Some("https://github.com/eugeneware/ffmpeg-static/releases/download/b6.1.1/ffmpeg-win32-x64.gz"),
        _ => None,
    }
}

/// The state of the encoder download: None when idle, else megabytes so far or the outcome.
pub enum FfmpegDownload {
    Running(f32),
    Failed(String),
}
static FFMPEG_DOWNLOAD: std::sync::Mutex<Option<FfmpegDownload>> = std::sync::Mutex::new(None);

pub fn ffmpeg_download_state<R>(f: impl FnOnce(&Option<FfmpegDownload>) -> R) -> R {
    f(&FFMPEG_DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Download the encoder into the tools folder and check it runs (blocking; see
/// `start_ffmpeg_download` for the window).
pub fn install_ffmpeg() -> Result<PathBuf, String> {
    if let Some(p) = ffmpeg_path() {
        return Ok(p);
    }
    let url = ffmpeg_download_url().ok_or("there is no ready-made ffmpeg for this kind of machine; install ffmpeg and put it on the PATH")?;
    let dest = own_ffmpeg().ok_or("no data folder to keep the encoder in")?;
    let set = |s: Option<FfmpegDownload>| *FFMPEG_DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()) = s;
    set(Some(FfmpegDownload::Running(0.0)));
    let r = aurafx_ai::fetch_to_file(url, &dest, &|done, _| set(Some(FfmpegDownload::Running(done as f32 / 1.0e6))))
        .map_err(|e| format!("downloading the video encoder failed: {e:#}"))
        .and_then(|_| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
            }
            if runs(&dest) {
                Ok(dest.clone())
            } else {
                let _ = std::fs::remove_file(&dest);
                Err("the downloaded encoder does not run on this machine; install ffmpeg and put it on the PATH".to_string())
            }
        });
    set(r.as_ref().err().map(|e| FfmpegDownload::Failed(e.clone())));
    r
}

/// Start the download in the background (the Animation window shows its progress).
pub fn start_ffmpeg_download(ctx: egui::Context) {
    if ffmpeg_download_state(|s| matches!(s, Some(FfmpegDownload::Running(_)))) {
        return;
    }
    *FFMPEG_DOWNLOAD.lock().unwrap_or_else(|e| e.into_inner()) = Some(FfmpegDownload::Running(0.0));
    std::thread::spawn(move || {
        let _ = install_ffmpeg();
        ctx.request_repaint();
    });
}

impl State {
    /// Show animation time `t` (0..1) of the keyframe animation.
    pub fn apply_animation_time(&mut self, t: f64) {
        let mut s = self.session.lock();
        if let Some(c) = s.apply_animation(t) {
            s.pending_camera = Some(c);
        }
    }

    pub fn start_animation(&mut self, out: PathBuf, fps: f32, orbit: Option<usize>) {
        let keyframed = !self.session.lock().animation.is_empty();
        let mode = match orbit {
            Some(_) => AnimMode::Orbit,
            None if keyframed => AnimMode::Keyframes,
            None => AnimMode::Time,
        };
        let views = self.session.lock().animation_views.take();
        let layout = match views.as_deref().map(crate::capture::Views::parse) {
            None => None,
            Some(Some(v)) => Some(v),
            Some(None) => {
                self.toast("save_animation: views must be 'all', 'charts', '3d', 'chart N' or 'view N'", true);
                return;
            }
        };
        let picture = self.session.lock().animation_picture.take();
        self.start_animation_mode(out, fps, mode, orbit.unwrap_or(72), layout, picture);
    }

    pub fn start_animation_mode(
        &mut self,
        out: PathBuf,
        fps: f32,
        mode: AnimMode,
        orbit_frames: usize,
        layout: Option<crate::capture::Views>,
        picture: Option<(Option<(u32, u32)>, Option<aurafx_pipeline::session::PictureBackground>)>,
    ) {
        let format = match out.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()) {
            Some(e) if e == "mp4" => AnimFormat::Mp4,
            Some(e) if e == "webm" => AnimFormat::Webm,
            Some(e) if e == "png" => AnimFormat::Png,
            _ => AnimFormat::Gif,
        };
        if matches!(format, AnimFormat::Mp4 | AnimFormat::Webm) && !ffmpeg_available() {
            self.toast("MP4 and WebM need the video encoder (ffmpeg): File > Animation offers to download it, or afx.install_ffmpeg(); GIF and PNG frames need nothing", true);
            return;
        }
        let ntimes = self.session.lock().times.len();
        let kf_frames = self.session.lock().animation.frames.max(2);
        let (mode, total) = match mode {
            AnimMode::Orbit => (AnimMode::Orbit, orbit_frames.max(2)),
            AnimMode::Keyframes => (AnimMode::Keyframes, kf_frames),
            AnimMode::Time if ntimes > 1 => (AnimMode::Time, ntimes),
            AnimMode::Time => (AnimMode::Orbit, 72),
        };
        let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("frames").to_string();
        let dir = if format == AnimFormat::Png { out.with_file_name(&stem) } else { std::env::temp_dir().join(format!("aurafx-anim-{}", std::process::id())) };
        if let Err(e) = std::fs::create_dir_all(&dir) {
            self.toast(format!("Cannot create {}: {e}", dir.display()), true);
            return;
        }
        let start_time_index = self.session.lock().time_index;
        let start_params = self.session.lock().animated_params();
        let start_camera = Some(self.viewport.camera.clone());
        self.anim_dialog.preview = None;
        match mode {
            AnimMode::Time => self.session.lock().set_time_index(0),
            AnimMode::Keyframes => self.apply_animation_time(0.0),
            AnimMode::Orbit => {}
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
            start_camera,
            start_params,
            picture: if layout.is_some() { None } else { picture.filter(|p| p.0.is_some() || p.1.is_some()) },
            layout,
            capturing: false,
            frame_id: 0,
        });
        self.status = format!("Recording {total} frames…");
    }

    /// Called after the 3D view rendered this frame.
    pub fn animation_tick(&mut self, ctx: &egui::Context) {
        // Live preview of the keyframe animation (no recording).
        if let Some((start, t0)) = self.anim_dialog.preview {
            let frames = self.session.lock().animation.frames.max(2) as f64;
            let t = t0 + start.elapsed().as_secs_f64() * self.anim_dialog.fps as f64 / (frames - 1.0);
            let t = if t >= 1.0 {
                self.anim_dialog.preview = None;
                1.0
            } else {
                t
            };
            self.anim_dialog.t = t;
            self.apply_animation_time(t);
            ctx.request_repaint();
        }
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
        if let Some(views) = job.layout.clone() {
            // Several views: a picture of the window's frame, taken by `capture`, then the next frame.
            if !job.capturing {
                job.capturing = true;
                self.capture.animation_result = None;
                self.capture.request(crate::capture::Request {
                    path,
                    views,
                    transparent: false,
                    controls: false,
                    done: crate::capture::Done::Animation,
                    background: self.anim_dialog.options.gap_color(),
                });
                return;
            }
            match self.capture.animation_result.take() {
                None => return,
                Some(Err(e)) => {
                    self.toast(format!("Frame capture failed: {e}"), true);
                    self.anim_job = None;
                    return;
                }
                Some(Ok(())) => {
                    let job = self.anim_job.as_mut().unwrap();
                    job.capturing = false;
                }
            }
        } else if let Some((size, background)) = job.picture {
            // Drawn once more at the asked size or background, through the screenshot path.
            if !job.capturing {
                let id = {
                    let mut s = self.session.lock();
                    s.next_screenshot_id += 1;
                    s.next_screenshot_id
                };
                job.capturing = true;
                job.frame_id = id;
                self.viewport.request_screenshot(aurafx_pipeline::session::ScreenshotRequest {
                    size,
                    id,
                    background,
                    quiet: true,
                    ..aurafx_pipeline::session::ScreenshotRequest::new(path)
                });
                return;
            }
            let id = job.frame_id;
            let got = {
                let mut s = self.session.lock();
                s.screenshot_results.iter().position(|r| r.0 == id).map(|i| s.screenshot_results.remove(i).1)
            };
            match got {
                None => return,
                Some(Err(e)) => {
                    self.toast(format!("Frame capture failed: {e}"), true);
                    self.anim_job = None;
                    return;
                }
                Some(Ok(())) => {
                    let job = self.anim_job.as_mut().unwrap();
                    job.capturing = false;
                }
            }
        } else if let Err(e) = self.viewport.save_current_png(&path) {
            self.toast(format!("Frame capture failed: {e:#}"), true);
            self.anim_job = None;
            return;
        }
        let job = self.anim_job.as_mut().unwrap();
        job.frame += 1;
        if job.frame >= job.total {
            let job = self.anim_job.take().unwrap();
            if job.mode != AnimMode::Orbit {
                let mut s = self.session.lock();
                s.set_time_index(job.start_time_index);
                s.restore_animated(&job.start_params);
            }
            if let Some(c) = job.start_camera.clone() {
                self.viewport.camera = c;
            }
            self.finish_animation(job);
            return;
        }
        match job.mode {
            AnimMode::Time => self.session.lock().set_time_index(job.frame),
            AnimMode::Keyframes => {
                let t = job.frame as f64 / (job.total - 1) as f64;
                let cam = self.session.lock().apply_animation(t);
                if let Some(c) = cam {
                    let vc = &mut self.viewport.camera;
                    vc.eye = glam::DVec3::from_array(c.eye);
                    vc.target = glam::DVec3::from_array(c.target);
                    vc.up = glam::DVec3::from_array(c.up);
                    vc.fov_y_deg = c.fov_y_deg;
                    vc.orthographic = c.orthographic;
                    vc.ortho_height = c.ortho_height;
                }
            }
            AnimMode::Orbit => {
                let step = 1.0 / job.total as f64;
                // `rotate` takes fractions of a half turn.
                self.viewport.camera.rotate(-2.0 * step, 0.0);
            }
        }
        job.settle = 2;
        self.status = format!("Recording frame {}/{}", job.frame + 1, job.total);
    }

    /// Encode frames a script rendered (afx.save_animation with a cue) and report back.
    fn encode_for_script(&mut self, req: aurafx_pipeline::session::EncodeRequest) {
        let session = self.session.clone();
        self.status = "Encoding the animation…".into();
        let poster = self.session.lock().animation_poster.take();
        std::thread::spawn(move || {
            let r = encode_frames(&req.dir, req.frames, &req.out, req.fps).and_then(|_| write_poster(&req.dir, poster.as_deref()));
            let _ = std::fs::remove_dir_all(&req.dir);
            session.lock().screenshot_results.push((req.id, r));
        });
    }

    fn finish_animation(&mut self, job: AnimJob) {
        let out = job.out.clone();
        match job.format {
            AnimFormat::Png => {
                self.toast(format!("Saved {} frames to {}", job.total, job.dir.display()), false);
            }
            AnimFormat::Gif | AnimFormat::Mp4 | AnimFormat::Webm => {
                let tx = self.anim_done_tx.clone();
                let fps = job.fps;
                self.status = format!(
                    "Encoding {}…",
                    match job.format {
                        AnimFormat::Mp4 => "MP4",
                        AnimFormat::Webm => "WebM",
                        _ => "GIF",
                    }
                );
                let poster = self.session.lock().animation_poster.take();
                std::thread::spawn(move || {
                    let r = encode_frames(&job.dir, job.total, &out, fps).and_then(|_| write_poster(&job.dir, poster.as_deref()));
                    let _ = std::fs::remove_dir_all(&job.dir);
                    let _ = tx.send(r.map(|_| out));
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
        let enc = self.session.lock().encode_request.take();
        if let Some(r) = enc {
            self.encode_for_script(r);
        }
        let tool = self.session.lock().ffmpeg_install_request.take();
        if let Some(id) = tool {
            let session = self.session.clone();
            self.status = "Downloading the video encoder…".into();
            std::thread::spawn(move || {
                let r = install_ffmpeg().map(|_| ());
                session.lock().screenshot_results.push((id, r));
            });
        }
    }
}

/// The first frame of `dir` as a picture (the still shown before a video plays); the format
/// follows the extension.
fn write_poster(dir: &std::path::Path, poster: Option<&std::path::Path>) -> Result<(), String> {
    let Some(poster) = poster else { return Ok(()) };
    let img = image::open(dir.join("frame_00000.png")).map_err(|e| format!("poster: {e}"))?;
    let jpeg = matches!(poster.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("jpg" | "jpeg"));
    let r = if jpeg { img.to_rgb8().save(poster) } else { img.save(poster) };
    r.map_err(|e| format!("poster {}: {e}", poster.display()))
}

/// The ffmpeg arguments that encode a video of this extension (MP4: H.264; WebM: VP9).
pub fn video_codec_args(ext: &str) -> Option<Vec<&'static str>> {
    match ext.to_ascii_lowercase().as_str() {
        "mp4" => Some(vec!["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-pix_fmt", "yuv420p"]),
        "webm" => Some(vec!["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libvpx-vp9", "-b:v", "0", "-crf", "32", "-pix_fmt", "yuv420p"]),
        _ => None,
    }
}

/// Encode `dir/frame_00000.png`... into `out` (GIF, or MP4 or WebM with ffmpeg).
pub fn encode_frames(dir: &std::path::Path, total: usize, out: &std::path::Path, fps: f32) -> Result<(), String> {
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
    let Some(codec) = video_codec_args(&ext) else {
        let frames: Vec<PathBuf> = (0..total).map(|i| dir.join(format!("frame_{i:05}.png"))).collect();
        return encode_gif(&frames, out, fps).map_err(|e| format!("{e:#}"));
    };
    let Some(ffmpeg) = ffmpeg_path() else {
        return Err(format!(
            "{} needs the video encoder (ffmpeg): afx.install_ffmpeg() or File > Animation downloads it, or put ffmpeg on the PATH",
            ext.to_uppercase()
        ));
    };
    let st = std::process::Command::new(ffmpeg)
        .args(["-y", "-loglevel", "error", "-framerate", &fps.to_string(), "-i"])
        .arg(dir.join("frame_%05d.png"))
        .args(codec)
        .arg(out)
        .status();
    match st {
        Ok(s) if s.success() => Ok(()),
        Ok(s) => Err(format!("ffmpeg exited with {s}")),
        Err(e) => Err(format!("ffmpeg: {e}")),
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
    // Hidden while the views are recorded as laid out: it would be in the pictures.
    if !state.anim_dialog.open || state.anim_job.as_ref().is_some_and(|j| j.layout.is_some()) {
        return;
    }
    let mut open = true;
    let mut start: Option<(PathBuf, f32, AnimMode, usize)> = None;
    let view = state.viewport.size();
    let n_views = state.capture.pane_rects.iter().filter(|(p, _)| crate::capture::is_view(*p)).count();
    let n_3d = state.views.len() + 1;
    let ntimes = state.session.lock().times.len();
    egui::Window::new("Animation").open(&mut open).collapsible(false).resizable(false).default_width(420.0).show_fit(ctx, |ui| {
        {
            let d = &mut state.anim_dialog;
            if ntimes < 2 && d.mode == AnimMode::Time {
                d.mode = AnimMode::Orbit;
            }
            ui.horizontal(|ui| {
                ui.label("Animate");
                ui.add_enabled_ui(ntimes > 1, |ui| {
                    ui.radio_value(&mut d.mode, AnimMode::Time, format!("Time steps ({ntimes})"));
                });
                ui.radio_value(&mut d.mode, AnimMode::Orbit, "Orbit 360°");
                ui.radio_value(&mut d.mode, AnimMode::Keyframes, "Keyframes");
            });
        }
        if state.anim_dialog.mode == AnimMode::Keyframes {
            ui.separator();
            keyframe_editor(state, ui, ntimes);
        }
        ui.separator();
        let d = &mut state.anim_dialog;
        egui::Grid::new("anim").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
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
                    ui.radio_value(&mut d.format, AnimFormat::Mp4, "MP4").on_disabled_hover_text("Needs the video encoder: see the line below");
                    ui.radio_value(&mut d.format, AnimFormat::Webm, "WebM").on_disabled_hover_text("Needs the video encoder: see the line below");
                });
                ui.radio_value(&mut d.format, AnimFormat::Png, "PNG frames");
            });
            ui.end_row();
            if !ffmpeg_available() {
                ui.label("");
                ui.vertical(|ui| {
                    let running = ffmpeg_download_state(|s| match s {
                        Some(FfmpegDownload::Running(mb)) => Some(*mb),
                        _ => None,
                    });
                    if let Some(mb) = running {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(format!("Downloading the video encoder… {mb:.0} MB"));
                        });
                        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
                    } else if ffmpeg_download_url().is_some() {
                        if ui
                            .button("Download the video encoder")
                            .on_hover_text(
                                "MP4 and WebM are written by ffmpeg, a separate free program (GPL) that auraFX runs. This fetches a ready-made copy (about 30 MB, from github.com/eugeneware/ffmpeg-static) into auraFX's own data folder; nothing else on the machine changes. An ffmpeg already on the PATH is used instead when there is one.",
                            )
                            .clicked()
                        {
                            start_ffmpeg_download(ui.ctx().clone());
                        }
                    } else {
                        ui.label(RichText::new("MP4 and WebM need ffmpeg on the PATH").weak().small());
                    }
                    if let Some(e) = ffmpeg_download_state(|s| match s {
                        Some(FfmpegDownload::Failed(e)) => Some(e.clone()),
                        _ => None,
                    }) {
                        ui.colored_label(crate::theme::ERROR, e);
                    }
                });
                ui.end_row();
            }
        });
        ui.separator();
        ui.set_min_width(480.0);
        ui.label("Record");
        crate::screenshot_ui::which_views_ui(ui, &mut d.views, &mut d.view_no, n_views, n_3d);
        d.options.reset_size(view);
        egui::Grid::new("anim picture").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
            if d.views == 0 {
                d.options.size_rows(ui, view);
            }
            ui.label("Background");
            d.options.background_ui(ui, false, d.views != 0);
            ui.end_row();
        });
        if d.views != 0 {
            ui.label(RichText::new("Frames are cut out of the window, with every view where it is laid out; this window is hidden while recording.").weak().small());
        } else if d.format == AnimFormat::Gif && d.options.width > 960 {
            ui.label(RichText::new("GIF frames are scaled down to 960 pixels wide; MP4, WebM and PNG frames keep the size.").weak().small());
        }
        if ui.button(RichText::new(format!("{} Save…", crate::icons::FLOPPY_DISK)).strong()).clicked() {
            let (ext, label) = match d.format {
                AnimFormat::Gif => ("gif", "Animated GIF"),
                AnimFormat::Mp4 => ("mp4", "MP4 video"),
                AnimFormat::Webm => ("webm", "WebM video (VP9, for web pages)"),
                AnimFormat::Png => ("png", "PNG frames (a folder is created)"),
            };
            if let Some(p) = crate::dialogs::FileDialog::new().add_filter(label, &[ext]).set_file_name(format!("animation.{ext}")).save_file() {
                let p = if p.extension().is_none() { p.with_extension(ext) } else { p };
                start = Some((p, d.fps, d.mode, d.orbit_frames));
            }
        }
    });
    if !open {
        state.anim_dialog.open = false;
        state.anim_dialog.preview = None;
    }
    if let Some((p, fps, mode, frames)) = start {
        let d = &state.anim_dialog;
        let layout = crate::screenshot_ui::layout_of(d.views, d.view_no);
        let picture = Some((d.options.size(view), d.options.background()));
        state.start_animation_mode(p, fps, mode, frames, layout, picture);
    }
}

/// How a key moves on to the next one, as a small menu. Returns true when changed.
fn interp_menu(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, interp: &mut aurafx_pipeline::animation::Interp) -> bool {
    let before = *interp;
    egui::ComboBox::from_id_salt(id).width(86.0).selected_text(interp.label()).show_ui(ui, |ui| {
        for i in aurafx_pipeline::animation::Interp::ALL {
            ui.selectable_value(interp, i, i.label());
        }
    });
    *interp != before
}

/// Timeline editor: scrub, camera keyframes, parameter tracks.
fn keyframe_editor(state: &mut State, ui: &mut egui::Ui, ntimes: usize) {
    use aurafx_filters::params::ParamValue;
    use aurafx_pipeline::animation::animatable;
    let numeric = |v: &ParamValue| matches!(v, ParamValue::Float(_) | ParamValue::Int(_) | ParamValue::Vec3(_));
    let anim = state.session.lock().animation.clone();
    let mut changed = anim.clone();
    let mut t = state.anim_dialog.t;
    let pct = |t: f64| format!("{:.0}%", t * 100.0);

    // Timeline with keyframe ticks.
    ui.horizontal(|ui| {
        let playing = state.anim_dialog.preview.is_some();
        let icon = if playing { crate::icons::PAUSE } else { crate::icons::PLAY };
        if ui.button(icon).on_hover_text("Preview").clicked() {
            state.anim_dialog.preview = if playing { None } else { Some((std::time::Instant::now(), if t >= 1.0 { 0.0 } else { t })) };
        }
        let r = ui.add(egui::Slider::new(&mut t, 0.0..=1.0).show_value(false).trailing_fill(true));
        let rect = r.rect;
        let painter = ui.painter();
        let x = |t: f64| egui::lerp(rect.left() + 6.0..=rect.right() - 6.0, t as f32);
        for k in &anim.camera {
            painter.circle_filled(egui::pos2(x(k.t), rect.top() + 2.0), 3.5, crate::theme::ACCENT);
        }
        for tr in &anim.tracks {
            for k in &tr.keys {
                painter.circle_filled(egui::pos2(x(k.t), rect.bottom() - 2.0), 3.0, egui::Color32::from_rgb(80, 200, 255));
            }
        }
        ui.label(pct(t));
        if r.changed() {
            state.anim_dialog.preview = None;
            state.anim_dialog.t = t;
            state.apply_animation_time(t);
        }
    });
    ui.label(
        RichText::new("Move the slider, set up the view or a filter value, then add a keyframe. Orange dots are camera keys, blue dots parameter keys.")
            .weak()
            .small(),
    );

    // Camera keys.
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} Camera", crate::icons::VIDEO_CAMERA)).strong());
        if ui.button(format!("{} Key view at {}", crate::icons::PLUS, pct(t))).clicked() {
            let cam = state.camera_state();
            changed.key_camera(t, cam);
        }
    });
    let mut remove_cam = None;
    for (i, k) in anim.camera.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.add_space(16.0);
            if crate::touch::small_button(ui, pct(k.t)).on_hover_text("Jump here").clicked() {
                state.anim_dialog.t = k.t;
                state.apply_animation_time(k.t);
            }
            if i + 1 < anim.camera.len() {
                let mut interp = k.interp;
                if interp_menu(ui, ("cam interp", i), &mut interp) {
                    changed.camera[i].interp = interp;
                }
            }
            if crate::touch::small_button(ui, crate::icons::TRASH).on_hover_text("Delete this keyframe").clicked() {
                remove_cam = Some(i);
            }
        });
    }
    if let Some(i) = remove_cam {
        changed.camera.remove(i);
    }

    // Parameter tracks.
    ui.add_space(4.0);
    let (active, params): (Option<(aurafx_pipeline::NodeId, String)>, Vec<(String, String)>) = {
        let s = state.session.lock();
        match s.active_node() {
            Some(n) => {
                let labels: std::collections::HashMap<String, String> =
                    n.def().map(|d| d.params.iter().map(|p| (p.key.clone(), p.label.clone())).collect()).unwrap_or_default();
                let mut ps: Vec<(String, String)> =
                    n.params.0.iter().filter(|(_, v)| numeric(v)).map(|(k, _)| (k.clone(), labels.get(k).cloned().unwrap_or_else(|| k.clone()))).collect();
                // Display properties too.
                for d in aurafx_pipeline::animation::DISPLAY_TRACKS {
                    ps.push((format!("display.{d}"), format!("Display · {}", d.replace('_', " "))));
                }
                (Some((n.id, n.name.clone())), ps)
            }
            None => (None, vec![]),
        }
    };
    ui.label(RichText::new(format!("{} Filter and display values", crate::icons::SLIDERS_HORIZONTAL)).strong());
    match &active {
        Some((id, name)) if !params.is_empty() => {
            ui.horizontal(|ui| {
                if !params.iter().any(|(k, _)| *k == state.anim_dialog.track_key) {
                    state.anim_dialog.track_key = params[0].0.clone();
                }
                let cur = params.iter().find(|(k, _)| *k == state.anim_dialog.track_key).map(|(_, l)| l.clone()).unwrap_or_default();
                egui::ComboBox::from_id_salt("track_key").selected_text(format!("{name} · {cur}")).show_ui(ui, |ui| {
                    for (k, l) in &params {
                        ui.selectable_value(&mut state.anim_dialog.track_key, k.clone(), l);
                    }
                });
                if ui.button(format!("{} Key value at {}", crate::icons::PLUS, pct(t))).on_hover_text("Records the current value of this property").clicked() {
                    let key = state.anim_dialog.track_key.clone();
                    let (v, spec) = {
                        let s = state.session.lock();
                        let n = s.nodes.get(id);
                        let v = match key.strip_prefix("display.") {
                            Some(d) => n.and_then(|n| aurafx_pipeline::animation::display_value(&n.display, d)),
                            None => n.and_then(|n| n.params.get(&key).cloned()),
                        };
                        (v, n.and_then(|n| n.def()).and_then(|d| d.params.iter().find(|p| p.key == key).cloned()))
                    };
                    // "Auto" values are filled in from the data, as when editing them in Properties.
                    let v = match (v, spec) {
                        (Some(v), _) if animatable(&v) => Some(v),
                        (Some(_), Some(spec)) => Some(crate::panels::auto_fill(state, *id, &spec)).filter(animatable),
                        _ => None,
                    };
                    if let Some(v) = v {
                        changed.key_param(*id, &state.anim_dialog.track_key, t, v);
                    }
                }
            });
        }
        _ => {
            ui.label(RichText::new("Select a filter (for example a Slice or Contour) to animate its values.").weak().small());
        }
    }
    let names: std::collections::HashMap<aurafx_pipeline::NodeId, String> = state.session.lock().nodes.values().map(|n| (n.id, n.name.clone())).collect();
    let mut remove_track = None;
    let mut remove_key = None;
    for (ti, tr) in anim.tracks.iter().enumerate() {
        let Some(name) = names.get(&tr.node) else { continue };
        ui.horizontal_wrapped(|ui| {
            ui.add_space(16.0);
            ui.label(format!("{name} · {}", tr.key));
            for (ki, k) in tr.keys.iter().enumerate() {
                let txt = match &k.value {
                    aurafx_filters::params::ParamValue::Float(v) => aurafx_data::info::g(*v),
                    aurafx_filters::params::ParamValue::Int(v) => v.to_string(),
                    aurafx_filters::params::ParamValue::Vec3(v) => {
                        format!("({}, {}, {})", aurafx_data::info::g(v[0]), aurafx_data::info::g(v[1]), aurafx_data::info::g(v[2]))
                    }
                    _ => String::new(),
                };
                let b = crate::touch::small_button(ui, format!("{} = {txt}", pct(k.t))).on_hover_text("Jump here; right-click to delete this keyframe");
                if b.clicked() {
                    state.anim_dialog.t = k.t;
                    state.apply_animation_time(k.t);
                }
                b.context_menu(|ui| {
                    if ui.button("Delete keyframe").clicked() {
                        remove_key = Some((ti, ki));
                        ui.close();
                    }
                });
                if ki + 1 < tr.keys.len() {
                    let mut interp = k.interp;
                    if interp_menu(ui, ("track interp", ti, ki), &mut interp) {
                        changed.tracks[ti].keys[ki].interp = interp;
                    }
                }
            }
            if crate::touch::small_button(ui, crate::icons::TRASH).on_hover_text("Delete this track").clicked() {
                remove_track = Some(ti);
            }
        });
    }
    if let Some((ti, ki)) = remove_key {
        changed.tracks[ti].keys.remove(ki);
    }
    if let Some(ti) = remove_track {
        changed.tracks.remove(ti);
    }
    changed.tracks.retain(|tr| !tr.keys.is_empty());

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        if ntimes > 1 {
            ui.checkbox(&mut changed.animate_time, format!("Also play the {ntimes} time steps"));
        }
        ui.label("Frames");
        ui.add(egui::DragValue::new(&mut changed.frames).range(2..=3600));
    });
    if !anim.is_empty() && crate::touch::small_button(ui, "Clear all keyframes").clicked() {
        changed.camera.clear();
        changed.tracks.clear();
    }
    if changed != anim {
        state.session.lock().animation = changed;
    }
}
