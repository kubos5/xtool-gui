#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod app;
mod runner;
mod term;
mod theme;
mod xtool;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("xtool-gui")
            .with_app_id("xtool-gui")
            .with_inner_size([1100.0, 760.0])
            .with_min_inner_size([760.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native("xtool-gui", options, Box::new(|cc| Ok(Box::new(app::App::new(cc)))))
}
