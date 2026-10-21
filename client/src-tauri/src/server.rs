use crate::capture::capture_desktop_frame;
use crate::input::handle_control_message;
use crate::types::ControlMessage;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct EmbeddedHostServer {
    pub port: u16,
    pub pin: String,
}

impl EmbeddedHostServer {
    pub fn new(port: u16, pin: String) -> Self {
        Self { port, pin }
    }

    pub fn start_background(self: Arc<Self>) {
        tauri::async_runtime::spawn(async move {
            let addr = format!("0.0.0.0:{}", self.port);
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => {
                    println!("[HostServer] Running embedded host server on http://{}", addr);
                    l
                }
                Err(e) => {
                    eprintln!("[HostServer] Failed to bind TCP {}: {}", addr, e);
                    return;
                }
            };

            loop {
                if let Ok((stream, _peer)) = listener.accept().await {
                    let server = self.clone();
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

        // 1. MJPEG Screen Stream: GET /stream
        if method == "GET" && path.starts_with("/stream") {
            let response_headers = "HTTP/1.1 200 OK\r\n\
                Connection: close\r\n\
                Cache-Control: no-cache, private\r\n\
                Pragma: no-cache\r\n\
                Content-Type: multipart/x-mixed-replace; boundary=--frame\r\n\
                Access-Control-Allow-Origin: *\r\n\r\n";

            if stream.write_all(response_headers.as_bytes()).await.is_err() {
                return;
            }

            // Stream frames at ~30 FPS
            loop {
                let (frame_bytes, mime_type) = capture_desktop_frame();
                let part_header = format!(
                    "--frame\r\n\
                    Content-Type: {}\r\n\
                    Content-Length: {}\r\n\r\n",
                    mime_type,
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

                tokio::time::sleep(Duration::from_millis(33)).await;
            }
        }
        // 2. Input Control Event: POST /input
        else if method == "POST" && path == "/input" {
            // Find body after empty line
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
        // 3. CORS Preflight
        else if method == "OPTIONS" {
            let response = "HTTP/1.1 204 No Content\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
                Access-Control-Allow-Headers: *\r\n\r\n";
            let _ = stream.write_all(response.as_bytes()).await;
        }
        // 4. Ping / Info
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
