// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
// The binary's crate name follows the product name.
#![allow(non_snake_case)]

fn main() {
    EasyVideoDownloadLib::run()
}
