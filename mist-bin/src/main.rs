//! Mist — MUD client entry point.

use eframe::egui;
use mist_ui::MistApp;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mist")
            // Wayland compositors identify windows by app_id. Without it a
            // client shows up with an empty class, which breaks window
            // rules, taskbar entries and `hyprctl clients` filtering.
            .with_app_id("com.wyldware.mist")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Mist",
        opts,
        Box::new(|cc| Ok(Box::new(MistApp::new(cc)))),
    )
}