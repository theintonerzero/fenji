//! Fenji's desktop launcher (launcher spec §3.1, §3.9): the Tauri shell over fenji-launcher's
//! model. The model owns the state and every secret; the view renders its snapshots.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod shell;
mod updates;

fn main() {
    shell::run();
}
