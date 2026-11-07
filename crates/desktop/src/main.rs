#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use cominorsa_desktop::{App, load_initial_settings, window_icon};
use cominorsa_domain::ThemePreference;
use dioxus::desktop::{Config, LogicalSize, WindowBuilder};

fn main() {
    let window = WindowBuilder::new()
        .with_title("COMINORSA CRM")
        .with_inner_size(LogicalSize::new(1200.0, 800.0));

    let settings = load_initial_settings();
    let mut config = Config::new().with_window(window).with_menu(None);
    if let Ok(icon) = window_icon() {
        config = config.with_icon(icon);
    }

    match settings.theme {
        ThemePreference::Dark => config = config.with_background_color((14, 15, 16, 255)),
        ThemePreference::Light => config = config.with_background_color((250, 250, 249, 255)),
        ThemePreference::System => {}
    }

    dioxus::LaunchBuilder::desktop()
        .with_cfg(config)
        .with_context(settings)
        .launch(App);
}
