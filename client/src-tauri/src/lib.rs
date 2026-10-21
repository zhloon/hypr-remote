pub mod capture;
pub mod discovery;
pub mod input;
pub mod server;
pub mod types;

use discovery::DiscoveryManager;
use server::EmbeddedHostServer;
use std::sync::Arc;
#[allow(unused_imports)]
use tauri::menu::{Menu, MenuItem};
#[allow(unused_imports)]
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
#[allow(unused_imports)]
use tauri::{AppHandle, Manager, State, WindowEvent};
use types::{ConnectResult, ControlMessage, LanDevice, MyDeviceInfo};

use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::Mutex;

struct AppState {
    discovery: Arc<DiscoveryManager>,
    _server: Arc<EmbeddedHostServer>,
    input_channel: Arc<Mutex<Option<(String, u16, UnboundedSender<ControlMessage>)>>>,
}

#[tauri::command]
fn get_my_device_info(state: State<AppState>) -> MyDeviceInfo {
    state.discovery.get_my_info()
}

#[tauri::command]
fn get_lan_devices(state: State<AppState>) -> Vec<LanDevice> {
    state.discovery.get_devices()
}

#[tauri::command]
async fn resolve_connection_code(
    state: State<'_, AppState>,
    code: String,
) -> Result<ConnectResult, String> {
    let clean = code.trim().replace(" ", "");

    // 1. Direct IP entry: e.g. "192.168.1.100" or "http://192.168.1.100:18080"
    let mut clean_target = code.trim().replace("http://", "").replace("https://", "");
    if let Some(pos) = clean_target.find('/') {
        clean_target = clean_target[..pos].to_string();
    }
    if clean_target.contains('.') {
        let target = if clean_target.contains(':') {
            clean_target
        } else {
            format!("{}:18080", clean_target)
        };
        return Ok(ConnectResult {
            success: true,
            error: None,
            stream_url: Some(format!("http://{}/stream", target)),
        });
    }

    // 2. Prevent accidental self-connection
    let my_info = state.discovery.get_my_info();
    if clean == my_info.code.replace(" ", "") {
        return Ok(ConnectResult {
            success: false,
            error: Some("⚠️ 您输入的是【本机连接码】！\n若要控制其他设备，请输入对方屏幕上显示的连接码或对方的局域网 IP。".to_string()),
            stream_url: None,
        });
    }

    // 3. Query LAN discovered devices
    if let Some((ip, port)) = state.discovery.resolve_code(&code) {
        return Ok(ConnectResult {
            success: true,
            error: None,
            stream_url: Some(format!("http://{}:{}/stream", ip, port)),
        });
    }

    Ok(ConnectResult {
        success: false,
        error: Some("未在局域网内扫描到该连接码。\n\n💡 解决方案：请直接在输入框中输入对方屏幕左侧显示的【局域网 IP】(例如 192.168.x.x) 发起直连！".to_string()),
        stream_url: None,
    })
}

#[tauri::command]
fn toggle_fullscreen(app: AppHandle, fullscreen: Option<bool>) -> Result<bool, String> {
    if let Some(window) = app.get_webview_window("main") {
        let target = match fullscreen {
            Some(f) => f,
            None => !window.is_fullscreen().unwrap_or(false),
        };
        window.set_fullscreen(target).map_err(|e| e.to_string())?;
        return Ok(target);
    }
    Ok(false)
}

#[tauri::command]
async fn send_input_to_remote(
    state: State<'_, AppState>,
    target_ip: String,
    target_port: u16,
    msg: ControlMessage,
) -> Result<(), String> {
    let mut lock = state.input_channel.lock().await;

    // 1. If persistent channel is already active for this target, send with 0 latency
    if let Some((ip, port, tx)) = &*lock {
        if *ip == target_ip && *port == target_port && !tx.is_closed() {
            let _ = tx.send(msg);
            return Ok(());
        }
    }

    // 2. Establish persistent zero-handshake TCP stream for input events
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ControlMessage>();
    let tip = target_ip.clone();
    let tport = target_port;
    let input_holder = state.input_channel.clone();

    tauri::async_runtime::spawn(async move {
        let addr_str = format!("{}:{}", tip, tport);
        if let Ok(addr) = addr_str.parse::<std::net::SocketAddr>() {
            match tokio::net::TcpStream::connect(addr).await {
                Ok(mut stream) => {
                    let _ = stream.set_nodelay(true);
                    let init_req = format!(
                        "POST /input_stream HTTP/1.1\r\n\
                        Host: {}\r\n\
                        Content-Type: text/plain\r\n\
                        Connection: keep-alive\r\n\r\n",
                        addr_str
                    );
                    use tokio::io::AsyncWriteExt;
                    if stream.write_all(init_req.as_bytes()).await.is_ok() {
                        println!("[InputClient] Connected input stream to {}", addr_str);
                        while let Some(m) = rx.recv().await {
                            if let Ok(serialized) = serde_json::to_string(&m) {
                                let line = format!("{}\n", serialized);
                                if stream.write_all(line.as_bytes()).await.is_err() {
                                    eprintln!("[InputClient] Write failed, connection lost to {}", addr_str);
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[InputClient] Could not connect input stream to {}: {}", addr_str, e);
                }
            }
        }
        let mut l = input_holder.lock().await;
        *l = None;
    });

    let _ = tx.send(msg);
    *lock = Some((target_ip, target_port, tx));
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let port = 18080;
    let discovery = Arc::new(DiscoveryManager::new(port));
    let server = Arc::new(EmbeddedHostServer::new(port, discovery.get_my_info()));

    let disc_clone = discovery.clone();
    let server_clone = server.clone();

    let state = AppState {
        discovery,
        _server: server,
        input_channel: Arc::new(Mutex::new(None)),
    };

    tauri::Builder::default()
        .manage(state)
        .setup(move |_app| {
            disc_clone.start_background();
            server_clone.start_background();

            // 1. Setup Tray Menu (Linux only: on modern macOS AppKit, NSStatusBarButton
            // custom subview in tray-icon triggers _appearanceForNonVibrantContent -> _fauxVibrantDarkAppearance SIGABRT).
            // On macOS, HyprRemote runs as a standard desktop client with native window & dock.
            #[cfg(target_os = "linux")]
            if let Some(icon) = _app.default_window_icon() {
                if let Ok(show_i) = MenuItem::with_id(_app, "show", "🖥️ 显示主界面", true, None::<&str>) {
                    if let Ok(quit_i) = MenuItem::with_id(_app, "quit", "❌ 彻底退出", true, None::<&str>) {
                        if let Ok(menu) = Menu::with_items(_app, &[&show_i, &quit_i]) {
                            let tray_builder = TrayIconBuilder::new()
                                .icon(icon.clone())
                                .menu(&menu)
                                .show_menu_on_left_click(false)
                                .on_menu_event(|app, event| {
                                    match event.id.as_ref() {
                                        "show" => {
                                            if let Some(window) = app.get_webview_window("main") {
                                                let _ = window.show();
                                                let _ = window.set_focus();
                                            }
                                        }
                                        "quit" => {
                                            app.exit(0);
                                        }
                                        _ => {}
                                    }
                                })
                                .on_tray_icon_event(|tray, event| {
                                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = event {
                                        let app = tray.app_handle();
                                        if let Some(window) = app.get_webview_window("main") {
                                            let _ = window.show();
                                            let _ = window.set_focus();
                                        }
                                    }
                                });

                            if let Err(e) = tray_builder.build(_app) {
                                eprintln!("[Tray] System tray unavailable ({}), running without tray icon.", e);
                            }
                        }
                    }
                }
            }

            Ok(())
        })
        .on_window_event(|_window, _event| {
            // Close window -> on Linux minimize to background tray; on macOS close cleanly!
            #[cfg(target_os = "linux")]
            if let WindowEvent::CloseRequested { api, .. } = _event {
                api.prevent_close();
                let _ = _window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_my_device_info,
            get_lan_devices,
            resolve_connection_code,
            toggle_fullscreen,
            send_input_to_remote,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
