#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cominorsa_desktop::App;
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};

fn main() {
    let window = WindowBuilder::new()
        .with_title("COMINORSA CRM")
        .with_inner_size(LogicalSize::new(1200.0, 800.0));

    dioxus::LaunchBuilder::desktop()
        .with_cfg(Config::new().with_window(window).with_menu(None))
        .launch(App);
}
