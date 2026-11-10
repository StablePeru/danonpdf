// A Windows program is either a console program or a GUI program. Without
// this line, double-clicking danonpdf.exe would also open an empty black
// console. Debug builds keep the console, so `println!` output stays visible
// while developing.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod merge_list;
mod texts;

use eframe::egui;

use app::DanonPdfApp;

// `main` returns a Result so that a failure to open the window (for example,
// no usable OpenGL driver) ends the program with an error code instead of a
// panic. A release build has no console to print that error to, so it is
// also shown in a native message box: otherwise a double-click would seem to
// do nothing at all.
fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Cargo sets CARGO_PKG_VERSION at compile time from Cargo.toml,
            // so the version in the title can never drift from the real one.
            .with_title(format!("{} {}", texts::APP_NAME, env!("CARGO_PKG_VERSION")))
            .with_inner_size([600.0, 420.0])
            .with_min_inner_size([420.0, 260.0])
            // Already the default; written out because the app depends on
            // it. On Windows it makes winit call OleInitialize, which puts
            // this thread in a single-threaded COM apartment. rfd's file
            // dialogs ask for that same kind of apartment, so the two agree.
            // A crate that wanted a multithreaded apartment on this thread
            // would make winit panic when the window opens.
            .with_drag_and_drop(true),
        // `..` fills every other field from the default options.
        ..Default::default()
    };

    let result = eframe::run_native(
        // Names the app (for example, where egui could keep its settings).
        texts::APP_NAME,
        options,
        // eframe calls this once, after the window exists, to build the app
        // state. `Box` because eframe stores apps of any type behind a pointer.
        Box::new(|_creation_context| Ok(Box::new(DanonPdfApp::default()))),
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
