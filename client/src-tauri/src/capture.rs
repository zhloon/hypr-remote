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
    // High-performance Wayland capture via grim (Quality 75: optimal balance of crystal text and high FPS)
    if let Ok(output) = std::process::Command::new("grim")
        .args(["-t", "jpeg", "-q", "75", "-"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            update_jpeg_dimensions(&output.stdout);
            return Some(output.stdout);
        }
    }

    // Fallback scaled capture if hardware requires lower resolution
    if let Ok(output) = std::process::Command::new("grim")
        .args(["-s", "0.85", "-t", "jpeg", "-q", "75", "-"])
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


