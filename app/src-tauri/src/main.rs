#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter};

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {name}! This string was built in Rust.")
}

#[tauri::command]
fn start_counting(app: AppHandle, to: u32) {
    thread::spawn(move || {
        for i in 1..=to {
            thread::sleep(Duration::from_millis(500));
            app.emit("tick", i).unwrap();
        }
    });
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![greet, start_counting])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
