//! The delete operation: write the pages of one PDF file that were not
//! chosen into a new file.
//!
//! Nothing is deleted from the input, which is only read: "deleting" means
//! leaving pages out of the copy. Which pages those are is the core's
//! business, and so is refusing to leave all of them out. This module adds
//! the same two rules as an extraction about what may be written where.

use std::path::Path;

use danonpdf_core::{PageSelection, delete_file};

use crate::outcome::{Failure, Report, check_outputs, failure_from};

/// Writes the pages of `input` that `pages` does not select, in their
/// order, into a new file at `output`.
///
/// `output` must not be `input`, whatever `force` says, and must not exist
/// yet unless `force` is true. A page past the end of the document, and a
/// selection that leaves no page at all, are failures of the core, and
/// nothing is written.
pub fn run(
    input: &Path,
    pages: &PageSelection,
    output: &Path,
    force: bool,
) -> Result<Report, Failure> {
    check_outputs(&[input], &[output], force)?;

    delete_file(input, pages, output)
        // The closure's `pages` is the page count that the core returns:
        // the pages that are left, not the ones that were deleted.
        .map(|pages| Report::Delete {
            output: output.to_path_buf(),
            pages,
        })
        .map_err(|error| failure_from(&error, &[input]))
}
