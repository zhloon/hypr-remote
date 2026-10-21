#![allow(unused_imports, dead_code)]
use crate::capture::capture_desktop_frame;
use crate::input::handle_control_message;
use crate::types::ControlMessage;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

use crate::types::MyDeviceInfo;

pub static LAST_CLIENT_FRAME_TIME: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static H264_STREAM_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub struct EmbeddedHostServer {
    pub port: u16,
    pub pin: String,
    pub my_info: MyDeviceInfo,
    frame_tx: watch::Sender<Arc<Vec<u8>>>,
    frame_rx: watch::Receiver<Arc<Vec<u8>>>,
}

impl EmbeddedHostServer {
    pub fn new(port: u16, my_info: MyDeviceInfo) -> Self {
        let (frame_tx, frame_rx) = watch::channel(Arc::new(Vec::new()));
        let pin = my_info.pin.clone();
        Self {
            port,
            pin,
            my_info,
            frame_tx,
            frame_rx,
        }
    }

    pub fn start_background(self: Arc<Self>) {
        // Task 1: Dedicated High-FPS Frame Capture Engine (Linux host only)
        #[cfg(target_os = "linux")]
        {
            let tx = self.frame_tx.clone();
            std::thread::Builder::new()
                .name("capture-engine".into())
                .spawn(move || {
                    let target_interval = Duration::from_millis(8); // Target ~120 FPS
                    loop {
                        // If GPU H.264 stream is running, completely sleep this thread! Zero CPU usage!
                        if H264_STREAM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed) {
                            std::thread::sleep(Duration::from_millis(300));
                            continue;
                        }

                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let last = LAST_CLIENT_FRAME_TIME.load(std::sync::atomic::Ordering::Relaxed);
                        if tx.receiver_count() > 1 || now.saturating_sub(last) < 2 {
                            let start = std::time::Instant::now();
                            let (frame_bytes, _) = capture_desktop_frame();
                            if !frame_bytes.is_empty() {
                                let _ = tx.send(Arc::new(frame_bytes));
                            }
                            let elapsed = start.elapsed();
                            if elapsed < target_interval {
                                std::thread::sleep(target_interval - elapsed);
                            } else {
                                std::thread::sleep(Duration::from_millis(1));
                            }
                        } else {
                            // No client active on MJPEG/JPEG stream: idle at low frequency
                            std::thread::sleep(Duration::from_millis(250));
                        }
                    }
                })
                .expect("failed to spawn capture engine thread");
        }

        // Task 2: High-Performance TCP Streaming Server with Port Fallback
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut bound_listener = None;

            for p in this.port..this.port + 10 {
                let addr = format!("0.0.0.0:{}", p);
                match TcpListener::bind(&addr).await {
                    Ok(l) => {
                        println!("[HostServer] Running embedded host server on http://{}", addr);
                        bound_listener = Some(l);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[HostServer] Port {} unavailable ({}), trying next...", p, e);
                    }
                }
            }

            let listener = match bound_listener {
                Some(l) => l,
                None => {
                    eprintln!("[HostServer] FATAL: Could not bind any TCP port between 18080-18089!");
                    return;
                }
            };

            loop {
                if let Ok((stream, _peer)) = listener.accept().await {
                    let server = this.clone();
                    tauri::async_runtime::spawn(async move {
                        server.handle_connection(stream).await;
                    });
                }
            }
        });
    }

    async fn handle_connection(&self, mut stream: TcpStream) {
        let _ = stream.set_nodelay(true);
        let sock_ref = socket2::SockRef::from(&stream);
        let _ = sock_ref.set_send_buffer_size(524288);
        let _ = sock_ref.set_recv_buffer_size(131072);
        let mut buf = [0u8; 4096];
        let bytes_read = match stream.read(&mut buf).await {
            Ok(n) if n > 0 => n,
            _ => return,
        };

        let request_str = String::from_utf8_lossy(&buf[..bytes_read]);
        let mut lines = request_str.lines();
        let request_line = match lines.next() {
            Some(line) => line,
            None => return,
        };

        let parts: Vec<&str> = request_line.split_whitespace().collect();
        if parts.len() < 2 {
            return;
        }

        let method = parts[0];
        let path = parts[1];

        // 1. High-Speed Single Frame (Zero Buffer Bloat for GPU Canvas): GET /frame
        if method == "GET" && path.starts_with("/frame") {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            LAST_CLIENT_FRAME_TIME.store(now, std::sync::atomic::Ordering::Relaxed);

            let frame_bytes = self.frame_rx.borrow().clone();
            let data = if frame_bytes.is_empty() {
                let (bytes, _) = capture_desktop_frame();
                Arc::new(bytes)
            } else {
                frame_bytes
            };
            let header = format!(
                "HTTP/1.1 200 OK\r\n\
                Content-Type: image/jpeg\r\n\
                Content-Length: {}\r\n\
                Cache-Control: no-cache, no-store, must-revalidate\r\n\
                Pragma: no-cache\r\n\
                Expires: 0\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Connection: close\r\n\r\n",
                data.len()
            );
            let _ = stream.write_all(header.as_bytes()).await;
            let _ = stream.write_all(&data).await;
        }
        // 2. High-FPS MJPEG Screen Stream: GET /stream
        else if method == "GET" && path.starts_with("/stream") {
            let response_headers = "HTTP/1.1 200 OK\r\n\
                Connection: close\r\n\
                Cache-Control: no-cache, no-store, must-revalidate\r\n\
                Pragma: no-cache\r\n\
                Expires: 0\r\n\
                Content-Type: multipart/x-mixed-replace; boundary=hyprframe\r\n\
                Access-Control-Allow-Origin: *\r\n\r\n";

            if stream.write_all(response_headers.as_bytes()).await.is_err() {
                return;
            }

            let mut rx = self.frame_rx.clone();

            loop {
                if rx.changed().await.is_err() {
                    break;
                }

                let frame_bytes = rx.borrow_and_update().clone();
                if frame_bytes.is_empty() {
                    continue;
                }

                let part_header = format!(
                    "--hyprframe\r\n\
                    Content-Type: image/jpeg\r\n\
                    Content-Length: {}\r\n\r\n",
                    frame_bytes.len()
                );

                if stream.write_all(part_header.as_bytes()).await.is_err() {
                    break;
                }
                if stream.write_all(&frame_bytes).await.is_err() {
                    break;
                }
                if stream.write_all(b"\r\n").await.is_err() {
                    break;
                }
            }
        }
        // 3. Persistent Input Stream (0ms latency, persistent TCP pipe): POST /input_stream
        else if method == "POST" && path.starts_with("/input_stream") {
            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/plain\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Connection: keep-alive\r\n\r\n";
            if stream.write_all(response.as_bytes()).await.is_err() {
                return;
            }

            println!("[HostServer] Client connected to persistent /input_stream");

            // Parse any input messages already received in header buffer
            if let Some(pos) = request_str.find("\r\n\r\n") {
                let initial = &request_str[pos + 4..];
                for line in initial.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        match serde_json::from_str::<ControlMessage>(trimmed) {
                            Ok(msg) => handle_control_message(msg),
                            Err(e) => eprintln!("[HostServer] Initial input parse error: {} on '{}'", e, trimmed),
                        }
                    }
                }
            }

            use tokio::io::AsyncBufReadExt;
            let mut reader = tokio::io::BufReader::new(stream);
            let mut line = String::new();
            while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    match serde_json::from_str::<ControlMessage>(trimmed) {
                        Ok(msg) => handle_control_message(msg),
                        Err(e) => eprintln!("[HostServer] Stream input parse error: {} on '{}'", e, trimmed),
                    }
                }
                line.clear();
            }
            println!("[HostServer] Client disconnected from persistent /input_stream");
        }
        // 4. One-off Input Control Event: POST /input
        else if method == "POST" && path == "/input" {
            if let Some(pos) = request_str.find("\r\n\r\n") {
                let body = &request_str[pos + 4..];
                match serde_json::from_str::<ControlMessage>(body) {
                    Ok(msg) => handle_control_message(msg),
                    Err(e) => eprintln!("[HostServer] POST /input parse error: {} on '{}'", e, body),
                }
            }

            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Length: 15\r\n\r\n\
                {\"status\":\"ok\"}";
            let _ = stream.write_all(response.as_bytes()).await;
        }
        // 5. Dynamic Quality / FPS Adjustment: GET or POST /quality?mode=crystal|performance|balanced
        else if (method == "GET" || method == "POST") && path.starts_with("/quality") {
            let mode = if path.contains("mode=crystal") {
                "crystal"
            } else if path.contains("mode=performance") {
                "performance"
            } else {
                "balanced"
            };
            crate::capture::set_quality_mode(mode);
            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Length: 15\r\n\r\n\
                {\"status\":\"ok\"}";
            let _ = stream.write_all(response.as_bytes()).await;
        }
        // 5.1 Dynamic Host Resolution Adjustment: GET or POST /set_resolution?res=2560x1440|1920x1080|preferred
        else if (method == "GET" || method == "POST") && path.starts_with("/set_resolution") {
            let res = if path.contains("res=2560x1440") {
                "2560x1440"
            } else if path.contains("res=1920x1080") {
                "1920x1080"
            } else if path.contains("res=3840x2160") {
                "3840x2160"
            } else if let Some(idx) = path.find("res=") {
                let rest = &path[idx + 4..];
                rest.split('&').next().unwrap_or("preferred")
            } else {
                "preferred"
            };
            let ok = crate::capture::set_screen_resolution(res);
            let body = format!("{{\"status\":\"{}\",\"resolution\":\"{}\"}}", if ok { "ok" } else { "failed" }, res);
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
        // 5.2 Bi-directional Shared Clipboard: GET or POST /clipboard
        else if method == "GET" && path.starts_with("/clipboard") {
            let content = crate::clipboard::get_os_clipboard();
            let body = serde_json::json!({ "content": content }).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json; charset=utf-8\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                Access-Control-Allow-Headers: *\r\n\
                Content-Length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
        else if method == "POST" && path.starts_with("/clipboard") {
            let mut expected_len = 0;
            for line in request_str.lines() {
                if let Some(val) = line.to_lowercase().strip_prefix("content-length:") {
                    if let Ok(l) = val.trim().parse::<usize>() {
                        expected_len = l;
                    }
                }
            }

            let mut body_bytes = Vec::new();
            if let Some(pos) = request_str.find("\r\n\r\n") {
                let initial_body = &buf[pos + 4..bytes_read];
                body_bytes.extend_from_slice(initial_body);
            }

            while body_bytes.len() < expected_len && body_bytes.len() < 2 * 1024 * 1024 {
                let mut temp = [0u8; 4096];
                match stream.read(&mut temp).await {
                    Ok(n) if n > 0 => body_bytes.extend_from_slice(&temp[..n]),
                    _ => break,
                }
            }

            let body_str = String::from_utf8_lossy(&body_bytes);
            let text_to_set = if let Ok(val) = serde_json::from_str::<serde_json::Value>(&body_str) {
                val["content"].as_str().map(|s| s.to_string()).unwrap_or_else(|| body_str.to_string())
            } else {
                body_str.to_string()
            };

            if !text_to_set.is_empty() {
                crate::clipboard::set_os_clipboard(&text_to_set);
            }

            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json; charset=utf-8\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                Access-Control-Allow-Headers: *\r\n\
                Content-Length: 15\r\n\r\n\
                {\"status\":\"ok\"}";
            let _ = stream.write_all(response.as_bytes()).await;
        }
        // 6. Ultra-Low-Latency Hardware H.264 Video Stream: GET /h264_stream
        else if method == "GET" && path.starts_with("/h264_stream") {
            H264_STREAM_ACTIVE.store(true, std::sync::atomic::Ordering::Relaxed);
            struct H264ActiveGuard {
                child_pid: std::sync::Arc<std::sync::atomic::AtomicU32>,
            }
            impl Drop for H264ActiveGuard {
                fn drop(&mut self) {
                    H264_STREAM_ACTIVE.store(false, std::sync::atomic::Ordering::Relaxed);
                    let pid = self.child_pid.load(std::sync::atomic::Ordering::Relaxed);
                    #[cfg(unix)]
                    if pid > 0 {
                        unsafe {
                            libc::kill(-(pid as i32), libc::SIGKILL);
                            libc::kill(pid as i32, libc::SIGKILL);
                        }
                    }
                    let _ = std::process::Command::new("pkill").args(["-9", "-f", "wf-recorder.*mpegts"]).status();
                    let _ = std::process::Command::new("killall").args(["-9", "wf-recorder"]).status();
                    println!("[H264] Cleaned up recorder process (PID: {}) and all children", pid);
                }
            }
            let child_pid_arc = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
            let _h264_guard = H264ActiveGuard { child_pid: child_pid_arc.clone() };

            let mut child = match crate::capture::spawn_h264_recorder_process() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[H264] Failed to spawn H.264 recorder: {}", e);
                    let response = "HTTP/1.1 503 Service Unavailable\r\n\
                        Content-Type: text/plain\r\n\
                        Access-Control-Allow-Origin: *\r\n\
                        Content-Length: 29\r\n\r\n\
                        H.264 recorder not available";
                    let _ = stream.write_all(response.as_bytes()).await;
                    return;
                }
            };

            if let Some(pid) = child.id() {
                child_pid_arc.store(pid, std::sync::atomic::Ordering::Relaxed);
            }

            let mut stdout = match child.stdout.take() {
                Some(s) => s,
                None => return,
            };

            let _ = stream.set_nodelay(true);

            // Pre-read first fragment with 7s timeout to guarantee encoder is producing valid data!
            let mut first_chunk = [0u8; 8192];
            let first_bytes = match tokio::time::timeout(
                Duration::from_millis(7000),
                stdout.read(&mut first_chunk),
            ).await {
                Ok(Ok(n)) if n > 0 => n,
                _ => {
                    eprintln!("[H264] Recorder did not produce data in time, signaling fallback...");
                    let _ = child.kill().await;
                    let response = "HTTP/1.1 503 Service Unavailable\r\n\
                        Content-Type: text/plain\r\n\
                        Access-Control-Allow-Origin: *\r\n\
                        Content-Length: 26\r\n\r\n\
                        Encoder output unavailable";
                    let _ = stream.write_all(response.as_bytes()).await;
                    return;
                }
            };

            let header = "HTTP/1.1 200 OK\r\n\
                Content-Type: video/mp4\r\n\
                Connection: close\r\n\
                Cache-Control: no-cache, no-store, must-revalidate\r\n\
                Pragma: no-cache\r\n\
                Expires: 0\r\n\
                Access-Control-Allow-Origin: *\r\n\r\n";

            if stream.write_all(header.as_bytes()).await.is_err() {
                let _ = child.kill().await;
                return;
            }

            if stream.write_all(&first_chunk[..first_bytes]).await.is_err() {
                let _ = child.kill().await;
                return;
            }

            // Pipe remaining encoder stdout directly into client TCP socket with zero-overhead 64KB buffer
            let mut stream_buf = vec![0u8; 65536];
            loop {
                match stdout.read(&mut stream_buf).await {
                    Ok(0) => break,
                    Ok(n) => {
                        if stream.write_all(&stream_buf[..n]).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }

            let _ = child.kill().await;
            println!("[H264] Client disconnected, stopped H.264 recorder");
        }
        // 7. Ping / Info: GET /info
        else if method == "GET" && path.starts_with("/info") {
            let mut info = self.my_info.clone();
            info.local_ip = crate::discovery::get_local_ip();
            let json = serde_json::to_string(&info).unwrap_or_default();
            let response = format!(
                "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Length: {}\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
        // 4. CORS Preflight
        else if method == "OPTIONS" {
            let response = "HTTP/1.1 204 No Content\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                Access-Control-Allow-Headers: *\r\n\r\n";
            let _ = stream.write_all(response.as_bytes()).await;
        }
        else {
            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: text/plain\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Length: 16\r\n\r\n\
                HyprRemote Ready";
            let _ = stream.write_all(response.as_bytes()).await;
        }
    }
}
