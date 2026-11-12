//! The merge operation: apply the tool's own rules, call the core, and say
//! what happened as a value.
//!
//! Nothing here prints or ends the program. [`run`] returns a report or a
//! failure, and `output.rs` decides how to show either. That split keeps
//! the rules testable without starting a program and reading its output.
//!
//! There is no PDF logic here either: joining the files is the core's job.
//! This module only adds the two rules about what may be written where.

use std::fs;
use std::path::{Path, PathBuf};

use danonpdf_core::{MergeError, error_chain, merge_files, same_file};

use crate::texts;

/// The name of this operation in the JSON report.
pub const OPERATION: &str = "merge";

/// What a merge that worked has to tell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeReport {
    /// Where the merged file was written, exactly as the caller spelled it.
    pub output: PathBuf,
    /// How many input files were read. The same file given twice counts
    /// twice.
    pub inputs: usize,
    /// How many pages the merged file has.
    pub pages: usize,
}

/// Why an operation did not happen.
///
/// Three views of the same failure, for three kinds of reader: `code` for a
/// program that decides what to do next, `message` for a person (or a
/// language model) reading the reason, and `path` for whoever needs the file
/// without digging it out of the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: ErrorCode,
    /// The reason, with every cause behind it, on one line.
    pub message: String,
    /// The file the failure is about. `None` when it is about no file in
    /// particular.
    pub path: Option<PathBuf>,
}

/// The kind of failure, as a caller can test for it.
///
/// An enum and not a string: the compiler then knows every kind there is, so
/// a `match` on it cannot forget one, and a typo is a build error instead of
/// a code that no caller expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// The output is one of the input files. `--force` does not lift this.
    OutputIsInput,
    /// The output already exists and `--force` was not given.
    OutputExists,
    /// There was nothing to merge.
    NoInputs,
    /// An input could not be read, or is not a PDF.
    ReadFailed,
    /// An input needs a password.
    Encrypted,
    /// An input opened, but its structure is damaged.
    BrokenPdf,
    /// The merged file could not be written.
    SaveFailed,
}

impl ErrorCode {
    /// The code as it appears in the JSON report.
    ///
    /// These strings are a promise to every script written against the
    /// tool: a variant may be renamed freely, its string never. That is why
    /// they are spelled out here instead of being derived from the variant
    /// names.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OutputIsInput => "output_is_input",
            Self::OutputExists => "output_exists",
            Self::NoInputs => "no_inputs",
            Self::ReadFailed => "read_failed",
            Self::Encrypted => "encrypted",
            Self::BrokenPdf => "broken_pdf",
            Self::SaveFailed => "save_failed",
        }
    }
}

/// Merges `inputs`, in order, into a new file at `output`.
///
/// Every input is read, as given: no file is skipped for its name, and the
/// same file twice is merged twice. `output` is used as given too: no
/// extension is added.
///
/// Two rules are checked before the core is called, in this order:
///
/// 1. `output` must not be one of the inputs, whatever `force` says. The
///    core could do it safely (it reads everything before it writes), but a
///    caller that names an input as the output has almost always mixed up
///    its arguments, and the result would replace a file it still wants.
/// 2. `output` must not exist yet, unless `force` is true.
///
/// The first rule comes first so that the answer does not depend on `force`:
/// an input always exists, so with the other order the same mistake would
/// be "exists" without `--force` and "is an input" with it.
///
/// Known limit: the second rule looks first and writes later. Another
/// program can create `output` in between, and the save would then replace
/// that new file. Closing the gap needs a "rename, but only if the name is
/// free" that std does not offer on every system; for a tool run by one
/// person or one agent on its own files, the window is accepted.
pub fn run(inputs: &[PathBuf], output: &Path, force: bool) -> Result<MergeReport, Failure> {
    // `same_file` is the core's rule, the one the window uses too, so both
    // refuse the same outputs: another spelling of an input is still it.
    if inputs.iter().any(|input| same_file(input, output)) {
        return Err(Failure {
            code: ErrorCode::OutputIsInput,
            message: texts::output_is_input(output),
            path: Some(output.to_path_buf()),
        });
    }
    if !force && is_taken(output) {
        return Err(Failure {
            code: ErrorCode::OutputExists,
            message: texts::output_exists(output),
            path: Some(output.to_path_buf()),
        });
    }

    // `map` changes the value inside `Ok`, `map_err` the one inside `Err`:
    // the core's page count becomes a report, its error becomes a failure.
    merge_files(inputs, output)
        .map(|pages| MergeReport {
            output: output.to_path_buf(),
            inputs: inputs.len(),
            pages,
        })
        .map_err(|error| failure_from(&error, inputs))
}

/// Whether something already has the name `path`.
///
/// Not `Path::exists`: that follows links, so it answers `false` for a link
/// whose target is gone, and the save would then replace the link without
/// `--force`. `symlink_metadata` looks at the name itself. If it fails for
/// another reason (a folder on the way that cannot be read, say), the name
/// counts as free and the save reports the real problem.
fn is_taken(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

/// Turns the core's error into the tool's failure: a stable code, the whole
/// message, and the file it is about.
///
/// `inputs` is the list the merge was called with. Two kinds of error only
/// carry the place of the input in that list, counted from 1; with the list
/// at hand, the path can be looked up for the caller.
fn failure_from(error: &MergeError, inputs: &[PathBuf]) -> Failure {
    // No `_ =>` arm, on purpose. With one, a variant added to `MergeError`
    // later would fall into it silently and reach callers under a wrong
    // code. Without one, the compiler refuses to build this crate until the
    // new variant is given a code here.
    let (code, path) = match error {
        MergeError::NoInputs => (ErrorCode::NoInputs, None),
        MergeError::Read { path, .. } => (ErrorCode::ReadFailed, Some(path.clone())),
        MergeError::Encrypted { input } => (ErrorCode::Encrypted, nth_input(inputs, *input)),
        MergeError::Broken { input, .. } => (ErrorCode::BrokenPdf, nth_input(inputs, *input)),
        MergeError::Save { path, .. } => (ErrorCode::SaveFailed, Some(path.clone())),
    };
    Failure {
        code,
        // The core's message says what failed; the errors behind it say
        // why. `error_chain` joins them, as the window's status line does.
        message: error_chain(error),
        path,
    }
}

/// The path of input number `number`, counted from 1 as the core does.
/// `None` when there is no such input.
fn nth_input(inputs: &[PathBuf], number: usize) -> Option<PathBuf> {
    // `checked_sub` gives `None` for 0 instead of wrapping below zero, and
    // `get` gives `None` past the end instead of panicking as `inputs[i]`
    // would. `?` passes the first `None` on; `cloned` copies the path out
    // of the borrowed list.
    inputs.get(number.checked_sub(1)?).cloned()
}

#[cfg(test)]
mod tests;
