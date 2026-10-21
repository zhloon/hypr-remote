use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

pub static CURRENT_SCREEN_WIDTH: AtomicU32 = AtomicU32::new(1920);
pub static CURRENT_SCREEN_HEIGHT: AtomicU32 = AtomicU32::new(1080);
pub static CAPTURE_QUALITY: AtomicU32 = AtomicU32::new(75);
pub static CAPTURE_SCALE: AtomicU32 = AtomicU32::new(100);
pub static ACTIVE_MONITOR_NAME: Mutex<Option<String>> = Mutex::new(None);

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
pub fn detect_focused_monitor() -> Option<String> {
    if let Ok(output) = std::process::Command::new("hyprctl").args(["monitors", "-j"]).output() {
        if output.status.success() {
            if let Ok(monitors) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) {
                let mon = monitors.iter().find(|m| m["focused"].as_bool().unwrap_or(false))
                    .or_else(|| monitors.first());
                if let Some(m) = mon {
                    if let Some(name) = m["name"].as_str() {
                        let w = m["width"].as_u64().unwrap_or(1920) as u32;
                        let h = m["height"].as_u64().unwrap_or(1080) as u32;
                        if w > 0 && h > 0 {
                            CURRENT_SCREEN_WIDTH.store(w, Ordering::Relaxed);
                            CURRENT_SCREEN_HEIGHT.store(h, Ordering::Relaxed);
                        }
                        let name_str = name.to_string();
                        if let Ok(mut lock) = ACTIVE_MONITOR_NAME.lock() {
                            *lock = Some(name_str.clone());
                        }
                        return Some(name_str);
                    }
                }
            }
        }
    }
    // Fallback: grim -l
    if let Ok(output) = std::process::Command::new("grim").arg("-l").output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout);
            if let Some(line) = s.lines().next() {
                let name = line.trim();
                if !name.is_empty() {
                    let name_str = name.to_string();
                    if let Ok(mut lock) = ACTIVE_MONITOR_NAME.lock() {
                        *lock = Some(name_str.clone());
                    }
                    return Some(name_str);
                }
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
pub fn detect_focused_monitor() -> Option<String> {
    None
}

#[cfg(target_os = "linux")]
fn capture_os() -> Option<Vec<u8>> {
    let q = CAPTURE_QUALITY.load(Ordering::Relaxed).to_string();
    let s = CAPTURE_SCALE.load(Ordering::Relaxed);

    let mut cmd = std::process::Command::new("grim");
    cmd.args(["-t", "jpeg", "-q", &q]);

    if let Some(mon) = detect_focused_monitor() {
        cmd.args(["-o", &mon]);
    }

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
    let mut fallback_cmd = std::process::Command::new("grim");
    fallback_cmd.args(["-s", "0.80", "-t", "jpeg", "-q", "65"]);
    if let Some(mon) = detect_focused_monitor() {
        fallback_cmd.args(["-o", &mon]);
    }
    fallback_cmd.arg("-");

    if let Ok(output) = fallback_cmd.output() {
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

    let mon_name = detect_focused_monitor();
    let out_flag = match mon_name {
        Some(ref m) => format!("-o {}", m),
        None => "".to_string(),
    };

    let (hw_args, has_hw) = if std::path::Path::new("/proc/driver/nvidia/version").exists() {
        println!("[H264/GPU] Detected NVIDIA GPU -> Using h264_nvenc (Hardware Acceleration, 60fps, 30M, Ultra-Clarity)");
        ("-c h264_nvenc -p preset=p1 -p tune=ull -p b=30M -p g=30 -b 0", true)
    } else if std::path::Path::new("/dev/dri/renderD128").exists() {
        println!("[H264/GPU] Detected VA-API GPU (/dev/dri/renderD128) -> Using h264_vaapi (Hardware Acceleration, 60fps, 30M, Ultra-Clarity)");
        ("-c h264_vaapi -d /dev/dri/renderD128 -p b=30M -p g=30 -b 0", true)
    } else if std::path::Path::new("/dev/dri/renderD129").exists() {
        println!("[H264/GPU] Detected VA-API GPU (/dev/dri/renderD129) -> Using h264_vaapi (Hardware Acceleration, 60fps, 30M, Ultra-Clarity)");
        ("-c h264_vaapi -d /dev/dri/renderD129 -p b=30M -p g=30 -b 0", true)
    } else {
        println!("[H264/CPU] No dedicated hardware encoder node -> Using libx264 ultrafast (CRF 16, 60fps, 30M)");
        ("-c libx264 -p preset=ultrafast -p tune=zerolatency -p crf=16 -p b=30M -p g=30 -b 0", false)
    };

    let cpu_fallback = "-c libx264 -p preset=ultrafast -p tune=zerolatency -p crf=18 -p b=25M -p g=30 -b 0";

    // Continuous 60fps stream with -D (no-damage), overwrite (-y), zero b-frames (-b 0), and instant packet flush!
    // If hardware acceleration fails, auto-fallback to CPU libx264 ultrafast!
    let wf_cmd = if has_hw {
        format!(
            "(wf-recorder -D -y {} -m matroska -x yuv420p -r 60 {} -f - 2>/dev/null || wf-recorder -D -y {} -m matroska -x yuv420p -r 60 {} -f -)",
            out_flag, hw_args, out_flag, cpu_fallback
        )
    } else {
        format!(
            "wf-recorder -D -y {} -m matroska -x yuv420p -r 60 {} -f -",
            out_flag, hw_args
        )
    };

    let pipeline = format!(
        "{} | ffmpeg -hide_banner -loglevel error -flush_packets 1 -f matroska -i - -c:v copy -f mp4 -movflags frag_keyframe+empty_moov+default_base_moof -",
        wf_cmd
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


