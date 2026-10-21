#[allow(unused_imports)]
use crate::capture::{CURRENT_SCREEN_HEIGHT, CURRENT_SCREEN_WIDTH};
use crate::types::ControlMessage;
#[allow(unused_imports)]
use std::sync::atomic::Ordering;

pub fn handle_control_message(msg: ControlMessage) {
    match msg {
        ControlMessage::MouseMove { x, y, dx, dy, is_rel } => {
            dispatch_mouse_move(x, y, dx, dy, is_rel);
        }
        ControlMessage::MouseButton { btn, down } => {
            dispatch_mouse_button(btn, down);
        }
        ControlMessage::MouseWheel { dx, dy } => {
            dispatch_mouse_wheel(dx, dy);
        }
        ControlMessage::Key { code, down } => {
            dispatch_key(code, down);
        }
        _ => {}
    }
}

// ==========================================
// macOS Input Implementation (Direct Warp + Session Tap)
// ==========================================
#[cfg(target_os = "macos")]
mod sys {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }

    #[repr(C)]
    #[derive(Copy, Clone)]
    pub struct CGRect {
        pub origin: CGPoint,
        pub size: CGSize,
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        // CGWarpMouseCursorPosition moves cursor directly without requiring Accessibility permissions!
        pub fn CGWarpMouseCursorPosition(new_cursor_position: CGPoint) -> i32;
        pub fn CGEventCreateMouseEvent(
            source: *mut c_void,
            mouse_type: u32,
            position: CGPoint,
            mouse_button: u32,
        ) -> *mut c_void;
        pub fn CGEventCreateScrollWheelEvent(
            source: *mut c_void,
            units: u32,
            wheel_count: u32,
            wheel1: i32,
        ) -> *mut c_void;
        pub fn CGEventCreateKeyboardEvent(
            source: *mut c_void,
            virtual_key: u16,
            key_down: bool,
        ) -> *mut c_void;
        pub fn CGEventPost(tap: u32, event: *mut c_void);
        pub fn CFRelease(cf: *mut c_void);
        pub fn CGMainDisplayID() -> u32;
        pub fn CGDisplayBounds(display: u32) -> CGRect;
    }

    pub const K_CG_SESSION_EVENT_TAP: u32 = 1;
    pub const K_CG_EVENT_LEFT_MOUSE_DOWN: u32 = 1;
    pub const K_CG_EVENT_LEFT_MOUSE_UP: u32 = 2;
    pub const K_CG_EVENT_RIGHT_MOUSE_DOWN: u32 = 3;
    pub const K_CG_EVENT_RIGHT_MOUSE_UP: u32 = 4;
    pub const K_CG_EVENT_MOUSE_MOVED: u32 = 5;
}

#[cfg(target_os = "macos")]
fn dispatch_mouse_move(x: f32, y: f32, _dx: i32, _dy: i32, _is_rel: bool) {
    unsafe {
        let display = sys::CGMainDisplayID();
        let bounds = sys::CGDisplayBounds(display);

        let pt = sys::CGPoint {
            x: (x as f64) * bounds.size.width,
            y: (y as f64) * bounds.size.height,
        };

        // 1. Move cursor directly (zero permission requirements)
        sys::CGWarpMouseCursorPosition(pt);

        // 2. Dispatch event to window server
        let ev = sys::CGEventCreateMouseEvent(
            std::ptr::null_mut(),
            sys::K_CG_EVENT_MOUSE_MOVED,
            pt,
            0,
        );
        if !ev.is_null() {
            sys::CGEventPost(sys::K_CG_SESSION_EVENT_TAP, ev);
            sys::CFRelease(ev);
        }
    }
}

#[cfg(target_os = "macos")]
fn dispatch_mouse_button(btn: u8, down: bool) {
    unsafe {
        let (ev_type, cg_btn) = match btn {
            0 => (
                if down { sys::K_CG_EVENT_LEFT_MOUSE_DOWN } else { sys::K_CG_EVENT_LEFT_MOUSE_UP },
                0,
            ),
            2 => (
                if down { sys::K_CG_EVENT_RIGHT_MOUSE_DOWN } else { sys::K_CG_EVENT_RIGHT_MOUSE_UP },
                1,
            ),
            _ => return,
        };

        let pt = sys::CGPoint { x: 0.0, y: 0.0 };
        let ev = sys::CGEventCreateMouseEvent(std::ptr::null_mut(), ev_type, pt, cg_btn);
        if !ev.is_null() {
            sys::CGEventPost(sys::K_CG_SESSION_EVENT_TAP, ev);
            sys::CFRelease(ev);
        }
    }
}

#[cfg(target_os = "macos")]
fn dispatch_mouse_wheel(_dx: i32, dy: i32) {
    unsafe {
        let ev = sys::CGEventCreateScrollWheelEvent(
            std::ptr::null_mut(),
            0,
            1,
            -dy * 3,
        );
        if !ev.is_null() {
            sys::CGEventPost(sys::K_CG_SESSION_EVENT_TAP, ev);
            sys::CFRelease(ev);
        }
    }
}

#[cfg(target_os = "macos")]
fn dispatch_key(code: u16, down: bool) {
    unsafe {
        let ev = sys::CGEventCreateKeyboardEvent(std::ptr::null_mut(), code, down);
        if !ev.is_null() {
            sys::CGEventPost(sys::K_CG_SESSION_EVENT_TAP, ev);
            sys::CFRelease(ev);
        }
    }
}

// ==========================================
// Linux / Hyprland / Wayland Input Implementation
// ==========================================
#[cfg(target_os = "linux")]
mod linux_impl {
    use crate::capture::{CURRENT_SCREEN_HEIGHT, CURRENT_SCREEN_WIDTH};
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
    use std::sync::Once;

    static MOUSE_TARGET_X: AtomicI32 = AtomicI32::new(-1);
    static MOUSE_TARGET_Y: AtomicI32 = AtomicI32::new(-1);
    static MOUSE_DIRTY: AtomicBool = AtomicBool::new(false);
    static WORKER_INIT: Once = Once::new();
    static MOVE_LOG_COUNTER: AtomicU32 = AtomicU32::new(0);

    static LAST_MONITOR_CHECK: AtomicU64 = AtomicU64::new(0);
    static CACHED_LOGICAL_W: AtomicU32 = AtomicU32::new(0);
    static CACHED_LOGICAL_H: AtomicU32 = AtomicU32::new(0);
    static CACHED_OFFSET_X: AtomicI32 = AtomicI32::new(0);
    static CACHED_OFFSET_Y: AtomicI32 = AtomicI32::new(0);

    pub fn prepare_ydotool_cmd(subcmd: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new("ydotool");
        cmd.arg(subcmd);

        if std::path::Path::new("/tmp/.ydotool_socket").exists() {
            cmd.env("YDOTOOL_SOCKET", "/tmp/.ydotool_socket");
        } else if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            let user_sock = format!("{}/.ydotool_socket", runtime_dir);
            if std::path::Path::new(&user_sock).exists() {
                cmd.env("YDOTOOL_SOCKET", user_sock);
            }
        }
        cmd
    }

    fn send_hyprland_cmd(cmd: &str) -> bool {
        use std::io::{Read, Write};
        use std::os::unix::net::UnixStream;

        let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default();
        if sig.is_empty() {
            return false;
        }
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());
        let socket_path = format!("{}/hypr/{}/.socket.sock", runtime_dir, sig);

        if let Ok(mut stream) = UnixStream::connect(&socket_path) {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(40)));
            let _ = stream.set_write_timeout(Some(std::time::Duration::from_millis(40)));
            let msg = format!("{}\n", cmd);
            if stream.write_all(msg.as_bytes()).is_ok() {
                let mut res = [0u8; 32];
                if let Ok(n) = stream.read(&mut res) {
                    let s = String::from_utf8_lossy(&res[..n]);
                    if s.trim() == "ok" || s.starts_with("ok") {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn get_active_monitor_bounds() -> (f32, f32, i32, i32) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_MONITOR_CHECK.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= 3 {
            LAST_MONITOR_CHECK.store(now, Ordering::Relaxed);
            if let Ok(output) = std::process::Command::new("hyprctl")
                .args(["monitors", "-j"])
                .output()
            {
                if output.status.success() {
                    if let Ok(monitors) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) {
                        let mon = monitors.iter().find(|m| m["focused"].as_bool().unwrap_or(false))
                            .or_else(|| monitors.first());

                        if let Some(m) = mon {
                            let w = m["width"].as_f64().unwrap_or(1920.0) as f32;
                            let h = m["height"].as_f64().unwrap_or(1080.0) as f32;
                            let s = m["scale"].as_f64().unwrap_or(1.0) as f32;
                            let scale = if s > 0.1 { s } else { 1.0 };
                            let x = m["x"].as_i64().unwrap_or(0) as i32;
                            let y = m["y"].as_i64().unwrap_or(0) as i32;
                            let logical_w = (w / scale).round() as u32;
                            let logical_h = (h / scale).round() as u32;

                            CACHED_LOGICAL_W.store(logical_w, Ordering::Relaxed);
                            CACHED_LOGICAL_H.store(logical_h, Ordering::Relaxed);
                            CACHED_OFFSET_X.store(x, Ordering::Relaxed);
                            CACHED_OFFSET_Y.store(y, Ordering::Relaxed);
                        }
                    }
                }
            }
        }

        let w = CACHED_LOGICAL_W.load(Ordering::Relaxed) as f32;
        let h = CACHED_LOGICAL_H.load(Ordering::Relaxed) as f32;
        let ox = CACHED_OFFSET_X.load(Ordering::Relaxed);
        let oy = CACHED_OFFSET_Y.load(Ordering::Relaxed);

        if w > 0.0 && h > 0.0 {
            (w, h, ox, oy)
        } else {
            (
                CURRENT_SCREEN_WIDTH.load(Ordering::Relaxed) as f32,
                CURRENT_SCREEN_HEIGHT.load(Ordering::Relaxed) as f32,
                0,
                0,
            )
        }
    }

    fn warp_cursor_position(px: i32, py: i32) {
        // 1. Direct Hyprland UNIX socket (fastest, sub-millisecond, exact logical coordinates)
        if send_hyprland_cmd(&format!("dispatch movecursor {} {}", px, py)) {
            return;
        }
        if send_hyprland_cmd(&format!("dispatch movecursor {},{}", px, py)) {
            return;
        }

        // 2. hyprctl dispatch movecursor CLI
        if let Ok(st) = std::process::Command::new("hyprctl")
            .args(["dispatch", "movecursor", &format!("{},{}", px, py)])
            .status()
        {
            if st.success() {
                return;
            }
        }
        if let Ok(st) = std::process::Command::new("hyprctl")
            .args(["dispatch", "movecursor", &px.to_string(), &py.to_string()])
            .status()
        {
            if st.success() {
                return;
            }
        }

        // 3. Fallback to ydotool
        let _ = prepare_ydotool_cmd("mousemove")
            .args(["--absolute", "-x", &px.to_string(), "-y", &py.to_string()])
            .status();
    }

    fn init_mouse_worker() {
        WORKER_INIT.call_once(|| {
            std::thread::spawn(|| {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(12));
                    if MOUSE_DIRTY.swap(false, Ordering::Acquire) {
                        let px = MOUSE_TARGET_X.load(Ordering::Relaxed);
                        let py = MOUSE_TARGET_Y.load(Ordering::Relaxed);
                        if px >= 0 && py >= 0 {
                            warp_cursor_position(px, py);
                        }
                    }
                }
            });
        });
    }

    fn flush_pending_move() {
        if MOUSE_DIRTY.swap(false, Ordering::Acquire) {
            let px = MOUSE_TARGET_X.load(Ordering::Relaxed);
            let py = MOUSE_TARGET_Y.load(Ordering::Relaxed);
            if px >= 0 && py >= 0 {
                warp_cursor_position(px, py);
            }
        }
    }

    pub fn dispatch_mouse_move(x: f32, y: f32, dx: i32, dy: i32, is_rel: bool) {
        init_mouse_worker();

        if is_rel {
            let cmd = format!("dispatch movecursor {:+} {:+}", dx, dy);
            if !send_hyprland_cmd(&cmd) {
                let _ = prepare_ydotool_cmd("mousemove")
                    .args(["-x", &dx.to_string(), "-y", &dy.to_string()])
                    .spawn();
            }
        } else {
            let (screen_w, screen_h, ox, oy) = get_active_monitor_bounds();
            let px = ox + (x * screen_w).round() as i32;
            let py = oy + (y * screen_h).round() as i32;

            MOUSE_TARGET_X.store(px, Ordering::Relaxed);
            MOUSE_TARGET_Y.store(py, Ordering::Relaxed);
            MOUSE_DIRTY.store(true, Ordering::Release);

            if MOVE_LOG_COUNTER.fetch_add(1, Ordering::Relaxed) % 60 == 0 {
                println!("[Input] Linux MouseMove: ({}, {}) on screen ({}x{}, offset: {},{})", px, py, screen_w as i32, screen_h as i32, ox, oy);
            }
        }
    }

    pub fn dispatch_mouse_button(btn: u8, down: bool) {
        // Flush pending cursor movement before clicking so the click is placed at the exact target
        flush_pending_move();

        // Standard Linux event codes: 0: Left -> 272 (BTN_LEFT), 1: Middle -> 274 (BTN_MIDDLE), 2: Right -> 273 (BTN_RIGHT)
        let keycode = match btn {
            0 => 272,
            2 => 273,
            _ => 274,
        };
        let state = if down { "1" } else { "0" };
        println!("[Input] Linux MouseButton: btn={} (keycode={}), down={}", btn, keycode, down);

        // 1. Primary: ydotool key <code:state> (supports hold, drag, select, release)
        let res = prepare_ydotool_cmd("key")
            .args([&format!("{}:{}", keycode, state)])
            .status();

        // 2. Fallback: ydotool click (if keycode emulation is unsupported on older ydotoold)
        if res.is_err() && !down {
            let hex = match btn {
                0 => "0xC0",
                2 => "0xC1",
                _ => "0xC2",
            };
            let _ = prepare_ydotool_cmd("click").args([hex]).status();
        }
    }

    pub fn dispatch_mouse_wheel(_dx: i32, dy: i32) {
        let wheel_delta = if dy > 0 { -1 } else if dy < 0 { 1 } else { 0 };
        if wheel_delta != 0 {
            let _ = prepare_ydotool_cmd("mousemove")
                .args(["-w", &wheel_delta.to_string()])
                .spawn();
        }
    }

    pub fn dispatch_key(code: u16, down: bool) {
        let state = if down { "1" } else { "0" };
        println!("[Input] Linux Key: code={}, state={}", code, state);
        let _ = prepare_ydotool_cmd("key")
            .args([&format!("{}:{}", code, state)])
            .spawn();
    }
}

#[cfg(target_os = "linux")]
use linux_impl::*;

// ==========================================
// Windows Input Stub
// ==========================================
#[cfg(target_os = "windows")]
fn dispatch_mouse_move(_x: f32, _y: f32, _dx: i32, _dy: i32, _is_rel: bool) {}
#[cfg(target_os = "windows")]
fn dispatch_mouse_button(_btn: u8, _down: bool) {}
#[cfg(target_os = "windows")]
fn dispatch_mouse_wheel(_dx: i32, _dy: i32) {}
#[cfg(target_os = "windows")]
fn dispatch_key(_code: u16, _down: bool) {}
