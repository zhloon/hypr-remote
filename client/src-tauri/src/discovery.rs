use crate::types::{LanDevice, MyDeviceInfo};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;

pub struct DiscoveryManager {
    my_info: MyDeviceInfo,
    devices: Arc<Mutex<HashMap<String, LanDevice>>>,
}

impl DiscoveryManager {
    pub fn new(port: u16) -> Self {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis();
        // Generate random 6-digit code (e.g., 742 819)
        let num1 = (now % 900 + 100) as u32;
        let num2 = ((now / 1000) % 900 + 100) as u32;
        let code = format!("{} {}", num1, num2);
        let pin = format!("{:04}", (now % 9000 + 1000) as u32);

        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "Hypr-Device".to_string());

        let os = std::env::consts::OS.to_string();

        let my_info = MyDeviceInfo {
            code,
            pin,
            port,
            hostname,
            os,
        };

        Self {
            my_info,
            devices: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn get_my_info(&self) -> MyDeviceInfo {
        self.my_info.clone()
    }

    pub fn get_devices(&self) -> Vec<LanDevice> {
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let mut map = self.devices.lock().unwrap();
        // Remove stale devices (> 6 seconds without beacon)
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

            // Task 1: Periodic broadcast
            let s_send = socket.clone();
            let my_info = this.my_info.clone();
            tauri::async_runtime::spawn(async move {
                let clean_code = my_info.code.replace(" ", "");
                let beacon_msg = format!(
                    "HYPR_BEACON:{}:{}:{}:{}:{}",
                    clean_code, my_info.pin, my_info.port, my_info.hostname, my_info.os
                );
                let target: SocketAddr = "255.255.255.255:8082".parse().unwrap();

                loop {
                    let _ = s_send.send_to(beacon_msg.as_bytes(), target).await;
                    tokio::time::sleep(Duration::from_millis(1800)).await;
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
                                    // Found other device
                                    let port = parts[3].parse::<u16>().unwrap_or(18080);
                                    let hostname = parts[4].to_string();
                                    let os = parts[5].to_string();
                                    let ip = src.ip().to_string();

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
