//! Files dragged from the file explorer onto the window: reading the ones
//! that were let go of, and showing that letting go will do something.
//!
//! Both views take dropped files, each in its own way (the merge list adds
//! them, the Split view chooses one), but they read them and light up the
//! same way, so that part lives here.

use std::path::PathBuf;

use eframe::egui;

/// The files dropped onto the window during this frame. Empty on nearly
/// every frame.
pub fn dropped_files(ui: &egui::Ui) -> Vec<PathBuf> {
    // `input` lends egui's input state to the closure while holding a lock
    // on it, so the closure only copies the paths out; the caller changes
    // its own state after the lock is released.
    ui.input(|input| {
        input
            .raw
            .dropped_files
            .iter()
            .map(|file| file.path().to_path_buf())
            .collect()
    })
}

/// How a drop zone looks during this frame.
pub struct DropZone {
    /// Whether files are being dragged over the window right now.
    pub hovering: bool,
    /// The border to draw around the zone: highlighted while `hovering`,
    /// the usual thin line otherwise.
    pub border: egui::Stroke,
}

/// Looks at whether files are being dragged over the window, and picks the
/// border for a drop zone drawn in `ui`.
///
/// While files are dragged over the window, egui lists them in
/// `hovered_files`. The border lights up so the user sees that letting go
/// will do something.
pub fn drop_zone(ui: &egui::Ui) -> DropZone {
    let hovering = ui.input(|input| !input.raw.hovered_files.is_empty());
    let visuals = ui.visuals();
    let border = if hovering {
        egui::Stroke::new(2.0, visuals.selection.bg_fill)
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    DropZone { hovering, border }
}
