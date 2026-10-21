use std::sync::atomic::{AtomicU32, Ordering};

pub static CURRENT_SCREEN_WIDTH: AtomicU32 = AtomicU32::new(1920);
pub static CURRENT_SCREEN_HEIGHT: AtomicU32 = AtomicU32::new(1080);

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
    // 1. High-Definition Wayland capture via grim (Quality 80 for razor-sharp text & code)
    if let Ok(output) = std::process::Command::new("grim")
        .args(["-t", "jpeg", "-q", "80", "-"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            update_jpeg_dimensions(&output.stdout);
            return Some(output.stdout);
        }
    }

    // 2. Fallback with slight scale if full screen is too heavy on weak hardware
    if let Ok(output) = std::process::Command::new("grim")
        .args(["-s", "0.85", "-t", "jpeg", "-q", "80", "-"])
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
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayPixelsWide(display: u32) -> usize;
        fn CGDisplayPixelsHigh(display: u32) -> usize;
    }
    unsafe {
        let disp = CGMainDisplayID();
        let w = CGDisplayPixelsWide(disp) as u32;
        let h = CGDisplayPixelsHigh(disp) as u32;
        if w > 0 && h > 0 {
            CURRENT_SCREEN_WIDTH.store(w, Ordering::Relaxed);
            CURRENT_SCREEN_HEIGHT.store(h, Ordering::Relaxed);
        }
    }

    let tmp_orig = "/tmp/hypr_mac_capture.jpg";
    let tmp_scaled = "/tmp/hypr_mac_scaled.jpg";

    if let Ok(st) = std::process::Command::new("screencapture")
        .args(["-x", "-t", "jpg", tmp_orig])
        .status()
    {
        if st.success() {
            // High Definition 1920px max dimension: crisp fonts, sharp text, smooth framerate
            let _ = std::process::Command::new("sips")
                .args(["-Z", "1920", "-s", "formatOptions", "80", tmp_orig, "--out", tmp_scaled])
                .output();

            if let Ok(data) = std::fs::read(tmp_scaled) {
                if !data.is_empty() {
                    return Some(data);
                }
            }
            if let Ok(data) = std::fs::read(tmp_orig) {
                if !data.is_empty() {
                    return Some(data);
                }
            }
        }
    }

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
