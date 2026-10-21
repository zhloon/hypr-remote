#![allow(unused_imports, dead_code)]
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

pub static CURRENT_SCREEN_WIDTH: AtomicU32 = AtomicU32::new(1920);
pub static CURRENT_SCREEN_HEIGHT: AtomicU32 = AtomicU32::new(1080);
pub static CAPTURE_QUALITY: AtomicU32 = AtomicU32::new(68);
pub static CAPTURE_SCALE: AtomicU32 = AtomicU32::new(85);
pub static ACTIVE_MONITOR_NAME: Mutex<Option<String>> = Mutex::new(None);

pub fn set_quality_mode(mode: &str) {
    match mode {
        "crystal" => {
            // Ultra-clarity: 100% resolution, Quality 85 (crystal text, ideal for coding/reading)
            CAPTURE_QUALITY.store(85, Ordering::Relaxed);
            CAPTURE_SCALE.store(100, Ordering::Relaxed);
            println!("[Capture] Switched to Ultra Clarity mode (Q=85, scale=1.0)");
        }
        "performance" => {
            // High-framerate: 70% resolution, Quality 55 (fastest encoding, 60+ FPS)
            CAPTURE_QUALITY.store(55, Ordering::Relaxed);
            CAPTURE_SCALE.store(70, Ordering::Relaxed);
            println!("[Capture] Switched to High Framerate mode (Q=55, scale=0.70)");
        }
        _ => {
            // Balanced (default): 85% resolution, Quality 68 (fast encoding, smooth 60 FPS)
            CAPTURE_QUALITY.store(68, Ordering::Relaxed);
            CAPTURE_SCALE.store(85, Ordering::Relaxed);
            println!("[Capture] Switched to Balanced mode (Q=68, scale=0.85)");
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
static LAST_MONITOR_CHECK: AtomicU64 = AtomicU64::new(0);

#[cfg(target_os = "linux")]
pub fn detect_focused_monitor() -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let last = LAST_MONITOR_CHECK.load(Ordering::Relaxed);

    // Fast-path: return cached monitor if checked within last 4 seconds
    if now.saturating_sub(last) < 4 {
        if let Ok(lock) = ACTIVE_MONITOR_NAME.lock() {
            if let Some(ref m) = *lock {
                return Some(m.clone());
            }
        }
    }
    LAST_MONITOR_CHECK.store(now, Ordering::Relaxed);

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
    // Kill any lingering recorder instance to ensure clean port/device access and avoid CPU leaks
    let _ = std::process::Command::new("pkill").args(["-9", "-f", "wf-recorder.*mpegts"]).status();
    let _ = std::process::Command::new("killall").args(["-9", "wf-recorder"]).status();

    let mon_name = {
        let active = ACTIVE_MONITOR_NAME.lock().ok().and_then(|g| g.clone());
        active.or_else(detect_focused_monitor).or_else(|| {
            if let Ok(output) = std::process::Command::new("wf-recorder").arg("-L").output() {
                let s = String::from_utf8_lossy(&output.stdout);
                s.lines().next().map(|l| l.trim().to_string())
            } else {
                None
            }
        })
    };
    let out_flag = match mon_name {
        Some(ref m) if !m.is_empty() => format!("-o {}", m),
        _ => "".to_string(),
    };

    let is_nvidia = std::path::Path::new("/proc/driver/nvidia/version").exists();
    let has_vaapi = std::path::Path::new("/dev/dri/renderD128").exists() || std::path::Path::new("/dev/dri/renderD129").exists();
    let vaapi_dev = if std::path::Path::new("/dev/dri/renderD128").exists() { "/dev/dri/renderD128" } else { "/dev/dri/renderD129" };

    if is_nvidia {
        println!("[H264/GPU] Detected NVIDIA GPU -> Preferring h264_nvenc (Hardware Acceleration, 120fps, Ultra-Low Latency)");
    } else if has_vaapi {
        println!("[H264/GPU] Detected VA-API GPU ({}) -> Using h264_vaapi (Hardware Acceleration, 120fps, Zero-Copy)", vaapi_dev);
    } else {
        println!("[H264/CPU] No dedicated hardware encoder node -> Using libx264 ultrafast (120fps, capped 2 threads)");
    }

    // If CPU fallback is needed, cap threads=4 to prevent stutter while avoiding thermal runaway
    let cpu_fallback = "-c libx264 -b 0 -p preset=ultrafast -p tune=zerolatency -p g=30 -p threads=4";
    let cpu_wf = format!("wf-recorder -D -y {} -m mpegts -r 120 {} -f /dev/stdout", out_flag, cpu_fallback);
    let vaapi_wf = format!("wf-recorder -D -y {} -m mpegts -r 120 -c h264_vaapi -d {} -b 0 -p g=30 -f /dev/stdout", out_flag, vaapi_dev);

    let wf_cmd = if is_nvidia {
        let nv_p1 = format!("wf-recorder -D -y {} -m mpegts -r 120 -c h264_nvenc -b 0 -p preset=p1 -p tune=ull -p g=30 -f /dev/stdout", out_flag);
        let nv_fast = format!("wf-recorder -D -y {} -m mpegts -r 120 -c h264_nvenc -b 0 -p preset=fast -p g=30 -f /dev/stdout", out_flag);
        let nv_basic = format!("wf-recorder -D -y {} -m mpegts -r 120 -c h264_nvenc -b 0 -p g=30 -f /dev/stdout", out_flag);
        if has_vaapi {
            format!("({} || {} || {} || {} || {})", nv_p1, nv_fast, nv_basic, vaapi_wf, cpu_wf)
        } else {
            format!("({} || {} || {} || {})", nv_p1, nv_fast, nv_basic, cpu_wf)
        }
    } else if has_vaapi {
        format!("({} || {})", vaapi_wf, cpu_wf)
    } else {
        cpu_wf
    };

    let pipeline = format!(
        "{} | ffmpeg -hide_banner -loglevel warning -fflags nobuffer+discardcorrupt -flags low_delay -probesize 65536 -analyzeduration 100000 -f mpegts -i - -c:v copy -flush_packets 1 -f mp4 -movflags frag_keyframe+empty_moov+default_base_moof -frag_duration 33333 -",
        wf_cmd
    );

    println!("[H264] Spawning hardware streaming pipeline: {}", pipeline);

    let mut cmd = tokio::process::Command::new("sh");
    cmd.arg("-c").arg(pipeline);
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::inherit());
    #[cfg(unix)]
    cmd.process_group(0);

    cmd.spawn()
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_h264_recorder_process() -> Result<tokio::process::Child, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Hardware H.264 stream via wf-recorder is designed for Linux / Hyprland hosts",
    ))
}

#[cfg(target_os = "linux")]
pub fn set_screen_resolution(res: &str) -> bool {
    crate::input::reset_monitor_cache();

    if res == "physical" || res == "auto" || res == "preferred" {
        println!("[Hyprland] Reverting to physical monitor");
        let _ = std::process::Command::new("hyprctl")
            .args(["output", "remove", "HEADLESS-1"])
            .status();

        if let Ok(mut lock) = ACTIVE_MONITOR_NAME.lock() {
            *lock = None;
        }

        let mon = detect_focused_monitor().unwrap_or_else(|| "eDP-1".to_string());
        println!("[Hyprland] Active physical monitor: {}", mon);
        let _ = std::process::Command::new("hyprctl")
            .args(["keyword", "device:hyprremote-virtual-device:output", &mon])
            .status();
        let _ = std::process::Command::new("hyprctl")
            .args(["keyword", "device:HyprRemote-Virtual-Device:output", &mon])
            .status();

        let _ = std::process::Command::new("pkill").arg("-x").arg("wf-recorder").status();
        return true;
    }

    println!("[Hyprland] Creating virtual headless display with resolution: {}", res);

    // 1. Create headless output (HEADLESS-1)
    let _ = std::process::Command::new("hyprctl")
        .args(["output", "create", "headless", "HEADLESS-1"])
        .status();

    // 2. Configure headless monitor resolution (e.g. HEADLESS-1,2560x1440@120,auto,1)
    let mon_arg = format!("HEADLESS-1,{}@120,auto,1", res);
    let _ = std::process::Command::new("hyprctl")
        .args(["keyword", "monitor", &mon_arg])
        .status();

    // 3. Set active monitor to HEADLESS-1
    if let Ok(mut lock) = ACTIVE_MONITOR_NAME.lock() {
        *lock = Some("HEADLESS-1".to_string());
    }

    // 4. Update screen width & height
    if let Some((w_str, h_str)) = res.split_once('x') {
        if let (Ok(w), Ok(h)) = (w_str.parse::<u32>(), h_str.parse::<u32>()) {
            CURRENT_SCREEN_WIDTH.store(w, Ordering::Relaxed);
            CURRENT_SCREEN_HEIGHT.store(h, Ordering::Relaxed);
        }
    }

    // 5. Bind virtual uinput mouse device directly to HEADLESS-1 (both lowercase and original name)
    let _ = std::process::Command::new("hyprctl")
        .args(["keyword", "device:hyprremote-virtual-device:output", "HEADLESS-1"])
        .status();
    let _ = std::process::Command::new("hyprctl")
        .args(["keyword", "device:HyprRemote-Virtual-Device:output", "HEADLESS-1"])
        .status();

    // 6. Restart wf-recorder to capture HEADLESS-1 at the new resolution
    let _ = std::process::Command::new("pkill").arg("-x").arg("wf-recorder").status();

    true
}

#[cfg(not(target_os = "linux"))]
pub fn set_screen_resolution(_res: &str) -> bool {
    false
}


