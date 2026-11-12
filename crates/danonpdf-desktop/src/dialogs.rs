//! The system's own dialogs: choose files, choose where to save, choose a
//! folder. Both views open them through here.
//!
//! Every dialog is modal and blocking: the call returns only once the
//! dialog closes, and the window is not redrawn meanwhile. That is what
//! native dialogs expect, and `set_parent` keeps the dialog in front of our
//! window. Each function returns `None` when the user cancels.
//!
//! Nothing is decided here. What a dialog starts with was worked out by the
//! view's state (`merge_list.rs`, `split_form.rs`), and what happens to the
//! path it returns is that state's business too.

use std::path::{Path, PathBuf};

use crate::paths::SaveSuggestion;
use crate::texts;

/// A file dialog titled `title` that belongs to our window and only shows
/// PDF files. The builder methods take the dialog by value and return it
/// changed, which is what lets the calls be chained.
fn pdf_dialog(frame: &eframe::Frame, title: &str) -> rfd::FileDialog {
    rfd::FileDialog::new()
        .set_title(title)
        .add_filter(texts::PDF_FILTER, &["pdf"])
        .set_parent(frame)
}

/// Asks for any number of PDF files, for the merge list.
pub fn pick_pdfs(frame: &eframe::Frame) -> Option<Vec<PathBuf>> {
    pdf_dialog(frame, texts::ADD_DIALOG_TITLE).pick_files()
}

/// Asks for one PDF file, for the Split view.
pub fn pick_pdf(frame: &eframe::Frame) -> Option<PathBuf> {
    pdf_dialog(frame, texts::CHOOSE_DIALOG_TITLE).pick_file()
}

/// Asks where to save a new PDF, starting from `suggested`.
///
/// On Windows the dialog also asks before replacing an existing file, and
/// adds ".pdf" when the user types a name without it (rfd sets the first
/// filter's extension as the default one). Whatever comes back, the view's
/// state still checks it before anything is written (`output_from_dialog`
/// in `paths.rs`).
pub fn ask_output_path(
    frame: &eframe::Frame,
    title: &str,
    suggested: SaveSuggestion,
) -> Option<PathBuf> {
    let mut dialog = pdf_dialog(frame, title);

    // Each optional step reassigns the variable. Which steps apply was
    // decided in `SaveSuggestion::for_path`, where it is tested.
    if let Some(folder) = suggested.folder {
        dialog = dialog.set_directory(folder);
    }
    if let Some(name) = suggested.file_name {
        dialog = dialog.set_file_name(name);
    }
    dialog.save_file()
}

/// Asks for a folder, starting in `start` when there is one.
///
/// On Windows this is the same dialog as for files, switched to folders:
/// the user can go anywhere and create a new folder in it. No file filter
/// is set, since nothing but folders can be picked.
///
/// `Option<&Path>` takes the folder by reference: the caller keeps its
/// `PathBuf` and lends it with `as_deref()`.
pub fn pick_folder(frame: &eframe::Frame, title: &str, start: Option<&Path>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new().set_title(title).set_parent(frame);
    if let Some(folder) = start {
        dialog = dialog.set_directory(folder);
    }
    dialog.pick_folder()
}
