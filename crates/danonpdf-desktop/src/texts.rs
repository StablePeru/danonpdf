//! Every text the user can see, in one place.
//!
//! The rest of the app never writes a sentence itself: it asks this module.
//! A translation (Basque first) then means one more module like this one,
//! not a hunt through the drawing code.

use std::fmt::Display;

/// The name in the title bar and in dialogs. A brand, so never translated.
pub const APP_NAME: &str = "DanonPDF";

/// Title of the message box shown when the window cannot open.
pub const STARTUP_ERROR_TITLE: &str = "DanonPDF cannot start";

/// Body of that message box. `error` is whatever eframe reported.
pub fn startup_error(error: &dyn Display) -> String {
    format!("DanonPDF could not open its window.\n\n{error}")
}
