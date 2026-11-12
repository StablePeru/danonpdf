//! Every text the user can see, in one place.
//!
//! The rest of the app never writes a sentence itself: it asks this module.
//! A translation (Basque first) then means one more module like this one,
//! not a hunt through the drawing code.
//!
//! One exception for now: when an operation fails, or a page selection is
//! not accepted, the reason comes from the core (`PdfError`,
//! `PageSyntaxError` and the errors behind them) in English. Translating
//! those means matching on each kind of error here, which can wait until
//! there is a second language.

use std::fmt::Display;
use std::path::Path;

/// The name in the title bar and in dialogs. A brand, so never translated.
pub const APP_NAME: &str = "DanonPDF";

/// Title of the message box shown when the window cannot open.
pub const STARTUP_ERROR_TITLE: &str = "DanonPDF cannot start";

/// Body of that message box. `error` is whatever eframe reported.
pub fn startup_error(error: &dyn Display) -> String {
    format!("DanonPDF could not open its window.\n\n{error}")
}

/// The file name the save dialog proposes for a merge, and the base of the
/// numbered names that follow it (see [`numbered_name`]).
pub const DEFAULT_OUTPUT_NAME: &str = "merged.pdf";

/// The `attempt`-th name to propose when `name` is the first choice:
/// `name` itself first, then "merged (2).pdf", "merged (3).pdf"… for
/// "merged.pdf". The numbering copies what file managers do with copies,
/// so it looks familiar.
///
/// Every proposal of the window goes through here, so the numbering looks
/// the same whatever is being saved, and a translation that numbers
/// differently changes one function.
pub fn numbered_name(name: &str, attempt: usize) -> String {
    match attempt {
        // 0 is not a real attempt; treat it as the first one rather than
        // proposing a strange "merged (0).pdf".
        0 | 1 => name.to_owned(),
        // The number goes before the extension, so the file still opens as
        // a PDF: ("merged", "pdf") becomes "merged (2).pdf".
        //
        // Where the extension starts is asked of the standard library and
        // not found by cutting at the last dot, because the two disagree on
        // names that do happen: in ".report" the dot is part of the name (a
        // hidden file on Linux), and cutting there would propose
        // " (2).report". With `Path` the rule is also the one the rest of
        // the app uses when it reads a stem or an extension.
        _ => match name_parts(name) {
            Some((stem, extension)) => format!("{stem} ({attempt}).{extension}"),
            // A name without an extension has nothing to keep at the end.
            None => format!("{name} ({attempt})"),
        },
    }
}

/// `name` cut where its extension starts: ("merged", "pdf") for
/// "merged.pdf". `None` when the name has no extension.
fn name_parts(name: &str) -> Option<(&str, &str)> {
    let name = Path::new(name);
    // `file_stem` and `extension` answer in `OsStr`, the system's own kind
    // of text, because a path need not be valid UTF-8. This one was made
    // from a `&str`, so `to_str` always succeeds; `?` is there because the
    // types cannot know that.
    let stem = name.file_stem()?.to_str()?;
    let extension = name.extension()?.to_str()?;
    Some((stem, extension))
}

// The view switch

pub const VIEW_MERGE: &str = "Merge";
pub const VIEW_MERGE_HINT: &str = "Join several PDFs into one";
pub const VIEW_SPLIT: &str = "Split";
pub const VIEW_SPLIT_HINT: &str = "Take pages out of one PDF, or cut it into several files";

// Merge view: toolbar

pub const ADD_BUTTON: &str = "Add PDFs…";
pub const ADD_HINT: &str = "Choose PDF files to add at the end of the list";
pub const CLEAR_BUTTON: &str = "Clear";
pub const CLEAR_HINT: &str = "Empty the list. The files themselves are not touched.";
pub const MERGE_BUTTON: &str = "Merge…";
pub const MERGE_HINT: &str = "Join every file in the list, in this order, into a new PDF";
pub const MERGE_NEEDS_TWO: &str = "Add at least two PDFs to merge them";

// Merge view: the list

pub const DROP_HINT: &str = "Drop PDF files here, or use \"Add PDFs…\".";
pub const DROP_NOW: &str = "Drop the files to add them to the list.";
pub const MOVE_UP: &str = "Up";
pub const MOVE_UP_HINT: &str = "Merge this file earlier";
pub const MOVE_DOWN: &str = "Down";
pub const MOVE_DOWN_HINT: &str = "Merge this file later";
pub const REMOVE: &str = "Remove";
pub const REMOVE_HINT: &str = "Take this file out of the list. The file itself is not touched.";

// Dialogs

pub const ADD_DIALOG_TITLE: &str = "Add PDFs";
pub const SAVE_DIALOG_TITLE: &str = "Save the merged PDF";
pub const CHOOSE_DIALOG_TITLE: &str = "Choose a PDF";
pub const EXTRACT_DIALOG_TITLE: &str = "Save the extracted pages";
pub const SPLIT_DIALOG_TITLE: &str = "Choose a folder for the new files";
/// The name of the file type filter in every file dialog.
pub const PDF_FILTER: &str = "PDF files";

// Merge view: status line

/// How many files the list holds, shown while there is nothing else to say.
pub fn files_in_list(count: usize) -> String {
    match count {
        0 => "No files yet.".to_owned(),
        1 => "1 PDF in the list. Add at least one more to merge.".to_owned(),
        _ => format!("{count} PDFs in the list, merged from top to bottom."),
    }
}

/// Why some of the files just added are not in the list.
///
/// The list only reports skipped files when at least one count is above
/// zero, but this function does not rely on it: with both at zero it still
/// returns a whole sentence instead of "Skipped .".
pub fn skipped(not_pdf: usize, duplicates: usize) -> String {
    if not_pdf == 0 && duplicates == 0 {
        return "Nothing was skipped.".to_owned();
    }
    // Each part is a phrase, and the parts that apply are joined, so the
    // sentence never mentions a count of zero.
    let mut parts = Vec::new();
    if not_pdf > 0 {
        parts.push(match not_pdf {
            1 => "1 file that is not a PDF".to_owned(),
            _ => format!("{not_pdf} files that are not PDFs"),
        });
    }
    if duplicates > 0 {
        parts.push(match duplicates {
            1 => "1 file already in the list".to_owned(),
            _ => format!("{duplicates} files already in the list"),
        });
    }
    format!("Skipped {}.", parts.join(" and "))
}

/// A merge worked. `output_name` is the file name, without its folder.
pub fn merged(pages: usize, output_name: &str) -> String {
    match pages {
        1 => format!("Merged 1 page into {output_name}."),
        _ => format!("Merged {pages} pages into {output_name}."),
    }
}

/// A merge failed. `reason` is the error and its causes.
pub fn merge_failed(reason: &str) -> String {
    format!("Could not merge: {reason}")
}

/// A merge was refused because the chosen file is one of the inputs.
/// `output_name` is the file name, without its folder.
pub fn output_is_input(output_name: &str) -> String {
    format!(
        "Not merged: {output_name} is one of the files in the list. \
         Choose another name, so that no file is replaced."
    )
}

/// A merge was refused because ".pdf" was added to the typed name and a file
/// with that name already exists. `output_name` is the name with ".pdf",
/// without its folder.
pub fn exists_after_adding_extension(output_name: &str) -> String {
    format!(
        "Not merged: {output_name} already exists. \
         Choose it in the dialog if you want to replace it."
    )
}

// Split view: choosing the file

pub const CHOOSE_BUTTON: &str = "Choose PDF…";
pub const CHOOSE_HINT: &str =
    "Choose the PDF to take pages from. It replaces the one chosen before.";
pub const SPLIT_DROP_HINT: &str = "Drop a PDF file here, or use \"Choose PDF…\".";
pub const SPLIT_DROP_NOW: &str = "Drop the file to use it here.";
/// Shown on the controls that need a file while none is chosen.
pub const NEEDS_FILE: &str = "Choose a PDF first";

/// How long the chosen file is, shown next to its name.
pub fn page_count(pages: usize) -> String {
    match pages {
        1 => "1 page".to_owned(),
        _ => format!("{pages} pages"),
    }
}

// Split view: the two modes

pub const MODE_EXTRACT: &str = "Extract pages";
pub const MODE_EXTRACT_HINT: &str = "Copy some of the pages into one new PDF";
pub const MODE_SPLIT: &str = "Split into files";
pub const MODE_SPLIT_HINT: &str = "Write all the pages into several new PDFs";

// Split view: extracting

pub const PAGES_LABEL: &str = "Pages:";
/// The example shown inside the pages field while it is empty. It is the
/// page syntax of the core, the one `danonpdf-cli --pages` takes.
pub const PAGES_EXAMPLE: &str = "1-3,7,10-";
pub const PAGES_HINT: &str = "Pages separated by commas. 7 is one page, 1-3 is pages 1 to 3, \
     and 10- is page 10 and every page after it. \
     They are written in the order given.";
/// Shown under the pages field while there is nothing to check.
pub const PAGES_PROMPT: &str = "Type the pages to extract, for example 1-3,7,10-";
pub const EXTRACT_BUTTON: &str = "Extract…";
pub const EXTRACT_HINT: &str = "Copy the selected pages, in the order typed, into a new PDF";
pub const EXTRACT_NEEDS_PAGES: &str = "Type which pages to extract first";

/// Shown under the pages field when the text selects pages of the file.
pub fn pages_selected(pages: usize) -> String {
    match pages {
        1 => "1 page selected.".to_owned(),
        _ => format!("{pages} pages selected."),
    }
}

/// Shown under the pages field when the text cannot be used. `reason` is
/// the core's own message.
pub fn pages_invalid(reason: &str) -> String {
    format!("Cannot use these pages: {reason}")
}

/// The file name the save dialog proposes for an extraction from a file
/// whose name, without its extension, is `input_stem`: "report-pages.pdf"
/// for "report.pdf". Numbered like any other proposal when it is taken (see
/// [`numbered_name`]).
pub fn extract_output_name(input_stem: &str) -> String {
    format!("{input_stem}-pages.pdf")
}

// Split view: splitting

pub const PAGES_PER_FILE_LABEL: &str = "Pages per file:";
pub const PAGES_PER_FILE_HINT: &str =
    "How many pages go into each new file. The last file may have fewer.";
pub const SPLIT_BUTTON: &str = "Split…";
pub const SPLIT_HINT: &str = "Choose a folder and write the new files into it";

/// What a split would write: how many files, and the names of the first
/// and the last one. With one file there is no "to".
pub fn split_preview(pages: usize, files: usize, first: &str, last: &str) -> String {
    let pages = page_count(pages);
    match files {
        1 => format!("{pages} into 1 file: {first}"),
        _ => format!("{pages} into {files} files: {first} to {last}"),
    }
}

// Split view: status line

pub const NO_FILE_CHOSEN: &str = "No PDF chosen yet.";

/// Shown while a file is chosen and there is nothing else to say.
/// `file_name` is the chosen file, without its folder.
pub fn file_chosen(file_name: &str) -> String {
    format!("{file_name} is only read: its pages go to new files.")
}

/// A file was chosen out of several. `file_name` is the one in use.
///
/// The view only reports this when at least one count is above zero, but
/// this function does not rely on it: with both at zero it still returns a
/// whole sentence.
pub fn left_out(file_name: &str, other_pdfs: usize, not_pdf: usize) -> String {
    let mut message = format!("Using {file_name}.");
    // Each sentence is added only when it has something to count, so the
    // line never mentions a count of zero.
    match other_pdfs {
        0 => {}
        1 => message.push_str(" Left out 1 other PDF: this view takes one file at a time."),
        _ => message.push_str(&format!(
            " Left out {other_pdfs} other PDFs: this view takes one file at a time."
        )),
    }
    if not_pdf > 0 {
        // The same words as in the Merge view, for the same thing.
        message.push(' ');
        message.push_str(&skipped(not_pdf, 0));
    }
    message
}

/// A file could not be chosen. `reason` is the error and its causes.
pub fn not_accepted(file_name: &str, reason: &str) -> String {
    format!("Cannot use {file_name}: {reason}")
}

/// An extraction worked. `output_name` is the file name, without its folder.
pub fn extracted(pages: usize, output_name: &str) -> String {
    match pages {
        1 => format!("Extracted 1 page into {output_name}."),
        _ => format!("Extracted {pages} pages into {output_name}."),
    }
}

/// A split worked. `folder` is where the files are, as the user chose it.
pub fn split_done(pages: usize, files: usize, folder: &str) -> String {
    let pages = page_count(pages);
    match files {
        1 => format!("Split {pages} into 1 file in {folder}."),
        _ => format!("Split {pages} into {files} files in {folder}."),
    }
}

/// An extraction failed. `reason` is the error and its causes.
pub fn extract_failed(reason: &str) -> String {
    format!("Could not extract: {reason}")
}

/// A split failed before it wrote anything. `reason` is the error and its
/// causes.
pub fn split_failed(reason: &str) -> String {
    format!("Could not split: {reason}")
}

/// An extraction was refused because the chosen name is the file being
/// read. `output_name` is the file name, without its folder.
pub fn extract_onto_input(output_name: &str) -> String {
    format!(
        "Not extracted: {output_name} is the file the pages come from. \
         Choose another name, so that it is not replaced."
    )
}

/// A split was refused because one of its files would be the file being
/// read (only a link can cause that). `output_name` is that file's name.
pub fn split_onto_input(output_name: &str) -> String {
    format!(
        "Not split: {output_name} in that folder is the file the pages come from. \
         Choose another folder."
    )
}

/// An extraction was refused because ".pdf" was added to the typed name and
/// a file with that name already exists. `output_name` is the name with
/// ".pdf", without its folder.
pub fn extract_exists_after_adding_extension(output_name: &str) -> String {
    format!(
        "Not extracted: {output_name} already exists. \
         Choose it in the dialog if you want to replace it."
    )
}

/// A split was refused because one of its names is in use. `output_name`
/// is the first such name. The window never replaces the files of a split,
/// so the advice is another folder.
pub fn split_name_taken(output_name: &str) -> String {
    format!(
        "Not split: {output_name} already exists in that folder, and nothing was written. \
         Choose another folder, or an empty one."
    )
}

/// A split stopped after some of its files. `reason` is the error and its
/// causes; `folder` is where the finished files are.
///
/// When files were kept, the line also says how to try again. Splitting
/// into the same folder a second time would be refused (see
/// [`split_name_taken`]): the files that were kept have the very names the
/// new split would use, and the window never replaces them. Saying so here
/// saves the user from finding it out with a second failure.
pub fn split_stopped(written: usize, parts: usize, folder: &str, reason: &str) -> String {
    let stopped = format!("The split stopped after {written} of {parts} files: {reason}.");
    let kept = match written {
        // Nothing is in the way of a retry, so there is no advice to give.
        0 => return format!("{stopped} No file was written."),
        1 => format!("The 1 file already in {folder} is complete and was kept."),
        _ => format!("The {written} files already in {folder} are complete and were kept."),
    };
    format!(
        "{stopped} {kept} To try again, choose another folder or empty that one first: \
         files that are already there are never replaced."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skipped_names_only_the_parts_that_apply() {
        assert_eq!(skipped(1, 0), "Skipped 1 file that is not a PDF.");
        assert_eq!(skipped(0, 2), "Skipped 2 files already in the list.");
        assert_eq!(
            skipped(3, 1),
            "Skipped 3 files that are not PDFs and 1 file already in the list."
        );
    }

    #[test]
    fn skipped_with_nothing_skipped_is_still_a_sentence() {
        assert_eq!(skipped(0, 0), "Nothing was skipped.");
    }

    #[test]
    fn names_are_numbered_after_the_first() {
        assert_eq!(numbered_name("merged.pdf", 1), "merged.pdf");
        assert_eq!(numbered_name("merged.pdf", 2), "merged (2).pdf");
        assert_eq!(numbered_name("merged.pdf", 10), "merged (10).pdf");
        assert_eq!(numbered_name("merged.pdf", 0), "merged.pdf");
    }

    #[test]
    fn the_number_goes_before_the_last_extension_only() {
        assert_eq!(numbered_name("report.final.pdf", 2), "report.final (2).pdf");
        // Nothing to keep at the end of a name without a dot.
        assert_eq!(numbered_name("notes", 3), "notes (3)");
    }

    #[test]
    fn a_leading_dot_is_part_of_the_name_and_not_an_extension() {
        // ".report" is a hidden file called ".report", with no extension.
        // Cutting at its only dot would give " (2).report": a name that
        // starts with a space and is no longer hidden.
        assert_eq!(numbered_name(".report", 2), ".report (2)");
        // With a real extension after it, only that one is kept at the end.
        assert_eq!(numbered_name(".report.pdf", 2), ".report (2).pdf");
    }

    #[test]
    fn a_dot_that_ends_the_name_keeps_its_place() {
        // For std, "report." has an empty extension: the number goes before
        // the dot, as it does before any other extension.
        assert_eq!(numbered_name("report.", 2), "report (2).");
        // ".." has no stem and no extension at all: nothing is cut.
        assert_eq!(numbered_name("..", 2), ".. (2)");
    }

    #[test]
    fn the_default_merge_name_is_numbered_like_any_other() {
        // Whatever the default name is, the numbered ones share its start and
        // its extension, so changing the constant changes every proposal.
        let (stem, extension) = DEFAULT_OUTPUT_NAME.rsplit_once('.').unwrap();
        assert_eq!(numbered_name(DEFAULT_OUTPUT_NAME, 1), DEFAULT_OUTPUT_NAME);
        assert_eq!(
            numbered_name(DEFAULT_OUTPUT_NAME, 2),
            format!("{stem} (2).{extension}")
        );
    }

    #[test]
    fn output_is_input_names_the_file() {
        assert_eq!(
            output_is_input("a.pdf"),
            "Not merged: a.pdf is one of the files in the list. \
             Choose another name, so that no file is replaced."
        );
    }

    #[test]
    fn exists_after_adding_extension_names_the_file() {
        assert_eq!(
            exists_after_adding_extension("report.pdf"),
            "Not merged: report.pdf already exists. \
             Choose it in the dialog if you want to replace it."
        );
    }

    #[test]
    fn merged_uses_singular_for_one_page() {
        assert_eq!(merged(1, "a.pdf"), "Merged 1 page into a.pdf.");
        assert_eq!(merged(12, "a.pdf"), "Merged 12 pages into a.pdf.");
    }

    // Split view

    #[test]
    fn page_counts_use_singular_for_one_page() {
        assert_eq!(page_count(1), "1 page");
        assert_eq!(page_count(12), "12 pages");
        assert_eq!(pages_selected(1), "1 page selected.");
        assert_eq!(pages_selected(4), "4 pages selected.");
        assert_eq!(extracted(1, "a.pdf"), "Extracted 1 page into a.pdf.");
        assert_eq!(extracted(3, "a.pdf"), "Extracted 3 pages into a.pdf.");
    }

    #[test]
    fn extract_output_name_adds_pages_to_the_input_name() {
        assert_eq!(extract_output_name("report"), "report-pages.pdf");
        // Numbered like every other proposal.
        assert_eq!(
            numbered_name(&extract_output_name("report"), 2),
            "report-pages (2).pdf"
        );
    }

    #[test]
    fn split_preview_names_one_file_or_the_first_and_the_last() {
        assert_eq!(
            split_preview(5, 3, "report-001.pdf", "report-003.pdf"),
            "5 pages into 3 files: report-001.pdf to report-003.pdf"
        );
        assert_eq!(
            split_preview(5, 1, "report-001.pdf", "report-001.pdf"),
            "5 pages into 1 file: report-001.pdf"
        );
        assert_eq!(
            split_preview(1, 1, "report-001.pdf", "report-001.pdf"),
            "1 page into 1 file: report-001.pdf"
        );
    }

    #[test]
    fn left_out_names_only_the_parts_that_apply() {
        assert_eq!(
            left_out("a.pdf", 1, 0),
            "Using a.pdf. Left out 1 other PDF: this view takes one file at a time."
        );
        assert_eq!(
            left_out("a.pdf", 2, 1),
            "Using a.pdf. Left out 2 other PDFs: this view takes one file at a time. \
             Skipped 1 file that is not a PDF."
        );
        assert_eq!(
            left_out("a.pdf", 0, 3),
            "Using a.pdf. Skipped 3 files that are not PDFs."
        );
        assert_eq!(left_out("a.pdf", 0, 0), "Using a.pdf.");
    }

    #[test]
    fn split_done_says_how_many_files_and_where() {
        assert_eq!(
            split_done(5, 3, "parts"),
            "Split 5 pages into 3 files in parts."
        );
        assert_eq!(
            split_done(1, 1, "parts"),
            "Split 1 page into 1 file in parts."
        );
    }

    #[test]
    fn split_stopped_says_how_far_it_got_what_was_kept_and_how_to_retry() {
        assert_eq!(
            split_stopped(2, 5, "parts", "cannot save x.pdf: disk full"),
            "The split stopped after 2 of 5 files: cannot save x.pdf: disk full. \
             The 2 files already in parts are complete and were kept. \
             To try again, choose another folder or empty that one first: \
             files that are already there are never replaced."
        );
        assert_eq!(
            split_stopped(1, 5, "parts", "boom"),
            "The split stopped after 1 of 5 files: boom. \
             The 1 file already in parts is complete and was kept. \
             To try again, choose another folder or empty that one first: \
             files that are already there are never replaced."
        );
    }

    #[test]
    fn split_stopped_before_any_file_has_nothing_in_the_way_of_a_retry() {
        // No file was left behind, so the same folder can be chosen again
        // and the advice about another folder would only mislead.
        assert_eq!(
            split_stopped(0, 5, "parts", "cannot create the folder parts: denied"),
            "The split stopped after 0 of 5 files: cannot create the folder parts: denied. \
             No file was written."
        );
    }

    #[test]
    fn failures_and_refused_files_carry_the_reason_they_were_given() {
        assert_eq!(
            extract_failed("cannot save out.pdf: access denied"),
            "Could not extract: cannot save out.pdf: access denied"
        );
        assert_eq!(
            split_failed("cannot read a.pdf as a PDF: no such file"),
            "Could not split: cannot read a.pdf as a PDF: no such file"
        );
        assert_eq!(
            not_accepted("locked.pdf", "input 1 is encrypted and needs a password"),
            "Cannot use locked.pdf: input 1 is encrypted and needs a password"
        );
        assert_eq!(
            pages_invalid("page 9 is past the end of the document, which has 5 pages"),
            "Cannot use these pages: page 9 is past the end of the document, which has 5 pages"
        );
    }

    #[test]
    fn the_idle_line_names_the_chosen_file() {
        assert_eq!(
            file_chosen("report.pdf"),
            "report.pdf is only read: its pages go to new files."
        );
    }

    #[test]
    fn refusals_of_the_split_view_name_the_file() {
        assert_eq!(
            extract_onto_input("report.pdf"),
            "Not extracted: report.pdf is the file the pages come from. \
             Choose another name, so that it is not replaced."
        );
        assert_eq!(
            extract_exists_after_adding_extension("out.pdf"),
            "Not extracted: out.pdf already exists. \
             Choose it in the dialog if you want to replace it."
        );
        assert_eq!(
            split_name_taken("report-002.pdf"),
            "Not split: report-002.pdf already exists in that folder, and nothing was written. \
             Choose another folder, or an empty one."
        );
        assert_eq!(
            split_onto_input("report-002.pdf"),
            "Not split: report-002.pdf in that folder is the file the pages come from. \
             Choose another folder."
        );
    }
}
