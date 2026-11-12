//! How an error is turned into one line of text, for every front end.
//!
//! [`MergeError`](crate::MergeError) keeps its own message short and hands
//! the lower-level error over through `source()`. Whoever shows the error
//! (the window's status line, the command-line tool) wants both, joined the
//! same way, so the joining lives here once.

use std::error::Error;

/// `error`'s message followed by the message of each error behind it,
/// joined with ": ". For example: "cannot save out.pdf: access denied".
///
/// `&dyn Error` accepts any error type, not only [`MergeError`]: the
/// function only needs the two things every error offers, a message and an
/// optional source.
///
/// [`MergeError`]: crate::MergeError
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
    use std::path::PathBuf;

    use super::*;
    use crate::MergeError;

    #[test]
    fn error_with_a_source_ends_with_the_message_of_that_source() {
        let error = MergeError::Save {
            path: PathBuf::from("out.pdf"),
            source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
        };
        assert_eq!(error_chain(&error), "cannot save out.pdf: access denied");
    }

    #[test]
    fn error_without_a_source_is_just_its_message() {
        assert_eq!(
            error_chain(&MergeError::NoInputs),
            "there are no input files to merge"
        );
    }
}
