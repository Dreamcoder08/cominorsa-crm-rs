#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cominorsa_desktop::{App, window_icon};
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};

fn main() {
    let window = WindowBuilder::new()
        .with_title("COMINORSA CRM")
        .with_inner_size(LogicalSize::new(1200.0, 800.0));

    let mut config = Config::new().with_window(window).with_menu(None);
    if let Ok(icon) = window_icon() {
        config = config.with_icon(icon);
    }

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .launch(App);
}
