//! The merge window: drawing only.
//!
//! egui is an immediate-mode GUI. There are no button objects that live on
//! and fire callbacks later. Instead, [`eframe::App::ui`] runs again for
//! every frame (after each mouse move, click or key press) and describes the
//! whole window from scratch. Drawing a button and asking "was it clicked?"
//! are one call: `ui.button("Clear").clicked()`.
//!
//! So everything that must survive from one frame to the next lives in the
//! app struct, here a [`MergeList`]. This module only reads that state to
//! draw it, and turns clicks and dropped files into calls on it. The rules
//! themselves (what "move up" means at the top, which files are accepted)
//! live in `merge_list.rs`, where they are tested without a window.

use std::path::PathBuf;

use eframe::egui;

use crate::merge_list::{MergeList, RowAction, SaveSuggestion, Status, display_name};
use crate::texts;

/// The whole app state. `Default` gives an empty list.
#[derive(Default)]
pub struct DanonPdfApp {
    list: MergeList,
}

impl eframe::App for DanonPdfApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.take_dropped_files(ui);

        // Panels are laid out in the order they are shown: the top and
        // bottom ones take their strips first, and the central panel gets
        // whatever is left. Each closure receives the panel's own `ui`.
        //
        // The closures borrow `self` (and `frame`) only while they run, one
        // after the other, so the borrows never overlap.
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui, frame));
        egui::Panel::bottom("status").show(ui, |ui| self.status_line(ui));
        egui::CentralPanel::default().show(ui, |ui| self.file_list(ui));
    }
}

impl DanonPdfApp {
    /// Adds the files dropped onto the window during this frame, if any.
    fn take_dropped_files(&mut self, ui: &egui::Ui) {
        // `input` lends egui's input state to the closure while holding a
        // lock on it, so the closure only copies the paths out; the list is
        // changed after the lock is released.
        let dropped: Vec<PathBuf> = ui.input(|input| {
            input
                .raw
                .dropped_files
                .iter()
                .map(|file| file.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.list.add(dropped);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        ui.horizontal(|ui| {
            // `if a && let Some(b) = c` is a "let chain": the second part
            // only runs (and the dialog only opens) when the button was
            // clicked, and the block only runs when the dialog returned
            // files (`None` means the user cancelled).
            if ui
                .button(texts::ADD_BUTTON)
                .on_hover_text(texts::ADD_HINT)
                .clicked()
                && let Some(paths) = pick_pdfs(frame)
            {
                self.list.add(paths);
            }

            let has_files = !self.list.files().is_empty();
            if ui
                .add_enabled(has_files, egui::Button::new(texts::CLEAR_BUTTON))
                .on_hover_text(texts::CLEAR_HINT)
                .clicked()
            {
                self.list.clear();
            }

            // A right-to-left layout fills the rest of the row from the
            // right edge, which puts "Merge…" in the corner, apart from the
            // buttons that only edit the list.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let merge = ui
                    .add_enabled(
                        self.list.can_merge(),
                        egui::Button::new(texts::MERGE_BUTTON),
                    )
                    .on_hover_text(texts::MERGE_HINT)
                    .on_disabled_hover_text(texts::MERGE_NEEDS_TWO);
                if merge.clicked()
                    && let Some(output) = ask_output_path(frame, self.list.save_suggestion())
                {
                    self.list.merge_into(output);
                }
            });
        });
    }

    fn file_list(&mut self, ui: &mut egui::Ui) {
        // While files are dragged over the window, egui lists them in
        // `hovered_files`. The border lights up so the user sees that
        // letting go will do something.
        let hovering = ui.input(|input| !input.raw.hovered_files.is_empty());
        let visuals = ui.visuals();
        let border = if hovering {
            egui::Stroke::new(2.0, visuals.selection.bg_fill)
        } else {
            visuals.widgets.noninteractive.bg_stroke
        };

        egui::Frame::group(ui.style())
            .stroke(border)
            .show(ui, |ui| {
                // Fill the whole panel, so the drop zone is the whole area and
                // not just the size of its contents.
                ui.set_min_size(ui.available_size());

                if hovering {
                    ui.centered_and_justified(|ui| ui.label(texts::DROP_NOW));
                } else if self.list.files().is_empty() {
                    ui.centered_and_justified(|ui| ui.weak(texts::DROP_HINT));
                } else {
                    egui::ScrollArea::vertical()
                        .auto_shrink(false)
                        .show(ui, |ui| self.rows(ui));
                }
            });
    }

    /// One row per file: its buttons, its place in the order and its name.
    fn rows(&mut self, ui: &mut egui::Ui) {
        // The loop below borrows the list to read it, so it cannot change
        // the list at the same time. It only notes which button was clicked
        // (at most one per frame), and the change happens after the loop,
        // once that borrow is over.
        let mut action: Option<RowAction> = None;
        let last = self.list.files().len().saturating_sub(1);

        for (index, path) in self.list.files().iter().enumerate() {
            ui.horizontal(|ui| {
                // This closure captures `action` by mutable reference, so
                // what it writes is still there after it returns.
                let up = egui::Button::new(texts::MOVE_UP);
                if ui
                    .add_enabled(index > 0, up)
                    .on_hover_text(texts::MOVE_UP_HINT)
                    .clicked()
                {
                    action = Some(RowAction::MoveUp(index));
                }
                let down = egui::Button::new(texts::MOVE_DOWN);
                if ui
                    .add_enabled(index < last, down)
                    .on_hover_text(texts::MOVE_DOWN_HINT)
                    .clicked()
                {
                    action = Some(RowAction::MoveDown(index));
                }
                if ui
                    .button(texts::REMOVE)
                    .on_hover_text(texts::REMOVE_HINT)
                    .clicked()
                {
                    action = Some(RowAction::Remove(index));
                }

                // The buttons come first so they line up in a column; a long
                // name is cut short with "…" instead of pushing them away.
                // Hovering it shows the full path.
                let name = format!("{}. {}", index + 1, display_name(path));
                ui.add(egui::Label::new(name).truncate())
                    .on_hover_text(path.display().to_string());
            });
        }

        // `if let` runs the block only when there is an action to apply.
        if let Some(action) = action {
            self.list.apply(action);
        }
    }

    fn status_line(&self, ui: &mut egui::Ui) {
        // A `match` must cover every variant of `Status`. If a new kind of
        // status is added later, this stops compiling until it is drawn.
        match self.list.status() {
            Status::Idle => {
                ui.weak(texts::files_in_list(self.list.files().len()));
            }
            // The fields are borrowed out of the status, so `*` copies the
            // numbers out of the references.
            Status::Skipped {
                not_pdf,
                duplicates,
            } => {
                ui.label(texts::skipped(*not_pdf, *duplicates));
            }
            Status::Merged { pages, output } => {
                ui.label(texts::merged(*pages, &display_name(output)))
                    .on_hover_text(output.display().to_string());
            }
            Status::OutputIsInput { output } => {
                let color = ui.visuals().error_fg_color;
                ui.colored_label(color, texts::output_is_input(&display_name(output)))
                    .on_hover_text(output.display().to_string());
            }
            Status::Failed { message } => {
                let color = ui.visuals().error_fg_color;
                ui.colored_label(color, texts::merge_failed(message));
            }
        }
    }
}

/// Opens the system's file picker for PDF files. `None` if the user cancels.
///
/// The dialog is modal and blocking: this call returns only once it closes,
/// and the window is not redrawn meanwhile. That is what native dialogs
/// expect, and `set_parent` keeps the dialog in front of our window.
fn pick_pdfs(frame: &eframe::Frame) -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .set_title(texts::ADD_DIALOG_TITLE)
        .add_filter(texts::PDF_FILTER, &["pdf"])
        .set_parent(frame)
        .pick_files()
}

/// Asks where to save the merged PDF, starting from `suggested`. `None` if
/// the user cancels.
///
/// On Windows the dialog also asks before replacing an existing file, and
/// adds ".pdf" when the user types a name without it (rfd sets the first
/// filter's extension as the default one). Whatever comes back, the list
/// still refuses to write over one of its own files.
fn ask_output_path(frame: &eframe::Frame, suggested: SaveSuggestion) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new()
        .set_title(texts::SAVE_DIALOG_TITLE)
        .add_filter(texts::PDF_FILTER, &["pdf"])
        .set_parent(frame);

    // The builder methods take the dialog by value and return it changed,
    // so each optional step reassigns the variable. Which steps apply was
    // decided in `SaveSuggestion::for_path`, where it is tested.
    if let Some(folder) = suggested.folder {
        dialog = dialog.set_directory(folder);
    }
    if let Some(name) = suggested.file_name {
        dialog = dialog.set_file_name(name);
    }
    dialog.save_file()
}
