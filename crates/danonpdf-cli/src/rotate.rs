//! The rotate operation: write every page of one PDF file into a new file,
//! with some of the pages turned.
//!
//! Which pages, and how a turn is added to the rotation a page already has,
//! is the core's business. Both arrive here already parsed (clap does that
//! while it reads the command line, see `cli.rs`): the selection as a
//! `PageSelection` and the degrees as a `Turn`. This module adds the same
//! two rules as an extraction about what may be written where.

use std::path::Path;

use danonpdf_core::{PageSelection, Turn, rotate_file};

use crate::outcome::{Failure, Report, check_outputs, failure_from};

/// Writes every page of `input`, in its order, into a new file at `output`,
/// with the pages that `pages` selects turned to the right by `turn`.
///
/// `output` must not be `input`, whatever `force` says, and must not exist
/// yet unless `force` is true. A page past the end of the document is a
/// failure of the core, and nothing is written.
pub fn run(
    input: &Path,
    pages: &PageSelection,
    turn: Turn,
    output: &Path,
    force: bool,
) -> Result<Report, Failure> {
    // "Turn these pages in place" is the natural thing to ask of a
    // rotation, and it is refused like everywhere else: the tool never
    // writes onto a file it was given to read (see `check_outputs`).
    check_outputs(&[input], &[output], force)?;

    rotate_file(input, pages, turn, output)
        // The closure's `pages` is the page count that the core returns;
        // it hides the selection of the same name inside the closure only.
        .map(|pages| Report::Rotate {
            output: output.to_path_buf(),
            pages,
        })
        .map_err(|error| failure_from(&error, &[input]))
}
