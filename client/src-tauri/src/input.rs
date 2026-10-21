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
// Linux / Hyprland Input Implementation
// ==========================================
#[cfg(target_os = "linux")]
fn send_hyprland_cmd(cmd: &str) -> bool {
    use std::io::Write;
    use std::os::unix::net::UnixStream;

    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").unwrap_or_default();
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/run/user/1000".to_string());

    let socket_path = format!("{}/hypr/{}/.socket.sock", runtime_dir, sig);
    if let Ok(mut stream) = UnixStream::connect(&socket_path) {
        let msg = format!("[[/]]{}\n", cmd);
        if stream.write_all(msg.as_bytes()).is_ok() {
            return true;
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn dispatch_mouse_move(x: f32, y: f32, dx: i32, dy: i32, is_rel: bool) {
    if is_rel {
        let cmd = format!("dispatch movecursor {:+} {:+}", dx, dy);
        if !send_hyprland_cmd(&cmd) {
            let _ = std::process::Command::new("hyprctl")
                .args(["dispatch", "movecursor", &format!("{:+}", dx), &format!("{:+}", dy)])
                .spawn();
        }
    } else {
        let screen_w = CURRENT_SCREEN_WIDTH.load(Ordering::Relaxed) as f32;
        let screen_h = CURRENT_SCREEN_HEIGHT.load(Ordering::Relaxed) as f32;
        let px = (x * screen_w) as i32;
        let py = (y * screen_h) as i32;

        let cmd = format!("dispatch movecursor {} {}", px, py);
        if !send_hyprland_cmd(&cmd) {
            let _ = std::process::Command::new("hyprctl")
                .args(["dispatch", "movecursor", &px.to_string(), &py.to_string()])
                .spawn();
        }
    }
}

#[cfg(target_os = "linux")]
fn dispatch_mouse_button(btn: u8, down: bool) {
    let btn_code = match btn {
        0 => "0xC0", // Left click
        2 => "0xC1", // Right click
        _ => "0xC2", // Middle
    };
    let _ = std::process::Command::new("ydotool")
        .args(["click", &format!("{}{}", btn_code, if down { "1" } else { "0" })])
        .spawn();
}

#[cfg(target_os = "linux")]
fn dispatch_mouse_wheel(_dx: i32, dy: i32) {
    let _ = std::process::Command::new("ydotool")
        .args(["mousemove", "-w", &(-dy).to_string()])
        .spawn();
}

#[cfg(target_os = "linux")]
fn dispatch_key(code: u16, down: bool) {
    let state = if down { "1" } else { "0" };
    let _ = std::process::Command::new("ydotool")
        .args(["key", &format!("{}:{}", code, state)])
        .spawn();
}

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
