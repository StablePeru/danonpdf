//! What can go wrong in the core, and how an error is turned into one line
//! of text for every front end.
//!
//! Every operation (merge today, others later) returns the same
//! [`PdfError`]. They fail in the same ways: an input that cannot be read,
//! that needs a password or that is damaged, and an output that cannot be
//! written. One enum for all of them means one place where a front end
//! decides what each failure means to its user.
//!
//! [`PdfError`] keeps its own message short and hands the lower-level error
//! over through `source()`. Whoever shows the error (the window's status
//! line, the command-line tool) wants both, joined the same way, so the
//! joining lives here once, in [`error_chain`].

use std::error::Error;
use std::fmt;
use std::path::PathBuf;

/// Everything that can stop an operation of the core.
///
/// `input` counts the inputs from 1, in the order they were given, so the
/// caller (which has the list) can name the file.
#[derive(Debug)]
pub enum PdfError {
    /// The list of inputs was empty.
    NoInputs,
    /// An input file could not be read, or is not a PDF.
    Read { path: PathBuf, source: lopdf::Error },
    /// An input is encrypted and needs a password to be opened.
    Encrypted { input: usize },
    /// An input opened, but its structure is broken.
    Broken { input: usize, reason: &'static str },
    /// The new document could not be written. Writing reports a plain
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
            Self::NoInputs => write!(f, "there are no input files to merge"),
            Self::Read { path, .. } => write!(f, "cannot read {} as a PDF", path.display()),
            Self::Encrypted { input } => {
                write!(f, "input {input} is encrypted and needs a password")
            }
            Self::Broken { input, reason } => write!(f, "input {input} is damaged: {reason}"),
            Self::Save { path, .. } => write!(f, "cannot save {}", path.display()),
        }
    }
}

impl Error for PdfError {
    /// The lower-level error behind this one, if any. The message above
    /// leaves it out on purpose, so a caller can print the whole chain
    /// without saying the same thing twice.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        // Two arms, not `Read { source, .. } | Save { source, .. }`: in an
        // or-pattern a name must have one type, and these two differ.
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Save { source, .. } => Some(source),
            Self::NoInputs | Self::Encrypted { .. } | Self::Broken { .. } => None,
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
        assert_eq!(
            PdfError::Encrypted { input: 2 }.to_string(),
            "input 2 is encrypted and needs a password"
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
        assert_eq!(
            error_chain(&PdfError::NoInputs),
            "there are no input files to merge"
        );
    }
}
