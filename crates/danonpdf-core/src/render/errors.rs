//! What can stop a page from being drawn, or its image from being written.

use std::error::Error;
use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::{PdfError, WhichInput};

/// The lower-level error behind a [`RenderError`], with its type erased.
///
/// `dyn Error` means "some error type, whichever it is": the caller can
/// print it and walk its `source()`, and never learns that it came from the
/// library that loads PDFium. That keeps the engine out of the public face
/// of this crate, so another engine can take its place later.
///
/// `Send + Sync` is part of the type on purpose. Without them the compiler
/// would have to assume that the hidden error cannot leave its thread, and
/// then neither could a [`RenderError`]: a worker thread that draws pages
/// could not send its failures back to the window.
pub type Cause = Box<dyn Error + Send + Sync + 'static>;

/// Everything that can stop a page from being drawn or saved as an image.
///
/// It is its own type, not more variants of [`PdfError`], for three reasons:
///
/// - Drawing fails in ways that no other operation can: the library that
///   draws may be missing. A front end that never draws a page should not
///   have to say what it does about that.
/// - [`PdfError::Read`] carries the error of the library that reads PDF for
///   merging and splitting. A file that PDFium refuses has no such error
///   to give.
/// - The two engines do not always agree on a file (see the notes on
///   `Renderer::render_page`), so "damaged" here means "PDFium says so".
///
/// The messages follow the wording of [`PdfError`] wherever the meaning is
/// the same, so a user reads one voice.
#[derive(Debug)]
pub enum RenderError {
    /// The program could not tell which folder it runs from, so there is no
    /// default place to look for the PDFium library.
    LibraryFolderUnknown { source: io::Error },
    /// There is no PDFium library at `path`: an absolute path, file name
    /// included, also when the folder was given as a relative one.
    LibraryMissing { path: PathBuf },
    /// There is a file at `path` (absolute, like the one above), and the
    /// system could not load it as the PDFium library: it is not a
    /// library, it is one for another kind of computer, or it lacks
    /// functions that this program needs.
    ///
    /// One rare case keeps `path` as it was given: the system could not
    /// say what the current folder is, so a relative path could not be
    /// made absolute, and nothing was loaded for that reason.
    LibraryFailed { path: PathBuf, source: Cause },
    /// PDFium is already loaded from `loaded`, and `asked` is another file
    /// (both absolute). A program can load PDFium once: it cannot be
    /// swapped while running.
    LibraryAlreadyLoaded { loaded: PathBuf, asked: PathBuf },
    /// The file could not be opened or read at all.
    Read { path: PathBuf, source: io::Error },
    /// The file was read, and it is not a PDF or is too damaged to open.
    NotPdf { path: PathBuf },
    /// The file is encrypted and needs a password to be opened.
    Encrypted,
    /// The file is protected by a security scheme that PDFium does not
    /// support. A password would not help.
    UnsupportedProtection,
    /// The file opened, and something inside it is broken.
    Broken { reason: &'static str },
    /// The document has no page `page`: it has `pages` pages. The same
    /// meaning as [`PdfError::PageOutOfRange`].
    PageOutOfRange { page: usize, pages: usize },
    /// The width asked for is 0 or above `maximum`.
    WidthOutOfRange { width: u32, maximum: u32 },
    /// PDFium reported a failure without a reason, while `doing` this.
    /// `detail` is what it did say, for a bug report.
    Engine { doing: &'static str, detail: String },
    /// The image could not be written to `path`.
    Save { path: PathBuf, source: io::Error },
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LibraryFolderUnknown { .. } => write!(
                f,
                "cannot tell which folder the program runs from, to look for the PDFium library there"
            ),
            Self::LibraryMissing { path } => {
                write!(f, "the PDFium library is missing: {}", path.display())
            }
            Self::LibraryFailed { path, .. } => {
                write!(f, "cannot load the PDFium library {}", path.display())
            }
            Self::LibraryAlreadyLoaded { loaded, asked } => write!(
                f,
                "the PDFium library is already loaded from {}, so it cannot be loaded from {}",
                loaded.display(),
                asked.display()
            ),
            Self::Read { path, .. } => write!(f, "cannot read {}", path.display()),
            // The words of `PdfError::Read`.
            Self::NotPdf { path } => write!(f, "cannot read {} as a PDF", path.display()),
            // Drawing reads one file, so it is "the file" and never
            // "input 2". `WhichInput::Only` supplies those words, and
            // `PdfError` the rest of each sentence: one wording, kept in
            // one place.
            //
            // `fmt::Display::fmt(&value, f)` and not `value.fmt(f)`:
            // `PdfError` has two methods called `fmt`, one from `Display`
            // and one from `Debug`, and this names the one that is meant.
            Self::Encrypted => fmt::Display::fmt(
                &PdfError::Encrypted {
                    input: WhichInput::Only,
                },
                f,
            ),
            Self::UnsupportedProtection => write!(
                f,
                "{} is protected in a way that cannot be opened",
                WhichInput::Only
            ),
            Self::Broken { reason } => fmt::Display::fmt(
                &PdfError::Broken {
                    input: WhichInput::Only,
                    reason,
                },
                f,
            ),
            Self::PageOutOfRange { page, pages } => fmt::Display::fmt(
                &PdfError::PageOutOfRange {
                    page: *page,
                    pages: *pages,
                },
                f,
            ),
            Self::WidthOutOfRange { width, maximum } => write!(
                f,
                "the width must be between 1 and {maximum} pixels, not {width}"
            ),
            Self::Engine { doing, detail } => {
                write!(f, "PDFium failed while {doing} ({detail})")
            }
            Self::Save { path, .. } => write!(f, "cannot save {}", path.display()),
        }
    }
}

impl Error for RenderError {
    /// The lower-level error behind this one, if any, so that
    /// `error_chain` can print both without saying anything twice.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::LibraryFolderUnknown { source }
            | Self::Read { source, .. }
            | Self::Save { source, .. } => Some(source),
            // A `Box<dyn Error + Send + Sync>` is not itself a
            // `&dyn Error`: `as_ref` borrows what is in the box, and the
            // compiler then drops the two markers that `source()` does not
            // promise.
            Self::LibraryFailed { source, .. } => Some(source.as_ref()),
            Self::LibraryMissing { .. }
            | Self::LibraryAlreadyLoaded { .. }
            | Self::NotPdf { .. }
            | Self::Encrypted
            | Self::UnsupportedProtection
            | Self::Broken { .. }
            | Self::PageOutOfRange { .. }
            | Self::WidthOutOfRange { .. }
            | Self::Engine { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error_chain;

    #[test]
    fn a_missing_library_is_named_with_its_full_path() {
        let error = RenderError::LibraryMissing {
            path: PathBuf::from("app/pdfium.dll"),
        };
        assert_eq!(
            error.to_string(),
            "the PDFium library is missing: app/pdfium.dll"
        );
    }

    #[test]
    fn a_library_that_does_not_load_ends_with_the_reason() {
        let error = RenderError::LibraryFailed {
            path: PathBuf::from("app/pdfium.dll"),
            source: "not a valid library".into(),
        };
        assert_eq!(
            error_chain(&error),
            "cannot load the PDFium library app/pdfium.dll: not a valid library"
        );
    }

    #[test]
    fn a_second_library_names_both_files() {
        let error = RenderError::LibraryAlreadyLoaded {
            loaded: PathBuf::from("a/pdfium.dll"),
            asked: PathBuf::from("b/pdfium.dll"),
        };
        assert_eq!(
            error.to_string(),
            "the PDFium library is already loaded from a/pdfium.dll, so it cannot be loaded from b/pdfium.dll"
        );
    }

    #[test]
    fn the_file_is_worded_as_the_other_operations_word_it() {
        assert_eq!(
            RenderError::NotPdf {
                path: PathBuf::from("a.pdf")
            }
            .to_string(),
            "cannot read a.pdf as a PDF"
        );
        assert_eq!(
            RenderError::Encrypted.to_string(),
            "the file is encrypted and needs a password"
        );
        assert_eq!(
            RenderError::UnsupportedProtection.to_string(),
            "the file is protected in a way that cannot be opened"
        );
        assert_eq!(
            RenderError::Broken {
                reason: "a page cannot be read"
            }
            .to_string(),
            "the file is damaged: a page cannot be read"
        );
        assert_eq!(
            RenderError::PageOutOfRange { page: 4, pages: 3 }.to_string(),
            "page 4 is past the end of the document, which has 3 pages"
        );
    }

    #[test]
    fn a_width_out_of_range_says_what_is_allowed() {
        assert_eq!(
            RenderError::WidthOutOfRange {
                width: 0,
                maximum: 4096
            }
            .to_string(),
            "the width must be between 1 and 4096 pixels, not 0"
        );
    }

    #[test]
    fn errors_about_files_end_with_the_reason() {
        let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        assert_eq!(
            error_chain(&RenderError::Read {
                path: PathBuf::from("a.pdf"),
                source: denied(),
            }),
            "cannot read a.pdf: access denied"
        );
        assert_eq!(
            error_chain(&RenderError::Save {
                path: PathBuf::from("page.png"),
                source: denied(),
            }),
            "cannot save page.png: access denied"
        );
    }

    #[test]
    fn an_engine_failure_says_what_it_was_doing() {
        let error = RenderError::Engine {
            doing: "drawing the page",
            detail: "Unknown".to_owned(),
        };
        assert_eq!(
            error.to_string(),
            "PDFium failed while drawing the page (Unknown)"
        );
    }
}
