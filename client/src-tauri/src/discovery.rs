use crate::types::{LanDevice, MyDeviceInfo};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::net::UdpSocket;

#[derive(Clone, Debug)]
pub struct NetInterface {
    pub name: String,
    pub ip: String,
    pub broadcast: Option<String>,
}

#[cfg(unix)]
pub fn get_active_interfaces() -> Vec<NetInterface> {
    let mut result = Vec::new();
    unsafe {
        let mut ifaddrs_ptr: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut ifaddrs_ptr) != 0 || ifaddrs_ptr.is_null() {
            return result;
        }

        let mut curr = ifaddrs_ptr;
        while !curr.is_null() {
            let ifa = &*curr;
            let flags = ifa.ifa_flags as i32;

            let is_up = (flags & libc::IFF_UP) != 0;
            let is_running = (flags & libc::IFF_RUNNING) != 0;
            let is_loopback = (flags & libc::IFF_LOOPBACK) != 0;

            if is_up && is_running && !is_loopback && !ifa.ifa_addr.is_null() {
                let name = std::ffi::CStr::from_ptr(ifa.ifa_name).to_string_lossy().to_string();

                let is_virtual = name.starts_with("utun")
                    || name.starts_with("tun")
                    || name.starts_with("tap")
                    || name.starts_with("gif")
                    || name.starts_with("stf")
                    || name.starts_with("awdl")
                    || name.starts_with("llw")
                    || name.starts_with("docker")
                    || name.starts_with("veth");

                if !is_virtual && (*ifa.ifa_addr).sa_family == libc::AF_INET as libc::sa_family_t {
                    let sin = &*(ifa.ifa_addr as *const libc::sockaddr_in);
                    let ip = std::net::Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr));
                    let ip_str = ip.to_string();

                    let is_valid_lan = (ip.is_private() || ip_str.starts_with("192.168.") || ip_str.starts_with("10.") || ip_str.starts_with("172."))
                        && !ip_str.starts_with("198.18.")
                        && !ip_str.starts_with("169.254.")
                        && !ip_str.starts_with("127.");

                    if is_valid_lan {
                        let broadcast_str = ip_str.rfind('.').map(|idx| format!("{}.255", &ip_str[..idx]));
                        result.push(NetInterface {
                            name,
                            ip: ip_str,
                            broadcast: broadcast_str,
                        });
                    }
                }
            }
            curr = ifa.ifa_next;
        }

        libc::freeifaddrs(ifaddrs_ptr);
    }
    result
}

#[cfg(windows)]
pub fn get_active_interfaces() -> Vec<NetInterface> {
    Vec::new()
}

static LOCAL_IP_CACHE: Mutex<Option<(std::time::Instant, String)>> = Mutex::new(None);

pub fn get_local_ip() -> String {
    if let Ok(guard) = LOCAL_IP_CACHE.lock() {
        if let Some((ts, ref ip)) = *guard {
            if ts.elapsed() < Duration::from_secs(5) {
                return ip.clone();
            }
        }
    }

    let ip = get_local_ip_uncached();

    if let Ok(mut guard) = LOCAL_IP_CACHE.lock() {
        *guard = Some((std::time::Instant::now(), ip.clone()));
    }

    ip
}

fn get_local_ip_uncached() -> String {
    let ifaces = get_active_interfaces();
    // 1. Prefer 192.168.x.x
    for iface in &ifaces {
        if iface.ip.starts_with("192.168.") {
            return iface.ip.clone();
        }
    }
    // 2. Next 10.x.x.x
    for iface in &ifaces {
        if iface.ip.starts_with("10.") {
            return iface.ip.clone();
        }
    }
    // 3. Next 172.x.x.x
    for iface in &ifaces {
        if iface.ip.starts_with("172.") {
            return iface.ip.clone();
        }
    }
    if let Some(first) = ifaces.first() {
        return first.ip.clone();
    }

    // Fallback: gateway UDP probe
    for gw in ["192.168.1.1:80", "192.168.0.1:80", "192.168.31.1:80", "192.168.5.1:80", "10.0.0.1:80"] {
        if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if socket.connect(gw).is_ok() {
                if let Ok(addr) = socket.local_addr() {
                    let ip = addr.ip().to_string();
                    if !ip.starts_with("127.") && !ip.starts_with("198.18.") && ip != "0.0.0.0" {
                        return ip;
                    }
                }
            }
        }
    }
    "127.0.0.1".to_string()
}


fn create_udp_socket(port: u16) -> Option<std::net::UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    let _ = socket.set_reuse_address(true);
    #[cfg(all(unix, not(target_os = "solaris")))]
    let _ = socket.set_reuse_port(true);
    let _ = socket.set_broadcast(true);
    let _ = socket.set_nonblocking(true);

    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse().ok()?;
    socket.bind(&addr.into()).ok()?;
    Some(socket.into())
}

fn get_broadcast_targets(local_ip: &str) -> Vec<SocketAddr> {
    let mut targets = std::collections::HashSet::new();
    targets.insert("255.255.255.255:8082".parse().unwrap());

    // 1. Interfaces broadcast addresses
    for iface in get_active_interfaces() {
        if let Some(b) = iface.broadcast {
            if let Ok(addr) = format!("{}:8082", b).parse() {
                targets.insert(addr);
            }
        }
        if let Some(idx) = iface.ip.rfind('.') {
            if let Ok(addr) = format!("{}.255:8082", &iface.ip[..idx]).parse() {
                targets.insert(addr);
            }
        }
    }

    // 2. Also derive from local_ip
    if let Some(idx) = local_ip.rfind('.') {
        if let Ok(addr) = format!("{}.255:8082", &local_ip[..idx]).parse() {
            targets.insert(addr);
        }
    }

    // 3. Common fallback subnets
    for sub in &[
        "192.168.1.255:8082",
        "192.168.0.255:8082",
        "192.168.31.255:8082",
        "192.168.2.255:8082",
        "192.168.5.255:8082",
        "10.0.0.255:8082",
    ] {
        if let Ok(addr) = sub.parse() {
            targets.insert(addr);
        }
    }

    targets.into_iter().collect()
}

pub struct DiscoveryManager {
    my_info: MyDeviceInfo,
    devices: Arc<Mutex<HashMap<String, LanDevice>>>,
}

impl DiscoveryManager {
    pub fn new(port: u16) -> Self {
        let hostname = std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "Hypr-Device".to_string());

        let os = std::env::consts::OS.to_string();
        let local_ip = get_local_ip();

        // Generate distinct code per machine based on hash
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&hostname, &mut hasher);
        std::hash::Hash::hash(&os, &mut hasher);
        std::hash::Hash::hash(&SystemTime::now(), &mut hasher);
        let h = std::hash::Hasher::finish(&hasher);
        let num1 = (h % 900 + 100) as u32;
        let num2 = ((h / 1000) % 900 + 100) as u32;
        let code = format!("{} {}", num1, num2);
        let pin = format!("{:04}", (h % 9000 + 1000) as u32);

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
        info.local_ip = get_local_ip();
        info
    }

    pub fn set_port(&mut self, port: u16) {
        self.my_info.port = port;
    }

    pub fn get_devices(&self) -> Vec<LanDevice> {
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as u64;
        let mut map = self.devices.lock().unwrap();
        map.retain(|_, d| now_ms - d.last_seen_ms < 15000);
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
            let std_sock = match create_udp_socket(8082) {
                Some(s) => s,
                None => {
                    eprintln!("[Discovery] Cannot bind UDP 8082 with SO_REUSEPORT");
                    return;
                }
            };

            let socket = match UdpSocket::from_std(std_sock) {
                Ok(s) => Arc::new(s),
                Err(e) => {
                    eprintln!("[Discovery] Error wrapping socket into Tokio: {e}");
                    return;
                }
            };

            // Task 1: Periodic Subnet & Multi-Subnet Broadcast
            let s_send = socket.clone();
            let my_info = this.my_info.clone();
            tauri::async_runtime::spawn(async move {
                let clean_code = my_info.code.replace(" ", "");
                loop {
                    let cur_ip = get_local_ip();
                    let targets = get_broadcast_targets(&cur_ip);
                    let beacon_msg = format!(
                        "HYPR_BEACON:{}:{}:{}:{}:{}",
                        clean_code, my_info.pin, my_info.port, my_info.hostname, my_info.os
                    );

                    for target in &targets {
                        let _ = s_send.send_to(beacon_msg.as_bytes(), target).await;
                    }
                    let is_streaming = crate::server::H264_STREAM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
                    let sleep_ms = if is_streaming { 5000 } else { 1500 };
                    tokio::time::sleep(Duration::from_millis(sleep_ms)).await;
                }
            });

            // Task 2: Active Unicast LAN Subnet Sweep (Bypasses Router AP Isolation & Broadcast Filters!)
            let s_sweep = socket.clone();
            let my_info_sweep = this.my_info.clone();
            tauri::async_runtime::spawn(async move {
                let clean_code = my_info_sweep.code.replace(" ", "");
                loop {
                    let is_streaming = crate::server::H264_STREAM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
                    if is_streaming {
                        tokio::time::sleep(Duration::from_millis(25000)).await;
                        continue;
                    }

                    let cur_ip = get_local_ip();
                    if let Some(idx) = cur_ip.rfind('.') {
                        let prefix = &cur_ip[..=idx]; // e.g. "192.168.5."
                        let my_last_octet = cur_ip[idx + 1..].parse::<u8>().unwrap_or(0);
                        let beacon_msg = format!(
                            "HYPR_BEACON:{}:{}:{}:{}:{}",
                            clean_code, my_info_sweep.pin, my_info_sweep.port, my_info_sweep.hostname, my_info_sweep.os
                        );
                        let bytes = beacon_msg.as_bytes();

                        for host in 1..=254 {
                            if host == my_last_octet {
                                continue;
                            }
                            if let Ok(target) = format!("{}{}:8082", prefix, host).parse::<SocketAddr>() {
                                let _ = s_sweep.send_to(bytes, target).await;
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(8000)).await;
                }
            });

            // Task 3: Listen for incoming beacons & instant reply
            let mut buf = [0u8; 1024];
            let clean_my_code = this.my_info.code.replace(" ", "");

            let s_listen = socket.clone();
            let my_info_listen = this.my_info.clone();
            let devices_listen = this.devices.clone();

            tauri::async_runtime::spawn(async move {
                loop {
                    if let Ok((len, src)) = s_listen.recv_from(&mut buf).await {
                        let sender_ip = src.ip();
                        if sender_ip.is_loopback() {
                            continue;
                        }

                        if let Ok(text) = std::str::from_utf8(&buf[..len]) {
                            if text.starts_with("HYPR_BEACON:") {
                                let parts: Vec<&str> = text.split(':').collect();
                                if parts.len() >= 6 {
                                    let code = parts[1];
                                    if code != clean_my_code {
                                        let port = parts[3].parse::<u16>().unwrap_or(18080);
                                        let hostname = parts[4].to_string();
                                        let os = parts[5].to_string();
                                        let real_ip = sender_ip.to_string();

                                        let formatted_code = if code.len() == 6 {
                                            format!("{} {}", &code[..3], &code[3..])
                                        } else {
                                            code.to_string()
                                        };

                                        let now_ms = SystemTime::now()
                                            .duration_since(UNIX_EPOCH)
                                            .unwrap()
                                            .as_millis() as u64;

                                        {
                                            let mut map = devices_listen.lock().unwrap();
                                            map.insert(
                                                code.to_string(),
                                                LanDevice {
                                                    code: formatted_code,
                                                    hostname,
                                                    os,
                                                    ip: real_ip,
                                                    port,
                                                    last_seen_ms: now_ms,
                                                },
                                            );
                                        }

                                        // Send immediate unicast reply
                                        let reply_msg = format!(
                                            "HYPR_BEACON:{}:{}:{}:{}:{}",
                                            clean_my_code,
                                            my_info_listen.pin,
                                            my_info_listen.port,
                                            my_info_listen.hostname,
                                            my_info_listen.os
                                        );
                                        let reply_target = SocketAddr::new(sender_ip, 8082);
                                        let _ = s_listen.send_to(reply_msg.as_bytes(), reply_target).await;
                                    }
                                }
                            }
                        }
                    }
                }
            });

            // Task 4: Zero-Failure TCP/HTTP LAN Scanner on Port 18080 (Bypasses all router broadcast & UDP filtering!)
            let my_info_http = this.my_info.clone();
            let devices_http = this.devices.clone();
            tauri::async_runtime::spawn(async move {
                let clean_code = my_info_http.code.replace(" ", "");
                loop {
                    let is_streaming = crate::server::H264_STREAM_ACTIVE.load(std::sync::atomic::Ordering::Relaxed);
                    if is_streaming {
                        tokio::time::sleep(Duration::from_millis(30000)).await;
                        continue;
                    }

                    let cur_ip = get_local_ip();
                    if let Some(idx) = cur_ip.rfind('.') {
                        let prefix = cur_ip[..=idx].to_string(); // e.g. "192.168.5."
                        let my_last_octet = cur_ip[idx + 1..].parse::<u8>().unwrap_or(0);

                        for host in 1..=254 {
                            if host == my_last_octet {
                                continue;
                            }
                            let target_ip = format!("{}{}", prefix, host);
                            let target_addr_str = format!("{}:18080", target_ip);
                            let clean_my = clean_code.clone();
                            let devs = devices_http.clone();

                            tauri::async_runtime::spawn(async move {
                                if let Ok(addr) = target_addr_str.parse::<SocketAddr>() {
                                    if let Ok(Ok(mut stream)) = tokio::time::timeout(
                                        Duration::from_millis(350),
                                        tokio::net::TcpStream::connect(addr),
                                    )
                                    .await
                                    {
                                        let _ = stream.set_nodelay(true);
                                        let req = format!(
                                            "GET /info HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
                                            target_addr_str
                                        );
                                        use tokio::io::{AsyncReadExt, AsyncWriteExt};
                                        if stream.write_all(req.as_bytes()).await.is_ok() {
                                            let mut buf = vec![0u8; 1024];
                                            if let Ok(Ok(n)) = tokio::time::timeout(
                                                Duration::from_millis(300),
                                                stream.read(&mut buf),
                                            )
                                            .await
                                            {
                                                if n > 0 {
                                                    let text = String::from_utf8_lossy(&buf[..n]);
                                                    if let Some(pos) = text.find("\r\n\r\n") {
                                                        let body = &text[pos + 4..];
                                                        if let Ok(info) = serde_json::from_str::<MyDeviceInfo>(body) {
                                                            let clean_target = info.code.replace(" ", "");
                                                            if clean_target != clean_my && !clean_target.is_empty() {
                                                                let now_ms = SystemTime::now()
                                                                    .duration_since(UNIX_EPOCH)
                                                                    .unwrap()
                                                                    .as_millis() as u64;

                                                                let formatted_code = if clean_target.len() == 6 {
                                                                    format!("{} {}", &clean_target[..3], &clean_target[3..])
                                                                } else {
                                                                    info.code.clone()
                                                                };

                                                                let mut map = devs.lock().unwrap();
                                                                map.insert(
                                                                    clean_target,
                                                                    LanDevice {
                                                                        code: formatted_code,
                                                                        hostname: info.hostname,
                                                                        os: info.os,
                                                                        ip: target_ip,
                                                                        port: info.port,
                                                                        last_seen_ms: now_ms,
                                                                    },
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            });
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(12000)).await;
                }
            });
        });
    }
}




