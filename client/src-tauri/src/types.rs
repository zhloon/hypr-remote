use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MyDeviceInfo {
    pub code: String,       // e.g. "742 819"
    pub pin: String,        // e.g. "4820"
    pub port: u16,          // e.g. 18080
    pub hostname: String,
    pub os: String,
    pub local_ip: String,   // Real local LAN IP e.g. "192.168.1.105"
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LanDevice {
    pub code: String,
    pub hostname: String,
    pub os: String,
    pub ip: String,
    pub port: u16,
    pub last_seen_ms: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
pub enum ControlMessage {
    MouseMove { x: f32, y: f32, dx: i32, dy: i32, is_rel: bool },
    MouseButton { btn: u8, down: bool },
    MouseWheel { dx: i32, dy: i32 },
    Key { code: u16, down: bool },
    Ping { ts: u64 },
    Pong { client_ts: u64 },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ConnectResult {
    pub success: bool,
    pub error: Option<String>,
    pub stream_url: Option<String>,
}
