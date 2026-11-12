//! The extract operation: copy chosen pages of one PDF file into a new
//! file.
//!
//! Which pages, and in which order, is the core's business: the selection
//! arrives here already parsed (clap does that while it reads the command
//! line, see `cli.rs`), and the core checks it against the document. This
//! module adds the same two rules as a merge about what may be written
//! where.

use std::path::Path;

use danonpdf_core::{PageSelection, extract_file};

use crate::outcome::{Failure, Report, check_outputs, failure_from};

/// Writes the pages of `input` that `pages` selects, in the order it gives
/// them, into a new file at `output`.
///
/// `output` must not be `input`, whatever `force` says, and must not exist
/// yet unless `force` is true. A page past the end of the document is a
/// failure of the core, and nothing is written.
pub fn run(
    input: &Path,
    pages: &PageSelection,
    output: &Path,
    force: bool,
) -> Result<Report, Failure> {
    check_outputs(&[input], &[output], force)?;

    extract_file(input, pages, output)
        // The closure's `pages` is the page count that the core returns;
        // it hides the selection of the same name inside the closure only.
        .map(|pages| Report::Extract {
            output: output.to_path_buf(),
            pages,
        })
        .map_err(|error| failure_from(&error, &[input]))
}
