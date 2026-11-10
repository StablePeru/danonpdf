//! The PDF logic of DanonPDF. It knows nothing about windows or buttons, so
//! every piece of it can be tested without a screen.

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
}
