// A Windows program is either a console program or a GUI program. Without
// this line, double-clicking danonpdf.exe would also open an empty black
// console. Debug builds keep the console, so `println!` output stays visible
// while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;

const APP_NAME: &str = "DanonPDF";

// `main` returns a Result so that a failure to open the window (for example,
// no usable OpenGL driver) ends the program with an error message instead of
// a panic.
fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        // The window title.
        APP_NAME,
        options,
        // eframe calls this once, after the window exists, to build the app
        // state. `Box` because eframe stores apps of any type behind a pointer.
        Box::new(|_creation_context| Ok(Box::new(DanonPdfApp))),
    )
}

/// The application state. Empty for now: the skeleton only shows its name.
struct DanonPdfApp;

impl eframe::App for DanonPdfApp {
    // egui is an immediate-mode GUI: this runs on every frame and describes
    // the whole window from scratch, instead of building widgets once and
    // updating them later.
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // The `ui` eframe hands over has no margin or background; the
        // central panel adds both.
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading(APP_NAME);
            // Cargo sets CARGO_PKG_VERSION at compile time from Cargo.toml,
            // so the version shown can never drift from the real one.
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        });
    }
}
