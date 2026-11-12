//! Every sentence the tool prints while it runs, in one place.
//!
//! The same idea as `texts.rs` in the desktop app: the rest of the code asks
//! this module for a sentence and never writes one itself, so the wording
//! can be reviewed, and later translated, without reading the logic.
//!
//! Two kinds of text are not here:
//!
//! - The help (`--help`) lives in `cli.rs`, next to the arguments it
//!   describes, because clap builds it from them.
//! - The reason a merge failed inside the core comes from the core
//!   (`PdfError` and the errors behind it).
//!
//! Sentences that follow "error: " start in lower case, as the core's do, so
//! the two read the same on the screen.

use std::path::Path;

/// The line printed when a merge worked, without `--json`.
///
/// `Path` has no `Display` of its own, because a path is not always valid
/// text: `display()` borrows it as something printable, with "�" in place
/// of whatever is not valid UTF-8.
pub fn merged(inputs: usize, pages: usize, output: &Path) -> String {
    // Each count picks its own noun, so "1 file" and "1 page" read right.
    let files = match inputs {
        1 => "1 file".to_owned(),
        _ => format!("{inputs} files"),
    };
    let pages = match pages {
        1 => "1 page".to_owned(),
        _ => format!("{pages} pages"),
    };
    format!("Merged {files} ({pages}) into {}", output.display())
}

/// Why a merge was refused: `output` is one of the files to merge.
pub fn output_is_input(output: &Path) -> String {
    format!(
        "the output {} is one of the input files; nothing was written",
        output.display()
    )
}

/// Why a merge was refused: `output` is already there and `--force` was not
/// given.
pub fn output_exists(output: &Path) -> String {
    format!(
        "the output {} already exists; pass --force to replace it",
        output.display()
    )
}

/// The line printed on stderr when the operation failed, without `--json`.
/// `message` is the error with its whole chain of causes.
pub fn error_line(message: &str) -> String {
    format!("error: {message}")
}

/// Why the command line was refused: a merge needs `needed` inputs and only
/// `given` were named. clap prints it after its own "error: ".
///
/// Only the verb follows a count here. "input files" stays plural because
/// `needed` is the minimum for a merge, which is never below two.
pub fn too_few_inputs(needed: usize, given: usize) -> String {
    let verb = match given {
        1 => "was",
        _ => "were",
    };
    format!("merge needs at least {needed} input files; only {given} {verb} given")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merged_counts_files_and_pages_and_names_the_output() {
        assert_eq!(
            merged(3, 12, Path::new("out.pdf")),
            "Merged 3 files (12 pages) into out.pdf"
        );
    }

    #[test]
    fn merged_uses_the_singular_for_one_file_or_one_page() {
        // The tool never merges one file (the parser asks for two), but the
        // sentence does not rely on that.
        assert_eq!(
            merged(1, 1, Path::new("out.pdf")),
            "Merged 1 file (1 page) into out.pdf"
        );
    }

    #[test]
    fn output_is_input_names_the_file_and_says_nothing_was_written() {
        assert_eq!(
            output_is_input(Path::new("a.pdf")),
            "the output a.pdf is one of the input files; nothing was written"
        );
    }

    #[test]
    fn output_exists_names_the_file_and_the_way_out() {
        assert_eq!(
            output_exists(Path::new("out.pdf")),
            "the output out.pdf already exists; pass --force to replace it"
        );
    }

    #[test]
    fn too_few_inputs_says_how_many_are_needed_and_how_many_came() {
        assert_eq!(
            too_few_inputs(2, 1),
            "merge needs at least 2 input files; only 1 was given"
        );
    }

    #[test]
    fn too_few_inputs_uses_the_plural_unless_exactly_one_came() {
        // clap already refuses a merge with no input at all, so 0 does not
        // reach this sentence today. It must still read right if the
        // minimum ever grows, or if that check moves here.
        assert_eq!(
            too_few_inputs(2, 0),
            "merge needs at least 2 input files; only 0 were given"
        );
        assert_eq!(
            too_few_inputs(3, 2),
            "merge needs at least 3 input files; only 2 were given"
        );
    }

    #[test]
    fn error_line_starts_with_error() {
        assert_eq!(error_line("it broke"), "error: it broke");
    }
}
