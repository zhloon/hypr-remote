use std::io::Write;
use std::process::{Command, Stdio};

#[cfg(target_os = "linux")]
fn prepare_wayland_env(cmd: &mut Command) {
    if std::env::var("XDG_RUNTIME_DIR").is_err() {
        let uid = unsafe { libc::getuid() };
        cmd.env("XDG_RUNTIME_DIR", format!("/run/user/{}", uid));
    }
    if std::env::var("WAYLAND_DISPLAY").is_err() {
        let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
            let uid = unsafe { libc::getuid() };
            format!("/run/user/{}", uid)
        });
        if let Ok(entries) = std::fs::read_dir(&runtime_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name();
                let s = name.to_string_lossy();
                if s.starts_with("wayland-") && !s.ends_with(".lock") {
                    cmd.env("WAYLAND_DISPLAY", s.to_string());
                    break;
                }
            }
        }
    }
}

pub fn get_os_clipboard() -> String {
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

pub fn set_os_clipboard(content: &str) -> bool {
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
