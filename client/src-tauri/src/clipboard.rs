#![allow(dead_code, unused_imports, unused_variables)]
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static CLIPBOARD_CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
static WAYLAND_ENV_CACHE: Mutex<Option<(String, String)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn prepare_wayland_env(cmd: &mut Command) {
    if let Ok(guard) = WAYLAND_ENV_CACHE.lock() {
        if let Some((ref r_dir, ref w_disp)) = *guard {
            cmd.env("XDG_RUNTIME_DIR", r_dir);
            cmd.env("WAYLAND_DISPLAY", w_disp);
            return;
        }
    }

    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        let uid = unsafe { libc::getuid() };
        format!("/run/user/{}", uid)
    });
    cmd.env("XDG_RUNTIME_DIR", &runtime_dir);

    let wayland_disp = if let Ok(disp) = std::env::var("WAYLAND_DISPLAY") {
        disp
    } else {
        let mut found = "wayland-0".to_string();
        if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let s = name.to_string_lossy();
                if s.starts_with("wayland-") && !s.ends_with(".lock") {
                    found = s.to_string();
                    break;
                }
            }
        }
        found
    };
    cmd.env("WAYLAND_DISPLAY", &wayland_disp);

    if let Ok(mut guard) = WAYLAND_ENV_CACHE.lock() {
        *guard = Some((runtime_dir, wayland_disp));
    }
}

fn fetch_os_clipboard_raw() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = Command::new("pbpaste").output() {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).to_string();
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 1. Wayland clipboard via wl-paste
        let mut wl_cmd = Command::new("wl-paste");
        wl_cmd.arg("--no-newline");
        prepare_wayland_env(&mut wl_cmd);
        if let Ok(output) = wl_cmd.output() {
            if output.status.success() && !output.stdout.is_empty() {
                return String::from_utf8_lossy(&output.stdout).to_string();
            }
        }

        // 2. X11 / XWayland clipboard via xclip
        if let Ok(output) = Command::new("xclip").args(["-o", "-selection", "clipboard"]).output() {
            if output.status.success() && !output.stdout.is_empty() {
                return String::from_utf8_lossy(&output.stdout).to_string();
            }
        }

        // 3. Fallback to xsel
        if let Ok(output) = Command::new("xsel").args(["--clipboard", "--output"]).output() {
            if output.status.success() && !output.stdout.is_empty() {
                return String::from_utf8_lossy(&output.stdout).to_string();
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = Command::new("powershell").args(["-command", "Get-Clipboard"]).output() {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).trim_end().to_string();
            }
        }
    }

    String::new()
}

pub fn get_os_clipboard() -> String {
    // 1. Fast path: return cached clipboard if queried within 400ms (saves ~95% of subprocess spawns)
    if let Ok(guard) = CLIPBOARD_CACHE.lock() {
        if let Some((ts, ref text)) = *guard {
            if ts.elapsed() < Duration::from_millis(400) {
                return text.clone();
            }
        }
    }

    let text = fetch_os_clipboard_raw();

    if let Ok(mut guard) = CLIPBOARD_CACHE.lock() {
        *guard = Some((Instant::now(), text.clone()));
    }

    text
}

pub fn set_os_clipboard(content: &str) -> bool {
    // Immediately prime cache so subsequent get_os_clipboard returns without spawn
    if let Ok(mut guard) = CLIPBOARD_CACHE.lock() {
        *guard = Some((Instant::now(), content.to_string()));
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(mut child) = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(content.as_bytes());
            }
            return child.wait().map(|s| s.success()).unwrap_or(false);
        }
    }

    #[cfg(target_os = "linux")]
    {
        // 1. Wayland clipboard via wl-copy
        let mut wl_cmd = Command::new("wl-copy");
        prepare_wayland_env(&mut wl_cmd);
        wl_cmd.stdin(Stdio::piped());
        if let Ok(mut child) = wl_cmd.spawn() {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(content.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }

        // 2. X11 / XWayland clipboard via xclip
        if let Ok(mut child) = Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(content.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }

        // 3. Fallback to xsel
        if let Ok(mut child) = Command::new("xsel")
            .args(["--clipboard", "--input"])
            .stdin(Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(content.as_bytes());
            }
            if child.wait().map(|s| s.success()).unwrap_or(false) {
                return true;
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(mut child) = Command::new("powershell")
            .args(["-command", "Set-Clipboard", "-Value", content])
            .spawn()
        {
            return child.wait().map(|s| s.success()).unwrap_or(false);
        }
    }

    false
}
