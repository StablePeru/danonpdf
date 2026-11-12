//! Every text the user can see, in one place.
//!
//! The rest of the app never writes a sentence itself: it asks this module.
//! A translation (Basque first) then means one more module like this one,
//! not a hunt through the drawing code.
//!
//! One exception for now: when a merge fails, the reason comes from the core
//! (`PdfError` and the errors behind it) in English. Translating those
//! means matching on each kind of error here, which can wait until there is
//! a second language.

use std::fmt::Display;

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
        // `rsplit_once` cuts at the last dot: ("merged", "pdf"). The number
        // goes between the two parts, before the extension, so the file
        // still opens as a PDF.
        _ => match name.rsplit_once('.') {
            Some((stem, extension)) => format!("{stem} ({attempt}).{extension}"),
            // A name without a dot has nothing to keep at the end.
            None => format!("{name} ({attempt})"),
        },
    }
}

// Toolbar

pub const ADD_BUTTON: &str = "Add PDFs…";
pub const ADD_HINT: &str = "Choose PDF files to add at the end of the list";
pub const CLEAR_BUTTON: &str = "Clear";
pub const CLEAR_HINT: &str = "Empty the list. The files themselves are not touched.";
pub const MERGE_BUTTON: &str = "Merge…";
pub const MERGE_HINT: &str = "Join every file in the list, in this order, into a new PDF";
pub const MERGE_NEEDS_TWO: &str = "Add at least two PDFs to merge them";

// The list

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
/// The name of the file type filter in both dialogs.
pub const PDF_FILTER: &str = "PDF files";

// Status line

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

// Split view: extracting

/// The file name the save dialog proposes for an extraction from a file
/// whose name, without its extension, is `input_stem`: "report-pages.pdf"
/// for "report.pdf". Numbered like any other proposal when it is taken (see
/// [`numbered_name`]).
pub fn extract_output_name(input_stem: &str) -> String {
    format!("{input_stem}-pages.pdf")
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
    fn extract_output_name_adds_pages_to_the_input_name() {
        assert_eq!(extract_output_name("report"), "report-pages.pdf");
        // Numbered like every other proposal.
        assert_eq!(
            numbered_name(&extract_output_name("report"), 2),
            "report-pages (2).pdf"
        );
    }
}
