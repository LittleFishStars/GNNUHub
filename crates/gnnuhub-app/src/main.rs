//! GNNUHub 桌面客户端（Tauri 2）二进制入口

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    gnnuhub_app::run();
}
