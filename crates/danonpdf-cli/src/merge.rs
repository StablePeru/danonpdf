//! The merge operation: apply the tool's own rules, call the core, and say
//! what happened as a value.
//!
//! There is no PDF logic here: joining the files is the core's job. This
//! module only adds the two rules about what may be written where, which it
//! shares with the other operations (see `check_outputs` in `outcome.rs`).

use std::path::{Path, PathBuf};

use danonpdf_core::merge_files;

use crate::outcome::{Failure, Report, check_outputs, failure_from};

/// Merges `inputs`, in order, into a new file at `output`.
///
/// Every input is read, as given: no file is skipped for its name, and the
/// same file twice is merged twice. `output` is used as given too: no
/// extension is added.
///
/// `output` must not be one of the inputs, whatever `force` says, and must
/// not exist yet unless `force` is true.
pub fn run(inputs: &[PathBuf], output: &Path, force: bool) -> Result<Report, Failure> {
    // `?` returns the failure from `run` if a rule is broken. `&[output]`
    // is a list of one: the rules are written for any number of outputs.
    check_outputs(inputs, &[output], force)?;

    // `map` changes the value inside `Ok`, `map_err` the one inside `Err`:
    // the core's page count becomes a report, its error becomes a failure.
    merge_files(inputs, output)
        .map(|pages| Report::Merge {
            output: output.to_path_buf(),
            inputs: inputs.len(),
            pages,
        })
        .map_err(|error| failure_from(&error, inputs))
}

#[cfg(test)]
mod tests;
