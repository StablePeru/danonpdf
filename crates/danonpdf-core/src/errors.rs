//! What can go wrong in the core, and how an error is turned into one line
//! of text for every front end.
//!
//! Every operation (merge, extract, split, counting pages) returns the same
//! [`PdfError`]. They fail in the same ways: an input that cannot be read,
//! that needs a password or that is damaged, and an output that cannot be
//! written. One enum for all of them means one place where a front end
//! decides what each failure means to its user.
//!
//! The price is that the type says a little less than it could: nothing in
//! it tells that a merge never returns `PageOutOfRange`, or that a split
//! never returns `NoInputs`. An enum per operation would say so, and would
//! repeat the four shared failures in each.
//!
//! [`PdfError`] keeps its own message short and hands the lower-level error
//! over through `source()`. Whoever shows the error (the window's status
//! line, the command-line tool) wants both, joined the same way, so the
//! joining lives here once, in [`error_chain`].

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

/// Which input an error is about.
///
/// A merge takes a list of files and tells them apart by their place in
/// it. The operations that read one file (counting its pages, extract,
/// split) have no list, and calling their file "input 1" would be the
/// wording of a merge in the wrong place.
///
/// The difference is settled here, in the core, because this is where it
/// is known: each operation says which of the two it is when it reports
/// the error. Both front ends show the message as it comes, so neither has
/// to reword it, and they cannot end up wording it differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhichInput {
    /// The one file of an operation that reads a single file.
    Only,
    /// The input at this place in the list, counted from 1 in the order
    /// the inputs were given, so the caller (which has the list) can name
    /// the file.
    Numbered(usize),
}

impl fmt::Display for WhichInput {
    /// The words that start the message: "the file" or "input 2".
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Only => write!(f, "the file"),
            Self::Numbered(number) => write!(f, "input {number}"),
        }
    }
}

/// Everything that can stop an operation of the core.
///
/// `input` says which input the error is about (see [`WhichInput`]).
#[derive(Debug)]
pub enum PdfError {
    /// The list of inputs was empty.
    NoInputs,
    /// An input file could not be read, or is not a PDF.
    Read { path: PathBuf, source: lopdf::Error },
    /// An input is encrypted and needs a password to be opened.
    Encrypted { input: WhichInput },
    /// An input opened, but its structure is broken.
    Broken {
        input: WhichInput,
        reason: &'static str,
    },
    /// A page selection names a page that the document does not have:
    /// `page`, in a document of `pages` pages.
    PageOutOfRange { page: usize, pages: usize },
    /// The folder that the new files go into does not exist and could not
    /// be created.
    CreateFolder {
        path: PathBuf,
        source: std::io::Error,
    },
    /// A new document could not be written. Writing reports a plain
    /// input/output error, not a `lopdf::Error`.
    Save {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for PdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `Path` is not always valid text, so it has no `Display` of its own:
        // `display()` borrows it as something printable.
        match self {
            // No operation is named: every operation shares this enum, so
            // "to merge" would be wrong the day another one takes a list
            // of inputs. The caller knows what it asked for.
            Self::NoInputs => write!(f, "there are no input files"),
            Self::Read { path, .. } => write!(f, "cannot read {} as a PDF", path.display()),
            // `{input}` prints a `WhichInput`, which is "the file" or
            // "input 2": the rest of the sentence is the same for both.
            Self::Encrypted { input } => {
                write!(f, "{input} is encrypted and needs a password")
            }
            Self::Broken { input, reason } => write!(f, "{input} is damaged: {reason}"),
            Self::PageOutOfRange { page, pages } => {
                // A `match` is an expression: each arm gives the text for
                // one case, and `_` takes every number that is not 1.
                let length = match pages {
                    1 => "1 page".to_owned(),
                    _ => format!("{pages} pages"),
                };
                write!(
                    f,
                    "page {page} is past the end of the document, which has {length}"
                )
            }
            Self::CreateFolder { path, .. } => {
                write!(f, "cannot create the folder {}", path.display())
            }
            Self::Save { path, .. } => write!(f, "cannot save {}", path.display()),
        }
    }
}

impl Error for PdfError {
    /// The lower-level error behind this one, if any. The message above
    /// leaves it out on purpose, so a caller can print the whole chain
    /// without saying the same thing twice.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // `Read` cannot join the or-pattern of the other two: in an
        // or-pattern a name must have one type, and its `source` is a
        // `lopdf::Error` while theirs is a `std::io::Error`.
        match self {
            Self::Read { source, .. } => Some(source),
            Self::CreateFolder { source, .. } | Self::Save { source, .. } => Some(source),
            Self::NoInputs
            | Self::Encrypted { .. }
            | Self::Broken { .. }
            | Self::PageOutOfRange { .. } => None,
        }
    }
}

/// `error`'s message followed by the message of each error behind it,
/// joined with ": ". For example: "cannot save out.pdf: access denied".
///
/// `&dyn Error` accepts any error type, not only [`PdfError`]: the function
/// only needs the two things every error offers, a message and an optional
/// source.
pub fn error_chain(error: &dyn Error) -> String {
    let mut message = error.to_string();
    // `source()` gives the error behind this one, if any. Walking it until
    // it returns `None` visits the whole chain, however long it is.
    let mut cause = error.source();
    while let Some(inner) = cause {
        message.push_str(": ");
        message.push_str(&inner.to_string());
        cause = inner.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;

    use super::*;

    #[test]
    fn error_messages_name_the_problem() {
        let read = PdfError::Read {
            path: Path::new("a.pdf").to_path_buf(),
            source: lopdf::Error::InvalidPassword,
        };
        assert_eq!(read.to_string(), "cannot read a.pdf as a PDF");
    }

    #[test]
    fn an_input_of_a_list_is_named_by_its_number() {
        // The wording of a merge: the number is what tells its inputs
        // apart.
        assert_eq!(
            PdfError::Encrypted {
                input: WhichInput::Numbered(2)
            }
            .to_string(),
            "input 2 is encrypted and needs a password"
        );
        assert_eq!(
            PdfError::Broken {
                input: WhichInput::Numbered(1),
                reason: "it has no catalog"
            }
            .to_string(),
            "input 1 is damaged: it has no catalog"
        );
    }

    #[test]
    fn the_only_input_of_an_operation_is_not_given_a_number() {
        // Counting pages, extract and split read one file. "input 1" would
        // make their user look for an input 2.
        assert_eq!(
            PdfError::Encrypted {
                input: WhichInput::Only
            }
            .to_string(),
            "the file is encrypted and needs a password"
        );
        assert_eq!(
            PdfError::Broken {
                input: WhichInput::Only,
                reason: "it has no catalog"
            }
            .to_string(),
            "the file is damaged: it has no catalog"
        );
    }

    #[test]
    fn a_page_past_the_end_says_how_long_the_document_is() {
        assert_eq!(
            PdfError::PageOutOfRange { page: 9, pages: 5 }.to_string(),
            "page 9 is past the end of the document, which has 5 pages"
        );
        assert_eq!(
            PdfError::PageOutOfRange { page: 2, pages: 1 }.to_string(),
            "page 2 is past the end of the document, which has 1 page"
        );
    }

    #[test]
    fn a_folder_that_cannot_be_created_is_named_with_the_reason() {
        let error = PdfError::CreateFolder {
            path: PathBuf::from("parts"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        };
        assert_eq!(
            error_chain(&error),
            "cannot create the folder parts: access denied"
        );
    }

    #[test]
    fn error_with_a_source_ends_with_the_message_of_that_source() {
        let error = PdfError::Save {
            path: PathBuf::from("out.pdf"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        };
        assert_eq!(error_chain(&error), "cannot save out.pdf: access denied");
    }

    #[test]
    fn error_without_a_source_is_just_its_message() {
        // The wording names no operation: the enum is shared by all of
        // them, and only the caller knows which one it asked for.
        assert_eq!(error_chain(&PdfError::NoInputs), "there are no input files");
    }
}
