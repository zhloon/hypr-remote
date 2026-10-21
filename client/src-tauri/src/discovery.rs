use crate::types::{LanDevice, MyDeviceInfo};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;

fn get_local_ip() -> String {
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "127.0.0.1".to_string()
}

pub struct DiscoveryManager {
    my_info: MyDeviceInfo,
    devices: Arc<Mutex<HashMap<String, LanDevice>>>,
}

impl DiscoveryManager {
    pub fn new(port: u16) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        let num1 = (now % 900 + 100) as u32;
        let num2 = ((now / 1000) % 900 + 100) as u32;
        let code = format!("{} {}", num1, num2);
        let pin = format!("{:04}", (now % 9000 + 1000) as u32);

        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "Hypr-Device".to_string());

        let os = std::env::consts::OS.to_string();
        let local_ip = get_local_ip();

        let my_info = MyDeviceInfo {
            code,
            pin,
            port,
            hostname,
            os,
            local_ip,
        };

        Self {
            my_info,
            devices: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get_my_info(&self) -> MyDeviceInfo {
        let mut info = self.my_info.clone();
        info.local_ip = get_local_ip(); // Refresh dynamic IP
        info
    }

    pub fn get_devices(&self) -> Vec<LanDevice> {
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let mut map = self.devices.lock().unwrap();
        map.retain(|_, d| now_ms - d.last_seen_ms < 6000);
        map.values().cloned().collect()
    }

    pub fn resolve_code(&self, target_code: &str) -> Option<(String, u16)> {
        let clean_target = target_code.replace(" ", "");
        let map = self.devices.lock().unwrap();
        for dev in map.values() {
            if dev.code.replace(" ", "") == clean_target {
                return Some((dev.ip.clone(), dev.port));
            }
        }
        None
    }

    pub fn start_background(self: Arc<Self>) {
        let this = self.clone();
        tauri::async_runtime::spawn(async move {
            let bind_addr: SocketAddr = "0.0.0.0:8082".parse().unwrap();
            let socket = match UdpSocket::bind(bind_addr).await {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    eprintln!("[Discovery] Cannot bind UDP 8082: {e}");
                    return;
                }
            };

            let _ = socket.set_broadcast(true);

            // Broadcast targets (standard + common home router subnets to bypass AP isolation)
            let broadcast_targets: Vec<SocketAddr> = vec![
                "255.255.255.255:8082".parse().unwrap(),
                "192.168.1.255:8082".parse().unwrap(),
                "192.168.0.255:8082".parse().unwrap(),
                "192.168.31.255:8082".parse().unwrap(),
                "192.168.2.255:8082".parse().unwrap(),
                "192.168.50.255:8082".parse().unwrap(),
                "10.0.0.255:8082".parse().unwrap(),
            ];

            // Task 1: Periodic broadcast across all targets
            let s_send = socket.clone();
            let my_info = this.my_info.clone();
            tauri::async_runtime::spawn(async move {
                let clean_code = my_info.code.replace(" ", "");
                loop {
                    let cur_ip = get_local_ip();
                    let beacon_msg = format!(
                        "HYPR_BEACON:{}:{}:{}:{}:{}:{}",
                        clean_code, my_info.pin, my_info.port, my_info.hostname, my_info.os, cur_ip
                    );

                    for target in &broadcast_targets {
                        let _ = s_send.send_to(beacon_msg.as_bytes(), target).await;
                    }
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                }
            });

            // Task 2: Listen for beacons
            let mut buf = [0u8; 1024];
            let clean_my_code = this.my_info.code.replace(" ", "");

            loop {
                if let Ok((len, src)) = socket.recv_from(&mut buf).await {
                    if let Ok(text) = std::str::from_utf8(&buf[..len]) {
                        if text.starts_with("HYPR_BEACON:") {
                            let parts: Vec<&str> = text.split(':').collect();
                            if parts.len() >= 6 {
                                let code = parts[1];
                                if code != clean_my_code {
                                    let port = parts[3].parse::<u16>().unwrap_or(18080);
                                    let hostname = parts[4].to_string();
                                    let os = parts[5].to_string();
                                    // Use IP declared in beacon or fallback to packet source
                                    let ip = if parts.len() >= 7 && parts[6].contains('.') {
                                        parts[6].to_string()
                                    } else {
                                        src.ip().to_string()
                                    };

                                    let formatted_code = if code.len() == 6 {
                                        format!("{} {}", &code[..3], &code[3..])
                                    } else {
                                        code.to_string()
                                    };

                                    let now_ms = SystemTime::now()
                                        .duration_since(UNIX_EPOCH)
                                        .unwrap()
                                        .as_millis() as u64;

                                    let mut map = this.devices.lock().unwrap();
                                    map.insert(
                                        code.to_string(),
                                        LanDevice {
                                            code: formatted_code,
                                            hostname,
                                            os,
                                            ip,
                                            port,
                                            last_seen_ms: now_ms,
                                        },
                                    );
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}
