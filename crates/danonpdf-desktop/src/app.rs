//! The window: the switch between its three views, and the drawing of the
//! Merge view. The Split view is drawn in `split_view.rs` and the Pages
//! view in `thumbnails_view.rs`.
//!
//! egui is an immediate-mode GUI. There are no button objects that live on
//! and fire callbacks later. Instead, [`eframe::App::ui`] runs again for
//! every frame (after each mouse move, click or key press) and describes the
//! whole window from scratch. Drawing a button and asking "was it clicked?"
//! are one call: `ui.button("Clear").clicked()`.
//!
//! So everything that must survive from one frame to the next lives in the
//! app struct: which view is shown, a [`MergeList`], a [`SplitForm`] and a
//! [`Thumbnails`]. The drawing code only reads that state to draw it, and
//! turns clicks and dropped files into calls on it. The rules themselves
//! (what "move up" means at the top, which files are accepted, which pages
//! are valid, which thumbnails are asked for) live in `merge_list.rs`,
//! `split_form.rs` and `thumbnails.rs`, where they are tested without a
//! window.
//!
//! Merging, extracting and splitting run on the thread that draws the
//! window, so the window waits for them. The Pages view is the one that
//! does not wait: its work is done by a second thread (see
//! `thumbnail_worker.rs`).

use std::path::PathBuf;

use eframe::egui;

use crate::dialogs;
use crate::drops;
use crate::merge_list::{MergeList, RowAction, Status};
use crate::paths::display_name;
use crate::split_form::SplitForm;
use crate::split_view;
use crate::texts;
use crate::thumbnails::Thumbnails;
use crate::thumbnails_view::{self, ThumbnailsView};

/// The whole app state. `Default` gives the Merge view with an empty list,
/// a Split view with no file chosen and a Pages view with none either.
///
/// Every view keeps its state while another one is shown, so switching
/// back and forth loses nothing.
///
/// `Default` starts no thread and loads no library: the Pages view starts
/// its worker the first time a file is chosen in it. That is also what
/// lets a test build the whole app without a window.
#[derive(Default)]
pub struct DanonPdfApp {
    view: View,
    list: MergeList,
    split: SplitForm,
    thumbnails: Thumbnails,
    /// What the Pages view owns besides its state: the worker thread and
    /// the textures. Both are empty until the view is drawn for the first
    /// time. When the app is dropped, on closing the window, this field
    /// goes with it, and dropping the worker ends its thread (see `Drop`
    /// in `thumbnail_worker.rs`).
    thumbnails_view: ThumbnailsView,
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
    /// See the pages of one PDF as thumbnails.
    Pages,
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
        //
        // Each arm answers one question when it is done: is the status line
        // it drew already out of date? The status panel has to be shown
        // before the central panel (the order above), so a click in the
        // central panel changes the status after its line was drawn. The
        // arms keep a copy of what was drawn and compare it afterwards.
        let status_is_stale = match self.view {
            View::Merge => {
                egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui, frame));
                let drawn = self.list.status().clone();
                egui::Panel::bottom("status").show(ui, |ui| self.status_line(ui));
                egui::CentralPanel::default().show(ui, |ui| self.file_list(ui));
                self.list.status() != &drawn
            }
            View::Split => {
                let drawn = self.split.status().clone();
                egui::Panel::bottom("status")
                    .show(ui, |ui| split_view::status_line(ui, &self.split));
                egui::CentralPanel::default()
                    .show(ui, |ui| split_view::show(ui, frame, &mut self.split));
                self.split.status() != &drawn
            }
            View::Pages => {
                // First of all, what the worker thread sent since the last
                // frame, so that this frame draws it. Nothing here waits:
                // a reply that has not arrived is drawn by a later frame,
                // which the worker asks for when it sends it.
                self.thumbnails_view
                    .exchange(ui.ctx(), &mut self.thumbnails);
                let drawn = self.thumbnails.status().clone();
                egui::Panel::bottom("status")
                    .show(ui, |ui| thumbnails_view::status_line(ui, &self.thumbnails));
                egui::CentralPanel::default().show(ui, |ui| {
                    self.thumbnails_view.show(ui, frame, &mut self.thumbnails);
                });
                // Drawing found out which pages are on screen and asked
                // for the missing ones: those requests go out now, and
                // not one frame later.
                self.thumbnails_view
                    .exchange(ui.ctx(), &mut self.thumbnails);
                self.thumbnails.status() != &drawn
            }
        };

        // egui only draws a new frame when something asks for one, usually
        // the mouse or the keyboard. Without this, the old line could stay
        // on screen until the mouse happened to move, so the result of
        // "Extract…" would show up late or seem not to show up at all.
        // Asking for one more frame does not depend on that luck: drawing the
        // central panel first is not possible (it would take the strip the
        // status line needs), and the frame is asked for only when the
        // status really changed, so an idle window still draws nothing.
        if status_is_stale {
            ui.ctx().request_repaint();
        }
    }
}

impl DanonPdfApp {
    /// The three buttons at the top that choose the view.
    fn view_switch(&mut self, ui: &mut egui::Ui) {
        // The buttons work on a copy of the view, and the copy goes
        // through `switch_to`: leaving a view is more than storing another
        // value.
        let mut chosen = self.view;
        ui.horizontal(|ui| {
            // `selectable_value` draws a button that looks pressed while
            // `chosen` equals the value it stands for, and stores that
            // value when it is clicked. Like every egui button it can be
            // reached with Tab and pressed with Space or Enter.
            ui.selectable_value(&mut chosen, View::Merge, texts::VIEW_MERGE)
                .on_hover_text(texts::VIEW_MERGE_HINT);
            ui.selectable_value(&mut chosen, View::Split, texts::VIEW_SPLIT)
                .on_hover_text(texts::VIEW_SPLIT_HINT);
            ui.selectable_value(&mut chosen, View::Pages, texts::VIEW_PAGES)
                .on_hover_text(texts::VIEW_PAGES_HINT);
        });
        self.switch_to(chosen);
    }

    /// Shows `view` from the next frame on.
    ///
    /// The Pages view is told when it is left: it has a thread drawing
    /// pages for it, and the pages it still waits for are cancelled, so
    /// that nothing is drawn for a view nobody is looking at. The other
    /// two views have nothing going on while they are not shown.
    ///
    /// It needs no `ui`, so a test can switch views without a window.
    fn switch_to(&mut self, view: View) {
        if self.view == View::Pages && view != View::Pages {
            self.thumbnails.hide();
        }
        self.view = view;
    }

    /// Asks egui for the files dropped onto the window during this frame
    /// and hands them on. Reading them needs the `ui`; deciding where they
    /// go does not, which is why that part is [`Self::take_files`].
    fn take_dropped_files(&mut self, ui: &egui::Ui) {
        self.take_files(drops::dropped_files(ui));
    }

    /// Hands `files` to the view that is shown: the merge list adds them,
    /// the Split view and the Pages view each choose one of them. The
    /// other views are not touched.
    ///
    /// It takes plain paths and no `ui`, so a test can drop files on the
    /// app without a window.
    fn take_files(&mut self, files: Vec<PathBuf>) {
        // Nearly every frame brings no file. An empty list must not reach a
        // view: each treats a call as something the user did, and would
        // wipe its status line for it.
        if files.is_empty() {
            return;
        }
        match self.view {
            View::Merge => self.list.add(files),
            View::Split => self.split.choose(files),
            // Only notes the file and a request for the worker: the file
            // is opened on the worker thread, not here.
            View::Pages => self.thumbnails.choose(files),
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

// The app struct needs no window to exist: a new one has no thread and no
// texture yet. So what it decides (which view gets the files that were
// dropped, what leaving a view does) is tested like the rest of the state:
// by calling it.
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::test_support::ScratchDir;
    use crate::thumbnail_worker::Reply;
    use crate::thumbnails::{self, Document, thumbnail_size};

    /// A path inside a folder called `docs`. The merge list only looks at
    /// names when files are added, so nothing has to exist there.
    fn doc(name: &str) -> PathBuf {
        Path::new("docs").join(name)
    }

    /// An app that shows the Split view. `..Default::default()` fills the
    /// fields that are not named from the default app.
    fn app_on_split() -> DanonPdfApp {
        DanonPdfApp {
            view: View::Split,
            ..Default::default()
        }
    }

    /// An app that shows the Pages view, in which `docs/report.pdf` is
    /// open and has eight pages, four of them on screen and asked for. The
    /// test plays the worker: there is no thread.
    fn app_with_pages_open() -> DanonPdfApp {
        let mut app = DanonPdfApp {
            view: View::Pages,
            ..Default::default()
        };
        app.take_files(vec![doc("report.pdf")]);
        app.thumbnails.take_requests();
        app.thumbnails.receive(Reply::Opened {
            generation: app.thumbnails.generation().current(),
            result: Ok(8),
        });
        app.thumbnails.show_rows(0..1, 4, thumbnail_size(1.0));
        app.thumbnails.take_requests();
        app
    }

    #[test]
    fn the_window_opens_on_the_merge_view() {
        assert_eq!(DanonPdfApp::default().view, View::Merge);
    }

    #[test]
    fn a_new_app_has_started_nothing() {
        // No file, no request for a worker: nothing that would start a
        // thread or load PDFium before the user asks to see pages.
        let app = DanonPdfApp::default();

        assert_eq!(app.thumbnails.document(), &Document::None);
        assert!(!app.thumbnails.has_requests());
    }

    #[test]
    fn files_dropped_on_the_pages_view_choose_its_file_and_nothing_else() {
        let mut app = DanonPdfApp {
            view: View::Pages,
            ..Default::default()
        };

        // The files do not exist: this view reads nothing on this thread.
        app.take_files(vec![doc("notes.txt"), doc("report.pdf"), doc("other.pdf")]);

        assert_eq!(
            app.thumbnails.document(),
            &Document::Opening {
                path: doc("report.pdf")
            }
        );
        // The same account of what was left out as in the Split view.
        assert_eq!(
            app.thumbnails.status(),
            &thumbnails::Status::LeftOut {
                other_pdfs: 1,
                not_pdf: 1
            }
        );
        assert!(app.thumbnails.has_requests());
        assert!(app.list.files().is_empty());
        assert_eq!(app.split.file(), None);
    }

    #[test]
    fn files_that_are_not_pdfs_dropped_on_the_pages_view_are_counted() {
        let mut app = app_with_pages_open();

        app.take_files(vec![doc("photo.jpg")]);

        assert_eq!(
            app.thumbnails.status(),
            &thumbnails::Status::NoPdf { not_pdf: 1 }
        );
        assert_eq!(app.thumbnails.pages(), Some(8));
    }

    #[test]
    fn files_dropped_on_another_view_do_not_reach_the_pages_view() {
        let mut app = DanonPdfApp::default();

        app.take_files(vec![doc("a.pdf")]);

        assert_eq!(app.thumbnails.document(), &Document::None);
        assert!(!app.thumbnails.has_requests());
    }

    #[test]
    fn leaving_the_pages_view_cancels_its_requests_and_keeps_its_file() {
        let mut app = app_with_pages_open();
        let before = app.thumbnails.generation().current();

        app.switch_to(View::Merge);

        assert_eq!(app.view, View::Merge);
        // A new generation: the worker skips the pages it was asked for.
        assert_eq!(app.thumbnails.generation().current(), before + 1);
        assert_eq!(app.thumbnails.slot(1), None);
        assert_eq!(app.thumbnails.pages(), Some(8));

        // Going between the other two views is nothing to the Pages view.
        app.switch_to(View::Split);
        assert_eq!(app.thumbnails.generation().current(), before + 1);

        // Back: the file is still there, and its pages are asked for again
        // by the first frame that draws the grid.
        app.switch_to(View::Pages);
        assert_eq!(app.thumbnails.pages(), Some(8));
    }

    #[test]
    fn choosing_the_view_that_is_already_shown_cancels_nothing() {
        // The view switch reports the current view on every frame in which
        // no button is clicked.
        let mut app = app_with_pages_open();
        let before = app.thumbnails.generation().current();

        app.switch_to(View::Pages);

        assert_eq!(app.thumbnails.generation().current(), before);
        assert_eq!(app.thumbnails.slot(1), Some(&thumbnails::Slot::Requested));
    }

    #[test]
    fn files_dropped_on_the_merge_view_join_the_list_and_nothing_else() {
        let mut app = DanonPdfApp::default();

        app.take_files(vec![doc("a.pdf"), doc("b.pdf")]);

        assert_eq!(app.list.files(), [doc("a.pdf"), doc("b.pdf")]);
        assert_eq!(app.split.file(), None);
    }

    #[test]
    fn files_dropped_on_the_split_view_choose_its_file_and_nothing_else() {
        let scratch = ScratchDir::new("app-drop-on-split");
        let report = scratch.pdf("report.pdf", 3);
        let mut app = app_on_split();

        app.take_files(vec![report.clone()]);

        // `map` looks inside the `Option` to compare just the path.
        assert_eq!(app.split.file().map(|file| &file.path), Some(&report));
        assert!(app.list.files().is_empty());
    }

    #[test]
    fn a_frame_without_dropped_files_reaches_neither_view() {
        // Nearly every frame has no dropped file. If an empty list reached
        // a view, it would count as an action of the user's and wipe the
        // status line (here: the notice about a skipped file).
        let mut app = DanonPdfApp::default();
        app.take_files(vec![doc("photo.jpg")]);
        let notice = app.list.status().clone();
        assert_ne!(notice, Status::Idle);

        app.take_files(Vec::new());
        assert_eq!(app.list.status(), &notice);
    }

    #[test]
    fn each_view_keeps_what_it_holds_while_the_other_is_shown() {
        let scratch = ScratchDir::new("app-views-keep-state");
        let report = scratch.pdf("report.pdf", 3);
        let mut app = DanonPdfApp::default();

        // Fill the Merge view, then work in the Split view.
        app.take_files(vec![doc("a.pdf"), doc("b.pdf")]);
        app.view = View::Split;
        app.take_files(vec![report.clone()]);
        app.split.set_pages_text("1-2".to_owned());

        assert_eq!(app.list.files(), [doc("a.pdf"), doc("b.pdf")]);
        assert!(app.list.can_merge());

        // Back to Merge, and one more file for its list.
        app.view = View::Merge;
        app.take_files(vec![doc("c.pdf")]);

        assert_eq!(app.list.files().len(), 3);
        assert_eq!(app.split.file().map(|file| &file.path), Some(&report));
        assert_eq!(app.split.pages_text(), "1-2");
        assert!(app.split.can_extract());
    }
}
