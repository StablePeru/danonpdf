//! The split operation: write the pages of one PDF file into several
//! files, a fixed number of pages in each.
//!
//! The core does it in two steps, and this module stands between them. The
//! core first reads the input and names the files it would write (a plan);
//! here those names are checked against the tool's rules; only then is the
//! plan carried out. A split that is refused has therefore written nothing,
//! not even the folder.

use std::num::NonZeroUsize;
use std::path::Path;

use danonpdf_core::{error_chain, plan_split};

use crate::outcome::{Failure, Report, check_outputs, failure_from};

/// Splits `input` into files of `every` pages each (the last one may have
/// fewer) inside `output_dir`, which is created if it is missing.
///
/// The files are named by the core (`split_output_paths`). None of them may
/// be `input`, whatever `force` says, and none may exist yet unless `force`
/// is true. Both are checked for every file before the first one is
/// written.
///
/// If writing stops midway, the failure lists the files already written in
/// its `written` field. They are complete, and they are kept.
pub fn run(
    input: &Path,
    every: NonZeroUsize,
    output_dir: &Path,
    force: bool,
) -> Result<Report, Failure> {
    let plan =
        plan_split(input, every, output_dir).map_err(|error| failure_from(&error, &[input]))?;
    check_outputs(&[input], plan.outputs(), force)?;

    // Read before `write`, which takes the plan by value: afterwards there
    // is no plan left to ask.
    let pages = plan.pages();
    plan.write()
        .map(|outputs| Report::Split { outputs, pages })
        .map_err(|stopped| {
            // The code and the path are those of the error that stopped
            // the split; the message starts one step earlier, with how far
            // it got, and the files it finished go along.
            let mut failure = failure_from(&stopped.error, &[input]);
            failure.message = error_chain(&stopped);
            failure.written = stopped.written;
            failure
        })
}
