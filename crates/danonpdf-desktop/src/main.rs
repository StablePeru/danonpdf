// A Windows program is either a console program or a GUI program. Without
// this line, double-clicking danonpdf.exe would also open an empty black
// console. Debug builds keep the console, so `println!` output stays visible
// while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod texts;

use eframe::egui;

// `main` returns a Result so that a failure to open the window (for example,
// no usable OpenGL driver) ends the program with an error code instead of a
// panic. A release build has no console to print that error to, so it is
// also shown in a native message box: otherwise a double-click would seem to
// do nothing at all.
fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    let result = eframe::run_native(
        // The window title.
        texts::APP_NAME,
        options,
        // eframe calls this once, after the window exists, to build the app
        // state. `Box` because eframe stores apps of any type behind a pointer.
        Box::new(|_creation_context| Ok(Box::new(DanonPdfApp))),
    );

    // `if let` runs the block only when the pattern matches: here, only when
    // `result` is an `Err`. `&result` borrows it, so we can still return it.
    if let Err(error) = &result {
        show_startup_error(error);
    }
    result
}

/// Tells the user, in a native message box, that the window did not open.
///
/// rfd's message box does not need a window of ours (there is none), and on
/// Windows it is a plain `MessageBoxW`, which works even when OpenGL does not.
fn show_startup_error(error: &eframe::Error) {
    // The answer (the button pressed) does not matter: there is only "OK".
    let _ = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Error)
        .set_title(texts::STARTUP_ERROR_TITLE)
        .set_description(texts::startup_error(error))
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
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
            ui.heading(texts::APP_NAME);
            // Cargo sets CARGO_PKG_VERSION at compile time from Cargo.toml,
            // so the version shown can never drift from the real one.
            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
        });
    }
}
