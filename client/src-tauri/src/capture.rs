use std::sync::atomic::{AtomicU32, Ordering};

pub static CURRENT_SCREEN_WIDTH: AtomicU32 = AtomicU32::new(1920);
pub static CURRENT_SCREEN_HEIGHT: AtomicU32 = AtomicU32::new(1080);
pub static CAPTURE_QUALITY: AtomicU32 = AtomicU32::new(75);
pub static CAPTURE_SCALE: AtomicU32 = AtomicU32::new(100);

pub fn set_quality_mode(mode: &str) {
    match mode {
        "crystal" => {
            // Ultra-clarity: 100% resolution, Quality 88 (crystal text, ideal for coding/reading)
            CAPTURE_QUALITY.store(88, Ordering::Relaxed);
            CAPTURE_SCALE.store(100, Ordering::Relaxed);
            println!("[Capture] Switched to Ultra Clarity mode (Q=88, scale=1.0)");
        }
        "performance" => {
            // High-framerate: 80% resolution, Quality 65 (fastest encoding, 50-60 FPS)
            CAPTURE_QUALITY.store(65, Ordering::Relaxed);
            CAPTURE_SCALE.store(80, Ordering::Relaxed);
            println!("[Capture] Switched to High Framerate mode (Q=65, scale=0.8)");
        }
        _ => {
            // Balanced (default): 100% resolution, Quality 75
            CAPTURE_QUALITY.store(75, Ordering::Relaxed);
            CAPTURE_SCALE.store(100, Ordering::Relaxed);
            println!("[Capture] Switched to Balanced mode (Q=75, scale=1.0)");
        }
    }
}

static FALLBACK_JPEG: &[u8] = include_bytes!("../icons/fallback.jpg");

/// Parse width & height from standard JPEG SOF markers in microseconds
#[allow(dead_code)]
fn update_jpeg_dimensions(data: &[u8]) {
    let mut i = 0;
    while i + 8 < data.len() {
        if data[i] == 0xFF && (data[i + 1] == 0xC0 || data[i + 1] == 0xC2) {
            let h = ((data[i + 5] as u32) << 8) | (data[i + 6] as u32);
            let w = ((data[i + 7] as u32) << 8) | (data[i + 8] as u32);
            if w > 0 && h > 0 {
                CURRENT_SCREEN_WIDTH.store(w, Ordering::Relaxed);
                CURRENT_SCREEN_HEIGHT.store(h, Ordering::Relaxed);
            }
            return;
        }
        i += 1;
    }
}

#[cfg(target_os = "linux")]
fn capture_os() -> Option<Vec<u8>> {
    let q = CAPTURE_QUALITY.load(Ordering::Relaxed).to_string();
    let s = CAPTURE_SCALE.load(Ordering::Relaxed);

    let mut cmd = std::process::Command::new("grim");
    cmd.args(["-t", "jpeg", "-q", &q]);

    let scale_arg;
    if s > 0 && s < 100 {
        scale_arg = format!("{:.2}", s as f32 / 100.0);
        cmd.args(["-s", &scale_arg]);
    }
    cmd.arg("-");

    if let Ok(output) = cmd.output() {
        if output.status.success() && !output.stdout.is_empty() {
            update_jpeg_dimensions(&output.stdout);
            return Some(output.stdout);
        }
    }

    // Fallback scaled capture if hardware requires lower resolution
    if let Ok(output) = std::process::Command::new("grim")
        .args(["-s", "0.80", "-t", "jpeg", "-q", "65", "-"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            update_jpeg_dimensions(&output.stdout);
            return Some(output.stdout);
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn capture_os() -> Option<Vec<u8>> {
    None
}

#[cfg(target_os = "windows")]
fn capture_os() -> Option<Vec<u8>> {
    None
}

pub fn capture_desktop_frame() -> (Vec<u8>, &'static str) {
    if let Some(jpeg_bytes) = capture_os() {
        return (jpeg_bytes, "image/jpeg");
    }
    // High-definition fallback is ALWAYS valid image/jpeg, guaranteeing WebKit compatibility
    (FALLBACK_JPEG.to_vec(), "image/jpeg")
}

#[cfg(target_os = "linux")]
pub fn spawn_h264_recorder_process() -> Result<tokio::process::Child, std::io::Error> {
    // Kill any lingering recorder instance to ensure clean port/device access
    let _ = std::process::Command::new("pkill").arg("-x").arg("wf-recorder").status();

    let encoder_args = if std::path::Path::new("/proc/driver/nvidia/version").exists() {
        println!("[H264/GPU] Detected NVIDIA GPU -> Using h264_nvenc (Hardware Acceleration, 60fps, 30M, Ultra-Clarity)");
        "-c h264_nvenc -p preset=p1 -p tune=ull -p b=30M -p g=30 -p bf=0"
    } else if std::path::Path::new("/dev/dri/renderD128").exists() {
        println!("[H264/GPU] Detected VA-API GPU (/dev/dri/renderD128) -> Using h264_vaapi (Hardware Acceleration, 60fps, 30M, Ultra-Clarity)");
        "-c h264_vaapi -d /dev/dri/renderD128 -p b=30M -p g=30 -p bf=0"
    } else if std::path::Path::new("/dev/dri/renderD129").exists() {
        println!("[H264/GPU] Detected VA-API GPU (/dev/dri/renderD129) -> Using h264_vaapi (Hardware Acceleration, 60fps, 30M, Ultra-Clarity)");
        "-c h264_vaapi -d /dev/dri/renderD129 -p b=30M -p g=30 -p bf=0"
    } else {
        println!("[H264/CPU] No dedicated hardware encoder node -> Using libx264 ultrafast (CRF 16, 60fps, 30M)");
        "-c libx264 -p preset=ultrafast -p tune=zerolatency -p crf=16 -p b=30M -p g=30 -p bf=0"
    };

    // Matroska container streams cleanly to stdout without seeking; ffmpeg remuxes it to Fragmented MP4 with 0% CPU
    let pipeline = format!(
        "wf-recorder -m matroska -x yuv420p -r 60 {} -f - | ffmpeg -hide_banner -loglevel error -f matroska -i - -c:v copy -f mp4 -movflags frag_keyframe+empty_moov+default_base_moof -",
        encoder_args
    );

    println!("[H264] Spawning hardware streaming pipeline: {}", pipeline);

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(pipeline);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::inherit());

    cmd.spawn()
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_h264_recorder_process() -> Result<tokio::process::Child, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Hardware H.264 stream via wf-recorder is designed for Linux / Hyprland hosts",
    ))
}


