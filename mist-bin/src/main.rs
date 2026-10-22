//! Mist — MUD client entry point.

use eframe::egui;
use mist_ui::MistApp;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("Mist"),
        ..Default::default()
    };

    eframe::run_native(
        "Mist",
        opts,
        Box::new(|cc| Ok(Box::new(MistApp::new(cc)))),
    )
}