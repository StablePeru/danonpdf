//! Every text the user can see, in one place.
//!
//! The rest of the app never writes a sentence itself: it asks this module.
//! A translation (Basque first) then means one more module like this one,
//! not a hunt through the drawing code.
//!
//! One exception for now: when a merge fails, the reason comes from the core
//! (`MergeError` and the errors behind it) in English. Translating those
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

/// The file name the save dialog proposes for a merge.
pub const DEFAULT_OUTPUT_NAME: &str = "merged.pdf";

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
pub fn skipped(not_pdf: usize, duplicates: usize) -> String {
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
    fn merged_uses_singular_for_one_page() {
        assert_eq!(merged(1, "a.pdf"), "Merged 1 page into a.pdf.");
        assert_eq!(merged(12, "a.pdf"), "Merged 12 pages into a.pdf.");
    }
}
