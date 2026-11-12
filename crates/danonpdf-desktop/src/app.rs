//! The window: the switch between its two views, and the drawing of the
//! Merge view. The Split view is drawn in `split_view.rs`.
//!
//! egui is an immediate-mode GUI. There are no button objects that live on
//! and fire callbacks later. Instead, [`eframe::App::ui`] runs again for
//! every frame (after each mouse move, click or key press) and describes the
//! whole window from scratch. Drawing a button and asking "was it clicked?"
//! are one call: `ui.button("Clear").clicked()`.
//!
//! So everything that must survive from one frame to the next lives in the
//! app struct: which view is shown, a [`MergeList`] and a [`SplitForm`].
//! The drawing code only reads that state to draw it, and turns clicks and
//! dropped files into calls on it. The rules themselves (what "move up"
//! means at the top, which files are accepted, which pages are valid) live
//! in `merge_list.rs` and `split_form.rs`, where they are tested without a
//! window.

use eframe::egui;

use crate::dialogs;
use crate::drops;
use crate::merge_list::{MergeList, RowAction, Status};
use crate::paths::display_name;
use crate::split_form::SplitForm;
use crate::split_view;
use crate::texts;

/// The whole app state. `Default` gives the Merge view with an empty list,
/// and a Split view with no file chosen.
///
/// Both views keep their state while the other one is shown, so switching
/// back and forth loses nothing.
#[derive(Default)]
pub struct DanonPdfApp {
    view: View,
    list: MergeList,
    split: SplitForm,
}

/// Which view the window shows.
///
/// `PartialEq` is what lets egui compare the current view with the one a
/// button stands for, to draw that button as selected.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum View {
    /// Join several PDFs into one. The view the window opens with.
    #[default]
    Merge,
    /// Take pages out of one PDF, or cut it into several files.
    Split,
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
        egui::Panel::top("views").show(ui, |ui| self.view_switch(ui));

        // Only the view that is shown is drawn. Nothing is hidden or kept
        // around for the other one: the next frame simply draws whichever
        // view `self.view` names by then.
        match self.view {
            View::Merge => {
                egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui, frame));
                egui::Panel::bottom("status").show(ui, |ui| self.status_line(ui));
                egui::CentralPanel::default().show(ui, |ui| self.file_list(ui));
            }
            View::Split => {
                egui::Panel::bottom("status")
                    .show(ui, |ui| split_view::status_line(ui, &self.split));
                egui::CentralPanel::default()
                    .show(ui, |ui| split_view::show(ui, frame, &mut self.split));
            }
        }
    }
}

impl DanonPdfApp {
    /// The two buttons at the top that choose the view.
    fn view_switch(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // `selectable_value` draws a button that looks pressed while
            // `self.view` equals the value it stands for, and stores that
            // value when it is clicked. Like every egui button it can be
            // reached with Tab and pressed with Space or Enter.
            ui.selectable_value(&mut self.view, View::Merge, texts::VIEW_MERGE)
                .on_hover_text(texts::VIEW_MERGE_HINT);
            ui.selectable_value(&mut self.view, View::Split, texts::VIEW_SPLIT)
                .on_hover_text(texts::VIEW_SPLIT_HINT);
        });
    }

    /// Hands the files dropped onto the window during this frame, if any,
    /// to the view that is shown: the merge list adds them, the Split view
    /// chooses one of them.
    fn take_dropped_files(&mut self, ui: &egui::Ui) {
        let dropped = drops::dropped_files(ui);
        if dropped.is_empty() {
            return;
        }
        match self.view {
            View::Merge => self.list.add(dropped),
            View::Split => self.split.choose(dropped),
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
                && let Some(paths) = dialogs::pick_pdfs(frame)
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
                    && let Some(output) = dialogs::ask_output_path(
                        frame,
                        texts::SAVE_DIALOG_TITLE,
                        self.list.save_suggestion(),
                    )
                {
                    self.list.merge_into(output);
                }
            });
        });
    }

    fn file_list(&mut self, ui: &mut egui::Ui) {
        // The border lights up while files are dragged over the window (see
        // `drops.rs`, which the Split view uses too).
        let zone = drops::drop_zone(ui);

        egui::Frame::group(ui.style())
            .stroke(zone.border)
            .show(ui, |ui| {
                // Fill the whole panel, so the drop zone is the whole area and
                // not just the size of its contents.
                ui.set_min_size(ui.available_size());

                if zone.hovering {
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
            Status::ExistsAfterAddingExtension { output } => {
                let color = ui.visuals().error_fg_color;
                ui.colored_label(
                    color,
                    texts::exists_after_adding_extension(&display_name(output)),
                )
                .on_hover_text(output.display().to_string());
            }
            Status::Failed { message } => {
                let color = ui.visuals().error_fg_color;
                ui.colored_label(color, texts::merge_failed(message));
            }
        }
    }
}
