//! The info operation: say how many pages a PDF file has.
//!
//! It writes nothing, so it has no rule of its own to apply. A caller needs
//! it before `extract`: a page past the end of the document is a failure,
//! and this is how to learn where the end is.

use std::path::Path;

use danonpdf_core::page_count;

use crate::outcome::{Failure, Report, failure_from};

/// Counts the pages of `input`.
pub fn run(input: &Path) -> Result<Report, Failure> {
    page_count(input)
        .map(|pages| Report::Info {
            input: input.to_path_buf(),
            pages,
        })
        // `&[input]` is the list of inputs, with its one entry: an error
        // about "the file" then finds its path there.
        .map_err(|error| failure_from(&error, &[input]))
}
