#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    hypr_remote_client_lib::run();
}
