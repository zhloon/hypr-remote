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
    use std::fs::{File, OpenOptions};
    use std::os::unix::io::AsRawFd;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};

    const UI_SET_EVBIT: libc::c_ulong = 1074025828;
    const UI_SET_KEYBIT: libc::c_ulong = 1074025829;
    const UI_SET_RELBIT: libc::c_ulong = 1074025830;
    const UI_SET_ABSBIT: libc::c_ulong = 1074025831;
    const UI_DEV_CREATE: libc::c_ulong = 21761;
    const UI_DEV_DESTROY: libc::c_ulong = 21762;

    const EV_SYN: u16 = 0x00;
    const EV_KEY: u16 = 0x01;
    const EV_REL: u16 = 0x02;
    const EV_ABS: u16 = 0x03;

    const SYN_REPORT: u16 = 0;

    const REL_X: u16 = 0x00;
    const REL_Y: u16 = 0x01;
    const REL_WHEEL: u16 = 0x08;
    const REL_HWHEEL: u16 = 0x06;

    const ABS_X: u16 = 0x00;
    const ABS_Y: u16 = 0x01;

    const BTN_LEFT: u16 = 272;
    const BTN_RIGHT: u16 = 273;
    const BTN_MIDDLE: u16 = 274;
    const BTN_SIDE: u16 = 275;
    const BTN_EXTRA: u16 = 276;
    const BTN_TOUCH: u16 = 330;

    const BUS_USB: u16 = 0x03;

    #[repr(C)]
    struct LinuxInputId {
        bustype: u16,
        vendor: u16,
        product: u16,
        version: u16,
    }

    #[repr(C)]
    struct LinuxUinputUserDev {
        name: [u8; 80],
        id: LinuxInputId,
        ff_effects_max: i32,
        absmax: [i32; 64],
        absmin: [i32; 64],
        absfuzz: [i32; 64],
        absflat: [i32; 64],
    }

    #[repr(C)]
    struct LinuxInputEvent {
        time: libc::timeval,
        type_: u16,
        code: u16,
        value: i32,
    }

    struct UinputDevice {
        file: File,
    }

    impl UinputDevice {
        fn open() -> Option<Self> {
            let paths = ["/dev/uinput", "/dev/input/uinput"];
            let mut file = None;
            for path in paths {
                if let Ok(f) = OpenOptions::new()
                    .read(false)
                    .write(true)
                    .open(path)
                {
                    file = Some(f);
                    break;
                }
            }

            let file = match file {
                Some(f) => f,
                None => {
                    eprintln!("[Input] /dev/uinput not accessible. Will fallback to ydotool.");
                    return None;
                }
            };

            let fd = file.as_raw_fd();
            unsafe {
                libc::ioctl(fd, UI_SET_EVBIT as _, EV_SYN as libc::c_int);
                libc::ioctl(fd, UI_SET_EVBIT as _, EV_KEY as libc::c_int);
                libc::ioctl(fd, UI_SET_EVBIT as _, EV_REL as libc::c_int);
                libc::ioctl(fd, UI_SET_EVBIT as _, EV_ABS as libc::c_int);

                libc::ioctl(fd, UI_SET_KEYBIT as _, BTN_LEFT as libc::c_int);
                libc::ioctl(fd, UI_SET_KEYBIT as _, BTN_RIGHT as libc::c_int);
                libc::ioctl(fd, UI_SET_KEYBIT as _, BTN_MIDDLE as libc::c_int);
                libc::ioctl(fd, UI_SET_KEYBIT as _, BTN_SIDE as libc::c_int);
                libc::ioctl(fd, UI_SET_KEYBIT as _, BTN_EXTRA as libc::c_int);
                libc::ioctl(fd, UI_SET_KEYBIT as _, BTN_TOUCH as libc::c_int);

                for code in 1..256 {
                    libc::ioctl(fd, UI_SET_KEYBIT as _, code as libc::c_int);
                }

                libc::ioctl(fd, UI_SET_RELBIT as _, REL_X as libc::c_int);
                libc::ioctl(fd, UI_SET_RELBIT as _, REL_Y as libc::c_int);
                libc::ioctl(fd, UI_SET_RELBIT as _, REL_WHEEL as libc::c_int);
                libc::ioctl(fd, UI_SET_RELBIT as _, REL_HWHEEL as libc::c_int);

                libc::ioctl(fd, UI_SET_ABSBIT as _, ABS_X as libc::c_int);
                libc::ioctl(fd, UI_SET_ABSBIT as _, ABS_Y as libc::c_int);

                let mut uidev: LinuxUinputUserDev = std::mem::zeroed();
                let name = b"HyprRemote-Virtual-Device";
                uidev.name[..name.len()].copy_from_slice(name);
                uidev.id.bustype = BUS_USB;
                uidev.id.vendor = 0x1234;
                uidev.id.product = 0x5678;
                uidev.id.version = 1;

                uidev.absmin[ABS_X as usize] = 0;
                uidev.absmax[ABS_X as usize] = 65535;
                uidev.absmin[ABS_Y as usize] = 0;
                uidev.absmax[ABS_Y as usize] = 65535;

                let dev_bytes = std::slice::from_raw_parts(
                    &uidev as *const _ as *const u8,
                    std::mem::size_of::<LinuxUinputUserDev>(),
                );
                if libc::write(fd, dev_bytes.as_ptr() as *const libc::c_void, dev_bytes.len()) < 0 {
                    eprintln!("[Input] Failed to write uinput_user_dev structure");
                    return None;
                }

                if libc::ioctl(fd, UI_DEV_CREATE as _) < 0 {
                    eprintln!("[Input] Failed UI_DEV_CREATE ioctl");
                    return None;
                }
            }

            println!("[Input] Successfully created native /dev/uinput virtual absolute device (0..65535)!");
            Some(Self { file })
        }

        fn write_event(&self, type_: u16, code: u16, value: i32) {
            let fd = self.file.as_raw_fd();
            let ev = LinuxInputEvent {
                time: libc::timeval { tv_sec: 0, tv_usec: 0 },
                type_,
                code,
                value,
            };
            unsafe {
                let bytes = std::slice::from_raw_parts(
                    &ev as *const _ as *const u8,
                    std::mem::size_of::<LinuxInputEvent>(),
                );
                libc::write(fd, bytes.as_ptr() as *const libc::c_void, bytes.len());
            }
        }

        fn syn(&self) {
            self.write_event(EV_SYN, SYN_REPORT, 0);
        }
    }

    impl Drop for UinputDevice {
        fn drop(&mut self) {
            unsafe {
                libc::ioctl(self.file.as_raw_fd(), UI_DEV_DESTROY as _);
            }
        }
    }

    static UINPUT: Mutex<Option<UinputDevice>> = Mutex::new(None);
    static UINPUT_INITIALIZED: AtomicBool = AtomicBool::new(false);
    static MOVE_LOG_COUNTER: AtomicU32 = AtomicU32::new(0);

    fn get_uinput() -> std::sync::MutexGuard<'static, Option<UinputDevice>> {
        let mut guard = match UINPUT.lock() {
            Ok(g) => g,
            Err(poisoned) => poisoned.into_inner(),
        };
        if !UINPUT_INITIALIZED.swap(true, Ordering::SeqCst) {
            let res = std::panic::catch_unwind(|| {
                UinputDevice::open()
            });
            *guard = res.unwrap_or(None);
        }
        guard
    }

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

    fn get_active_monitor_bounds() -> (f32, f32, i32, i32) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let last = LAST_MONITOR_CHECK.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= 2 {
            LAST_MONITOR_CHECK.store(now, Ordering::Relaxed);
            if let Ok(output) = std::process::Command::new("hyprctl")
                .args(["monitors", "-j"])
                .output()
            {
                if output.status.success() {
                    if let Ok(monitors) = serde_json::from_slice::<Vec<serde_json::Value>>(&output.stdout) {
                        let active_name = crate::capture::ACTIVE_MONITOR_NAME.lock().ok().and_then(|g| g.clone());
                        let mon = if let Some(ref target) = active_name {
                            monitors.iter().find(|m| m["name"].as_str() == Some(target.as_str()))
                                .or_else(|| monitors.iter().find(|m| m["focused"].as_bool().unwrap_or(false)))
                                .or_else(|| monitors.first())
                        } else {
                            monitors.iter().find(|m| m["focused"].as_bool().unwrap_or(false))
                                .or_else(|| monitors.first())
                        };

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

    pub fn dispatch_mouse_move(x: f32, y: f32, dx: i32, dy: i32, is_rel: bool) {
        // Priority 1: Native Linux /dev/uinput absolute or relative pointer (< 0.05ms)
        {
            let uinput_guard = get_uinput();
            if let Some(ref dev) = *uinput_guard {
                if is_rel {
                    dev.write_event(EV_REL, REL_X, dx);
                    dev.write_event(EV_REL, REL_Y, dy);
                    dev.syn();
                } else {
                    let abs_x = (x.clamp(0.0, 1.0) * 65535.0).round() as i32;
                    let abs_y = (y.clamp(0.0, 1.0) * 65535.0).round() as i32;
                    dev.write_event(EV_ABS, ABS_X, abs_x);
                    dev.write_event(EV_ABS, ABS_Y, abs_y);
                    dev.syn();
                    if MOVE_LOG_COUNTER.fetch_add(1, Ordering::Relaxed) % 60 == 0 {
                        println!("[Input] UInput ABS: ({}, {}) norm=({:.4}, {:.4})", abs_x, abs_y, x, y);
                    }
                }
                return;
            }
        }

        // Priority 2: Coalesced fallback worker via ydotool / hyprctl
        submit_coalesced_mouse_move(x, y, dx, dy, is_rel);
    }

    static PENDING_MOVE: Mutex<Option<(f32, f32, i32, i32, bool)>> = Mutex::new(None);
    static WORKER_RUNNING: AtomicBool = AtomicBool::new(false);

    fn submit_coalesced_mouse_move(x: f32, y: f32, dx: i32, dy: i32, is_rel: bool) {
        {
            let mut lock = match PENDING_MOVE.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            *lock = Some((x, y, dx, dy, is_rel));
        }

        if !WORKER_RUNNING.swap(true, Ordering::SeqCst) {
            std::thread::Builder::new()
                .name("ydotool-coalescer".into())
                .spawn(move || {
                    loop {
                        let target = {
                            let mut lock = match PENDING_MOVE.lock() {
                                Ok(g) => g,
                                Err(p) => p.into_inner(),
                            };
                            lock.take()
                        };

                        if let Some((x, y, dx, dy, is_rel)) = target {
                            if is_rel {
                                let _ = prepare_ydotool_cmd("mousemove")
                                    .args(["--", &dx.to_string(), &dy.to_string()])
                                    .status();
                            } else {
                                let (screen_w, screen_h, ox, oy) = get_active_monitor_bounds();
                                let px = ox + (x * screen_w).round() as i32;
                                let py = oy + (y * screen_h).round() as i32;
                                let ydo_res = prepare_ydotool_cmd("mousemove")
                                    .args(["-a", &px.to_string(), &py.to_string()])
                                    .status();
                                if ydo_res.is_err() || !ydo_res.unwrap().success() {
                                    let _ = std::process::Command::new("hyprctl")
                                        .args(["dispatch", "movecursor", &format!("{},{}", px, py)])
                                        .status();
                                }
                            }
                        } else {
                            WORKER_RUNNING.store(false, Ordering::SeqCst);
                            break;
                        }
                    }
                })
                .expect("failed to spawn ydotool coalescer");
        }
    }

    pub fn dispatch_mouse_button(btn: u8, down: bool) {
        let ev_code = match btn {
            0 => BTN_LEFT,
            1 => BTN_MIDDLE,
            2 => BTN_RIGHT,
            3 => BTN_SIDE,
            _ => BTN_EXTRA,
        };

        // 1. Native Linux /dev/uinput button injection
        {
            let uinput_guard = get_uinput();
            if let Some(ref dev) = *uinput_guard {
                dev.write_event(EV_KEY, ev_code, if down { 1 } else { 0 });
                dev.syn();
                return;
            }
        }

        // 2. Fallback to ydotool
        let keycode = match btn {
            0 => 272,
            2 => 273,
            _ => 274,
        };
        let state = if down { "1" } else { "0" };
        let _ = prepare_ydotool_cmd("key")
            .args([&format!("{}:{}", keycode, state)])
            .status();
    }

    pub fn dispatch_mouse_wheel(_dx: i32, dy: i32) {
        // 1. Native Linux /dev/uinput wheel injection
        {
            let uinput_guard = get_uinput();
            if let Some(ref dev) = *uinput_guard {
                dev.write_event(EV_REL, REL_WHEEL, dy);
                dev.syn();
                return;
            }
        }

        // 2. Fallback to ydotool
        let wheel_delta = if dy > 0 { -1 } else if dy < 0 { 1 } else { 0 };
        if wheel_delta != 0 {
            let _ = prepare_ydotool_cmd("mousemove")
                .args(["-w", &wheel_delta.to_string()])
                .spawn();
        }
    }

    pub fn dispatch_key(code: u16, down: bool) {
        // 1. Native Linux /dev/uinput keyboard key injection
        {
            let uinput_guard = get_uinput();
            if let Some(ref dev) = *uinput_guard {
                dev.write_event(EV_KEY, code, if down { 1 } else { 0 });
                dev.syn();
                return;
            }
        }

        // 2. Fallback to ydotool
        let state = if down { "1" } else { "0" };
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
