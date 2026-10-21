#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Install panic hook for detailed crash diagnostics
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("\n=============================================");
        eprintln!("🔥 [CRITICAL PANIC] HyprRemote has encountered a fatal error:");
        eprintln!("{}", panic_info);
        eprintln!("=============================================\n");
    }));

    #[cfg(target_os = "linux")]
    {
        // Prevent WebKitGTK compositing / DMA-BUF crash on Wayland & NVIDIA drivers
        if std::env::var("WEBKIT_DISABLE_COMPOSITING_MODE").is_err() {
            std::env::set_var("WEBKIT_DISABLE_COMPOSITING_MODE", "1");
        }
        if std::env::var("WEBKIT_DISABLE_DMABUF_RENDERER").is_err() {
            std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
        }
        if std::env::var("GDK_BACKEND").is_err() {
            std::env::set_var("GDK_BACKEND", "wayland,x11");
        }
    }
    hypr_remote_client_lib::run();
}
