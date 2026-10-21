use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};

pub static CURRENT_SCREEN_WIDTH: AtomicU32 = AtomicU32::new(1920);
pub static CURRENT_SCREEN_HEIGHT: AtomicU32 = AtomicU32::new(1080);

static COLOR_CYCLE: AtomicU8 = AtomicU8::new(0);

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
    // 1. Try grim with scale for high FPS Wayland streaming
    if let Ok(output) = std::process::Command::new("grim")
        .args(["-s", "0.7", "-t", "jpeg", "-q", "50", "-"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            update_jpeg_dimensions(&output.stdout);
            return Some(output.stdout);
        }
    }

    // 2. Try regular grim without scale
    if let Ok(output) = std::process::Command::new("grim")
        .args(["-t", "jpeg", "-q", "50", "-"])
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

    // screencapture writes to file (dash '-' creates a file named '-' instead of stdout)
    if let Ok(st) = std::process::Command::new("screencapture")
        .args(["-x", "-t", "jpg", tmp_orig])
        .status()
    {
        if st.success() {
            // Downscale to 1280 max dimension using macOS native sips tool (<4ms, reduces 1.1MB to 120KB)
            let _ = std::process::Command::new("sips")
                .args(["-Z", "1280", tmp_orig, "--out", tmp_scaled])
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

/// Fallback frame generator (valid 24-bit BMP) if OS capture utility is not permitted
fn generate_fallback_bmp(width: u32, height: u32) -> Vec<u8> {
    let cycle = COLOR_CYCLE.fetch_add(2, Ordering::Relaxed);
    let row_size = ((width * 3 + 3) / 4) * 4;
    let image_size = row_size * height;
    let file_size = 54 + image_size;
    let mut buf = Vec::with_capacity(file_size as usize);

    // 14-byte BMP Header
    buf.extend_from_slice(b"BM");
    buf.extend_from_slice(&(file_size as u32).to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&54u32.to_le_bytes());

    // 40-byte DIB Header
    buf.extend_from_slice(&40u32.to_le_bytes());
    buf.extend_from_slice(&(width as i32).to_le_bytes());
    buf.extend_from_slice(&(height as i32).to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes());
    buf.extend_from_slice(&24u16.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&(image_size as u32).to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes());
    buf.extend_from_slice(&2835u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Neutral dark desktop background
    let padding = row_size - width * 3;
    for y in 0..height {
        let b = 35u8.wrapping_add(cycle / 10);
        let g = 25u8.wrapping_add((y * 20 / height) as u8);
        for _ in 0..width {
            buf.push(b);
            buf.push(g);
            buf.push(20u8);
        }
        for _ in 0..padding {
            buf.push(0);
        }
    }
    buf
}

pub fn capture_desktop_frame() -> (Vec<u8>, &'static str) {
    if let Some(jpeg_bytes) = capture_os() {
        return (jpeg_bytes, "image/jpeg");
    }
    // High-fps neutral fallback frame
    (generate_fallback_bmp(640, 360), "image/bmp")
}
