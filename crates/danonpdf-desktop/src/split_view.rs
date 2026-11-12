//! The Split view: drawing only.
//!
//! Everything here reads a [`SplitForm`] to draw it, and turns clicks and
//! typing into calls on it. Which file is accepted, whether the pages typed
//! are valid, what a split would be called and what may be written where
//! are all decided in `split_form.rs`, where they are tested without a
//! window. `app.rs` explains how egui redraws from state on every frame.
//!
//! Every control is a standard egui widget with its text on it or next to
//! it, so it can be reached with Tab and a screen reader can name it.

use eframe::egui;

use crate::dialogs;
use crate::drops;
use crate::paths::display_name;
use crate::split_form::{Mode, PagesCheck, SplitForm, Status};
use crate::texts;

/// The least height of the drop zone, in egui points: two lines of text
/// and the space around them.
const DROP_ZONE_MIN_HEIGHT: f32 = 56.0;

/// Draws the view into the central panel: the file on top, then the choice
/// between extracting and splitting, then the controls of the chosen one.
pub fn show(ui: &mut egui::Ui, frame: &eframe::Frame, form: &mut SplitForm) {
    // The view is taller than the smallest window allows, so it scrolls
    // instead of pushing its last button out of reach.
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // A "let chain": the dialog only opens when the button was
            // clicked, and the block only runs when the dialog returned a
            // file (`None` means the user cancelled).
            if ui
                .button(texts::CHOOSE_BUTTON)
                .on_hover_text(texts::CHOOSE_HINT)
                .clicked()
                && let Some(path) = dialogs::pick_pdf(frame)
            {
                // An array of one path: `choose` takes any number of them,
                // because a drop can bring several.
                form.choose([path]);
            }
            chosen_file(ui, form);
            ui.add_space(8.0);

            // Without a file there is nothing to extract or split, so
            // everything below is drawn greyed out and takes no clicks.
            let has_file = form.file().is_some();
            ui.add_enabled_ui(has_file, |ui| {
                mode_switch(ui, form);
                ui.separator();
                // A `match` must cover every mode: a third one added later
                // would not compile until it has controls here.
                match form.mode() {
                    Mode::Extract => extract_controls(ui, frame, form),
                    Mode::Split => split_controls(ui, frame, form),
                }
            });
        });
}

/// The drop zone, which also shows the chosen file: its name and how many
/// pages it has, or an invitation to drop one.
fn chosen_file(ui: &mut egui::Ui, form: &SplitForm) {
    let zone = drops::drop_zone(ui);
    egui::Frame::group(ui.style())
        .stroke(zone.border)
        .show(ui, |ui| {
            // Room for the two lines of a chosen file, whatever is shown:
            // without it the box would shrink to one line while a file is
            // dragged over it, and everything below would jump.
            ui.set_min_height(DROP_ZONE_MIN_HEIGHT);
            // `vertical_centered` takes the whole width and centres each
            // line in it; its height is that of its contents, padded a
            // little so the zone is an easy target.
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                if zone.hovering {
                    ui.label(texts::SPLIT_DROP_NOW);
                } else if let Some(file) = form.file() {
                    // A long name is cut short with "…" instead of widening
                    // the window. Hovering it shows the full path.
                    ui.add(egui::Label::new(display_name(&file.path)).truncate())
                        .on_hover_text(file.path.display().to_string());
                    ui.weak(texts::page_count(file.pages));
                } else {
                    ui.weak(texts::SPLIT_DROP_HINT);
                }
                ui.add_space(8.0);
            });
        });
}

/// The two radio buttons that choose between extracting and splitting.
fn mode_switch(ui: &mut egui::Ui, form: &mut SplitForm) {
    ui.horizontal(|ui| {
        // `radio` draws one button, filled when its first argument is
        // true, and reports clicks. The form is told about the click and
        // the next frame draws the new state: the button itself remembers
        // nothing.
        if ui
            .radio(form.mode() == Mode::Extract, texts::MODE_EXTRACT)
            .on_hover_text(texts::MODE_EXTRACT_HINT)
            .on_disabled_hover_text(texts::NEEDS_FILE)
            .clicked()
        {
            form.set_mode(Mode::Extract);
        }
        if ui
            .radio(form.mode() == Mode::Split, texts::MODE_SPLIT)
            .on_hover_text(texts::MODE_SPLIT_HINT)
            .on_disabled_hover_text(texts::NEEDS_FILE)
            .clicked()
        {
            form.set_mode(Mode::Split);
        }
    });
}

/// The pages field, the line under it that says what the text means, and
/// the "Extract…" button.
fn extract_controls(ui: &mut egui::Ui, frame: &eframe::Frame, form: &mut SplitForm) {
    ui.horizontal(|ui| {
        let label = ui.label(texts::PAGES_LABEL);
        // A text field edits a `String` it is lent for the frame. The form
        // keeps its text private, so the field edits a copy, and the copy
        // goes back to the form only when it changed. Copying a few
        // characters per frame costs nothing that could be measured.
        let mut text = form.pages_text().to_owned();
        let field = ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(texts::PAGES_EXAMPLE)
                    // Take the rest of the row.
                    .desired_width(f32::INFINITY),
            )
            // Tells a screen reader that the label before it names this
            // field.
            .labelled_by(label.id)
            .on_hover_text(texts::PAGES_HINT)
            .on_disabled_hover_text(texts::NEEDS_FILE);
        // `changed` is true on the frames where a key edited the text, so
        // the form checks the pages as they are typed.
        if field.changed() {
            form.set_pages_text(text);
        }
    });

    // Each arm draws one label; the `;` after the `match` drops the
    // `Response` they all return.
    match form.pages_check() {
        PagesCheck::Pending => ui.weak(texts::PAGES_PROMPT),
        PagesCheck::Selected { pages, .. } => ui.label(texts::pages_selected(*pages)),
        PagesCheck::Invalid { message } => {
            let color = ui.visuals().error_fg_color;
            ui.colored_label(color, texts::pages_invalid(message))
        }
    };

    let extract = ui
        .add_enabled(form.can_extract(), egui::Button::new(texts::EXTRACT_BUTTON))
        .on_hover_text(texts::EXTRACT_HINT)
        .on_disabled_hover_text(texts::EXTRACT_NEEDS_PAGES);
    // The name to propose is only worked out once the button is clicked
    // (it asks the disk which names are taken). The extraction then runs
    // right here, on the thread that draws the window, so the window does
    // not answer until it ends. A later change will move it to a
    // background thread.
    if extract.clicked()
        && let Some(output) = dialogs::ask_output_path(
            frame,
            texts::EXTRACT_DIALOG_TITLE,
            form.extract_suggestion(),
        )
    {
        form.extract_into(output);
    }
}

/// The "pages per file" number, the preview of what a split would write,
/// and the "Split…" button.
fn split_controls(ui: &mut egui::Ui, frame: &eframe::Frame, form: &mut SplitForm) {
    ui.horizontal(|ui| {
        let label = ui.label(texts::PAGES_PER_FILE_LABEL);
        // As with the pages field: the widget edits a copy, and the form
        // decides what becomes of it. `DragValue` is a number that can be
        // dragged, typed over after a click, or changed with the arrow
        // keys once Tab has reached it. `range` keeps what it shows inside
        // the values the form accepts.
        let mut pages = form.pages_per_file();
        let field = ui
            .add(
                egui::DragValue::new(&mut pages)
                    .range(form.pages_per_file_range())
                    // Slower than the default of one step per pixel: page
                    // counts are small numbers.
                    .speed(0.1),
            )
            .labelled_by(label.id)
            .on_hover_text(texts::PAGES_PER_FILE_HINT)
            .on_disabled_hover_text(texts::NEEDS_FILE);
        if field.changed() {
            form.set_pages_per_file(pages);
        }
    });

    // `if let` draws the line only when there is a preview, which is
    // whenever a file is chosen.
    if let Some(preview) = form.split_preview() {
        ui.label(texts::split_preview(
            preview.pages,
            preview.files,
            &preview.first,
            &preview.last,
        ));
    }

    let split = ui
        .add_enabled(form.can_split(), egui::Button::new(texts::SPLIT_BUTTON))
        .on_hover_text(texts::SPLIT_HINT)
        .on_disabled_hover_text(texts::NEEDS_FILE);
    // `as_deref` lends the suggested folder as `Option<&Path>`. The split
    // runs right here, on the thread that draws the window, so the window
    // does not answer until every file is written. A later change will
    // move it to a background thread.
    if split.clicked()
        && let Some(folder) = dialogs::pick_folder(
            frame,
            texts::SPLIT_DIALOG_TITLE,
            form.split_folder_suggestion().as_deref(),
        )
    {
        form.split_into(folder);
    }
}

/// The line at the bottom of the window while this view is shown.
pub fn status_line(ui: &mut egui::Ui, form: &SplitForm) {
    let error = ui.visuals().error_fg_color;
    // A `match` must cover every variant of `Status`. If a new kind of
    // status is added later, this stops compiling until it is drawn.
    //
    // The fields are borrowed out of the status, so `*` copies the numbers
    // out of the references. A path is shown by its file name, with the
    // whole path on hover; a folder is shown whole, since its name alone
    // would not say where the files went.
    match form.status() {
        Status::Idle => {
            // `match` on an `Option`: one line for each case.
            match form.file() {
                Some(file) => ui.weak(texts::file_chosen(&display_name(&file.path))),
                None => ui.weak(texts::NO_FILE_CHOSEN),
            };
        }
        Status::NoPdf { not_pdf } => {
            // The same words as in the Merge view, for the same thing.
            ui.label(texts::skipped(*not_pdf, 0));
        }
        Status::LeftOut {
            other_pdfs,
            not_pdf,
        } => {
            // This status is only set together with a file; `map_or_else`
            // still has an answer (an empty name) if there were none.
            let name = form
                .file()
                .map_or_else(String::new, |file| display_name(&file.path));
            ui.label(texts::left_out(&name, *other_pdfs, *not_pdf));
        }
        Status::NotAccepted { file, message } => {
            ui.colored_label(error, texts::not_accepted(&display_name(file), message))
                .on_hover_text(file.display().to_string());
        }
        Status::Extracted { pages, output } => {
            ui.label(texts::extracted(*pages, &display_name(output)))
                .on_hover_text(output.display().to_string());
        }
        Status::SplitDone {
            files,
            pages,
            folder,
        } => {
            ui.label(texts::split_done(
                *pages,
                *files,
                &folder.display().to_string(),
            ));
        }
        Status::OutputIsInput { mode, output } => {
            let name = display_name(output);
            let message = match mode {
                Mode::Extract => texts::extract_onto_input(&name),
                Mode::Split => texts::split_onto_input(&name),
            };
            ui.colored_label(error, message)
                .on_hover_text(output.display().to_string());
        }
        Status::ExistsAfterAddingExtension { output } => {
            ui.colored_label(
                error,
                texts::extract_exists_after_adding_extension(&display_name(output)),
            )
            .on_hover_text(output.display().to_string());
        }
        Status::NameTaken { output } => {
            ui.colored_label(error, texts::split_name_taken(&display_name(output)))
                .on_hover_text(output.display().to_string());
        }
        Status::SplitStopped {
            written,
            parts,
            folder,
            message,
        } => {
            ui.colored_label(
                error,
                texts::split_stopped(*written, *parts, &folder.display().to_string(), message),
            );
        }
        Status::Failed { mode, message } => {
            let message = match mode {
                Mode::Extract => texts::extract_failed(message),
                Mode::Split => texts::split_failed(message),
            };
            ui.colored_label(error, message);
        }
    }
}
