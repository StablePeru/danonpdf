//! The PDF logic of DanonPDF. It knows nothing about windows or buttons, so
//! every piece of it can be tested without a screen.

mod document;
mod errors;
mod merge;
mod pages;
mod paths;
mod save;
mod split;
#[cfg(test)]
mod test_support;

// The modules stay private; these names are their public face, so callers
// write `danonpdf_core::merge_files` and not the longer path.
pub use errors::{PdfError, WhichInput, error_chain};
pub use merge::{merge_documents, merge_files};
pub use pages::{PageSelection, PageSyntaxError};
pub use paths::{name_is_taken, same_file};
pub use split::{SplitError, SplitPlan, extract_file, page_count, plan_split, split_output_paths};

use std::path::Path;

/// Returns `true` when `path` names a PDF file by its extension, in any
/// letter case (`report.pdf`, `REPORT.PDF`, `Report.Pdf`).
///
/// It only looks at the name, never at the file on disk: it is a quick
/// filter for files dropped onto the window, not a proof that the content
/// is a valid PDF.
pub fn is_pdf_path(path: &Path) -> bool {
    // `extension()` returns an `Option<&OsStr>`: `None` when there is no
    // extension at all. `is_some_and` answers "is there one, and does it
    // match?" in one step, without unwrapping.
    //
    // The comparison stays on `OsStr` on purpose: file names are not always
    // valid UTF-8, and converting to `str` first would reject them.
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercase_pdf_extension_is_pdf() {
        assert!(is_pdf_path(Path::new("invoice.pdf")));
    }

    #[test]
    fn uppercase_pdf_extension_is_pdf() {
        assert!(is_pdf_path(Path::new("INVOICE.PDF")));
    }

    #[test]
    fn mixed_case_extension_in_a_folder_is_pdf() {
        assert!(is_pdf_path(Path::new("docs/report.Pdf")));
    }

    #[test]
    fn other_extension_is_not_pdf() {
        assert!(!is_pdf_path(Path::new("photo.jpg")));
    }

    #[test]
    fn name_without_extension_is_not_pdf() {
        assert!(!is_pdf_path(Path::new("pdf")));
    }

    #[test]
    fn only_the_last_extension_counts() {
        assert!(!is_pdf_path(Path::new("archive.pdf.zip")));
    }

    #[test]
    fn hidden_file_named_pdf_has_no_extension() {
        // std::path treats a leading dot as part of the name, not as the
        // start of an extension: ".pdf" is a hidden file called ".pdf".
        assert!(!is_pdf_path(Path::new(".pdf")));
    }

    #[test]
    fn trailing_dot_is_an_empty_extension() {
        // "report." has an extension, but it is the empty string.
        assert!(!is_pdf_path(Path::new("report.")));
    }

    #[test]
    fn longer_extension_starting_with_pdf_is_not_pdf() {
        // The whole extension must be "pdf", not merely start with it.
        assert!(!is_pdf_path(Path::new("notes.pdfx")));
    }

    // Linux file names are bytes, not text, so they can be invalid UTF-8.
    // Windows names are UTF-16 and are built differently, so this test only
    // exists on Unix-like systems.
    #[cfg(unix)]
    #[test]
    fn non_utf8_file_name_with_pdf_extension_is_pdf() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // 0xE9 is "é" in Latin-1, but on its own it is not valid UTF-8.
        let name = OsStr::from_bytes(b"caf\xe9.pdf");
        assert!(name.to_str().is_none(), "the name must not be valid UTF-8");
        assert!(is_pdf_path(Path::new(name)));
    }
}
