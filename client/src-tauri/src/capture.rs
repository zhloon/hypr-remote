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
    use std::ffi::c_void;

    #[link(name = "CoreGraphics", kind = "framework")]
    #[link(name = "ImageIO", kind = "framework")]
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CGMainDisplayID() -> u32;
        fn CGDisplayCreateImage(displayID: u32) -> *mut c_void;
        fn CGImageGetWidth(image: *mut c_void) -> usize;
        fn CGImageGetHeight(image: *mut c_void) -> usize;
        fn CGImageRelease(image: *mut c_void);

        fn CFDataCreateMutable(allocator: *const c_void, capacity: isize) -> *mut c_void;
        fn CFDataGetLength(theData: *mut c_void) -> isize;
        fn CFDataGetBytePtr(theData: *mut c_void) -> *const u8;
        fn CFRelease(cf: *mut c_void);
        fn CFStringCreateWithCString(alloc: *const c_void, cStr: *const u8, encoding: u32) -> *mut c_void;

        fn CGImageDestinationCreateWithData(
            data: *mut c_void,
            type_: *mut c_void,
            count: usize,
            options: *const c_void,
        ) -> *mut c_void;
        fn CGImageDestinationAddImage(
            dest: *mut c_void,
            image: *mut c_void,
            properties: *const c_void,
        );
        fn CGImageDestinationFinalize(dest: *mut c_void) -> bool;
    }

    // 1. Blazing-fast in-memory capture via CoreGraphics + ImageIO (0 disk IO, 0 process forks, ~20ms)
    unsafe {
        let disp = CGMainDisplayID();
        let img = CGDisplayCreateImage(disp);
        if !img.is_null() {
            let w = CGImageGetWidth(img) as u32;
            let h = CGImageGetHeight(img) as u32;
            if w > 0 && h > 0 {
                CURRENT_SCREEN_WIDTH.store(w, Ordering::Relaxed);
                CURRENT_SCREEN_HEIGHT.store(h, Ordering::Relaxed);
            }

            let cf_data = CFDataCreateMutable(std::ptr::null(), 0);
            let k_utf8 = 0x08000100;
            let uti_jpeg = CFStringCreateWithCString(std::ptr::null(), b"public.jpeg\0".as_ptr(), k_utf8);

            let mut result = None;
            let dest = CGImageDestinationCreateWithData(cf_data, uti_jpeg, 1, std::ptr::null());
            if !dest.is_null() {
                CGImageDestinationAddImage(dest, img, std::ptr::null());
                if CGImageDestinationFinalize(dest) {
                    let len = CFDataGetLength(cf_data) as usize;
                    let ptr = CFDataGetBytePtr(cf_data);
                    if len > 0 && !ptr.is_null() {
                        let bytes = std::slice::from_raw_parts(ptr, len).to_vec();
                        result = Some(bytes);
                    }
                }
                CFRelease(dest);
            }
            CFRelease(uti_jpeg);
            CFRelease(cf_data);
            CGImageRelease(img);

            if let Some(data) = result {
                return Some(data);
            }
        }
    }

    // 2. Direct screencapture fallback (without slow sips process)
    let tmp_path = "/tmp/hypr_mac_capture.jpg";
    if let Ok(st) = std::process::Command::new("screencapture")
        .args(["-m", "-x", "-t", "jpg", tmp_path])
        .status()
    {
        if st.success() {
            if let Ok(data) = std::fs::read(tmp_path) {
                if !data.is_empty() {
                    update_jpeg_dimensions(&data);
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

#[cfg(target_os = "linux")]
pub fn spawn_h264_recorder_process() -> Result<tokio::process::Child, std::io::Error> {
    let mut cmd = tokio::process::Command::new("wf-recorder");
    cmd.arg("-m").arg("mp4");
    cmd.arg("--muxer-param=movflags=frag_keyframe+empty_moov+default_base_moof");
    cmd.arg("-x").arg("yuv420p");
    cmd.arg("-p").arg("tune=zerolatency");
    cmd.arg("-F").arg("fps=60");

    // Detect hardware acceleration device
    if std::path::Path::new("/proc/driver/nvidia/version").exists() {
        println!("[H264] Detected NVIDIA GPU -> Using h264_nvenc");
        cmd.arg("-c").arg("h264_nvenc");
        cmd.arg("-p").arg("preset=p1");
    } else if std::path::Path::new("/dev/dri/renderD128").exists() {
        println!("[H264] Detected VA-API GPU (/dev/dri/renderD128) -> Using h264_vaapi");
        cmd.arg("-c").arg("h264_vaapi");
        cmd.arg("-d").arg("/dev/dri/renderD128");
    } else if std::path::Path::new("/dev/dri/renderD129").exists() {
        println!("[H264] Detected VA-API GPU (/dev/dri/renderD129) -> Using h264_vaapi");
        cmd.arg("-c").arg("h264_vaapi");
        cmd.arg("-d").arg("/dev/dri/renderD129");
    } else {
        println!("[H264] No dedicated hardware encoder node -> Using libx264 ultrafast");
        cmd.arg("-c").arg("libx264");
        cmd.arg("-p").arg("preset=ultrafast");
    }

    cmd.arg("-f").arg("-");
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::null());

    cmd.spawn()
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_h264_recorder_process() -> Result<tokio::process::Child, std::io::Error> {
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Hardware H.264 stream via wf-recorder is designed for Linux / Hyprland hosts",
    ))
}


