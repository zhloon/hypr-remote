use crate::capture::capture_desktop_frame;
use crate::input::handle_control_message;
use crate::types::ControlMessage;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

pub struct EmbeddedHostServer {
    pub port: u16,
    pub pin: String,
    frame_tx: watch::Sender<Vec<u8>>,
    frame_rx: watch::Receiver<Vec<u8>>,
}

impl EmbeddedHostServer {
    pub fn new(port: u16, pin: String) -> Self {
        let (frame_tx, frame_rx) = watch::channel(Vec::new());
        Self {
            port,
            pin,
            frame_tx,
            frame_rx,
        }
    }

    pub fn start_background(self: Arc<Self>) {
        // Task 1: Dedicated High-FPS Frame Capture Engine (decoupled from network)
        let tx = self.frame_tx.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                let (frame_bytes, _) = capture_desktop_frame();
                if !frame_bytes.is_empty() {
                    let _ = tx.send(frame_bytes);
                }
                tokio::time::sleep(Duration::from_millis(15)).await;
            }
        });

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
            let frame_bytes = self.frame_rx.borrow().clone();
            let header = format!(
                "HTTP/1.1 200 OK\r\n\
                Content-Type: image/jpeg\r\n\
                Content-Length: {}\r\n\
                Cache-Control: no-cache, no-store, must-revalidate\r\n\
                Pragma: no-cache\r\n\
                Expires: 0\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Connection: close\r\n\r\n",
                frame_bytes.len()
            );
            let _ = stream.write_all(header.as_bytes()).await;
            let _ = stream.write_all(&frame_bytes).await;
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

            // Parse any input messages already received in header buffer
            if let Some(pos) = request_str.find("\r\n\r\n") {
                let initial = &request_str[pos + 4..];
                for line in initial.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        if let Ok(msg) = serde_json::from_str::<ControlMessage>(trimmed) {
                            handle_control_message(msg);
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
                    if let Ok(msg) = serde_json::from_str::<ControlMessage>(trimmed) {
                        handle_control_message(msg);
                    }
                }
                line.clear();
            }
        }
        // 4. One-off Input Control Event: POST /input
        else if method == "POST" && path == "/input" {
            if let Some(pos) = request_str.find("\r\n\r\n") {
                let body = &request_str[pos + 4..];
                if let Ok(msg) = serde_json::from_str::<ControlMessage>(body) {
                    handle_control_message(msg);
                }
            }

            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Length: 15\r\n\r\n\
                {\"status\":\"ok\"}";
            let _ = stream.write_all(response.as_bytes()).await;
        }
        // 3. Ping / Info: GET /info
        else if method == "GET" && path.starts_with("/info") {
            let response = "HTTP/1.1 200 OK\r\n\
                Content-Type: application/json\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Content-Length: 26\r\n\r\n\
                {\"status\":\"ok\",\"ready\":true}";
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
