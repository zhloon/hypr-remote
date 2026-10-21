use std::io::Write;
use std::process::{Command, Stdio};

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
        if let Ok(output) = Command::new("wl-paste").arg("--no-newline").output() {
            if output.status.success() {
                return String::from_utf8_lossy(&output.stdout).to_string();
            }
        }
        // 2. X11 / XWayland clipboard via xclip
        if let Ok(output) = Command::new("xclip").args(["-o", "-selection", "clipboard"]).output() {
            if output.status.success() {
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
        let wl_res = Command::new("wl-copy")
            .stdin(Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(content.as_bytes());
                }
                child.wait()
            });

        if let Ok(status) = wl_res {
            if status.success() {
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
            return child.wait().map(|s| s.success()).unwrap_or(false);
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
