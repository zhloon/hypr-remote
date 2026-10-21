pub mod capture;
pub mod discovery;
pub mod input;
pub mod server;
pub mod types;

use discovery::DiscoveryManager;
use server::EmbeddedHostServer;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, State, WindowEvent};
use types::{ConnectResult, ControlMessage, LanDevice, MyDeviceInfo};

struct AppState {
    discovery: Arc<DiscoveryManager>,
    _server: Arc<EmbeddedHostServer>,
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

    // 1. Check if connecting to self (loopback test)
    let my_info = state.discovery.get_my_info();
    if clean == my_info.code.replace(" ", "") {
        return Ok(ConnectResult {
            success: true,
            error: None,
            stream_url: Some(format!("http://127.0.0.1:{}/stream", my_info.port)),
        });
    }

    // 2. Query LAN discovered devices
    if let Some((ip, port)) = state.discovery.resolve_code(&code) {
        return Ok(ConnectResult {
            success: true,
            error: None,
            stream_url: Some(format!("http://{}:{}/stream", ip, port)),
        });
    }

    // 3. Fallback: direct IP:Port
    if code.contains('.') && code.contains(':') {
        return Ok(ConnectResult {
            success: true,
            error: None,
            stream_url: Some(format!("http://{}/stream", code.trim())),
        });
    }

    Ok(ConnectResult {
        success: false,
        error: Some("未在局域网内找到该连接码对应的设备，请确认对方已打开 HyprRemote。".to_string()),
        stream_url: None,
    })
}

#[tauri::command]
fn toggle_fullscreen(app: AppHandle, fullscreen: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("main") {
        window.set_fullscreen(fullscreen).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn send_input_to_remote(
    target_ip: String,
    target_port: u16,
    msg: ControlMessage,
) -> Result<(), String> {
    let url = format!("http://{}:{}/input", target_ip, target_port);
    let body = serde_json::to_string(&msg).map_err(|e| e.to_string())?;

    tauri::async_runtime::spawn(async move {
        if let Ok(addr) = format!("{}:{}", target_ip, target_port).parse::<std::net::SocketAddr>() {
            if let Ok(mut stream) = tokio::net::TcpStream::connect(addr).await {
                let _ = stream.set_nodelay(true);
                let req = format!(
                    "POST /input HTTP/1.1\r\n\
                    Host: {}\r\n\
                    Content-Type: application/json\r\n\
                    Content-Length: {}\r\n\
                    Connection: close\r\n\r\n{}",
                    url,
                    body.len(),
                    body
                );
                let _ = tokio::io::AsyncWriteExt::write_all(&mut stream, req.as_bytes()).await;
            }
        }
    });

    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let port = 18080;
    let discovery = Arc::new(DiscoveryManager::new(port));
    let server = Arc::new(EmbeddedHostServer::new(port, discovery.get_my_info().pin));

    let disc_clone = discovery.clone();
    let server_clone = server.clone();

    let state = AppState {
        discovery,
        _server: server,
    };

    tauri::Builder::default()
        .manage(state)
        .setup(move |app| {
            disc_clone.start_background();
            server_clone.start_background();

            // 1. Setup Tray Menu
            let show_i = MenuItem::with_id(app, "show", "🖥️ 显示主界面", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "❌ 彻底退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

            // 2. Build Tray Icon for background persistence
            let mut tray_builder = TrayIconBuilder::new()
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

            if let Some(icon) = app.default_window_icon() {
                tray_builder = tray_builder.icon(icon.clone());
            }

            tray_builder.build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            // Close window -> minimize to tray / run in background!
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
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
