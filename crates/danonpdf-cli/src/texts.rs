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
//! - The reason an operation failed inside the core comes from the core
//!   (`PdfError` and the errors behind it), and so does the reason a page
//!   selection is not one (`PageSyntaxError`).
//!
//! Sentences that follow "error: " start in lower case, as the core's do, so
//! the two read the same on the screen.

use std::path::{Path, PathBuf};

/// `count` with the noun that fits it: "1 page", "3 pages", "0 pages".
fn counted(count: usize, singular: &str, plural: &str) -> String {
    // A `match` on a number: the first arm takes exactly 1, and `_` takes
    // every other value.
    match count {
        1 => format!("1 {singular}"),
        _ => format!("{count} {plural}"),
    }
}

/// The line printed when a merge worked, without `--json`.
///
/// `Path` has no `Display` of its own, because a path is not always valid
/// text: `display()` borrows it as something printable, with "�" in place
/// of whatever is not valid UTF-8.
pub fn merged(inputs: usize, pages: usize, output: &Path) -> String {
    // Each count picks its own noun, so "1 file" and "1 page" read right.
    let files = counted(inputs, "file", "files");
    let pages = counted(pages, "page", "pages");
    format!("Merged {files} ({pages}) into {}", output.display())
}

/// The line printed when the pages of `input` were counted, without
/// `--json`.
pub fn page_count(input: &Path, pages: usize) -> String {
    let pages = counted(pages, "page", "pages");
    format!("{} has {pages}", input.display())
}

/// The line printed when pages were extracted, without `--json`.
pub fn extracted(pages: usize, output: &Path) -> String {
    let pages = counted(pages, "page", "pages");
    format!("Extracted {pages} into {}", output.display())
}

/// The line printed when a file was split, without `--json`. `outputs` are
/// the files that were written, in order.
///
/// It names the first and the last file, not all of them: a split can
/// write hundreds, and the names in between follow from those two.
pub fn split(pages: usize, outputs: &[PathBuf]) -> String {
    let pages = counted(pages, "page", "pages");
    let files = counted(outputs.len(), "file", "files");
    // A slice can be matched by its shape: no element, exactly one, or a
    // first and a last with anything (`..`) in between. The three arms
    // cover every length, so there is no `_` arm.
    match outputs {
        [] => format!("Split {pages} into {files}"),
        [only] => format!("Split {pages} into {files}: {}", only.display()),
        [first, .., last] => format!(
            "Split {pages} into {files}: {} to {}",
            first.display(),
            last.display()
        ),
    }
}

/// Why an operation was refused: `output` is one of the files it reads.
pub fn output_is_input(output: &Path) -> String {
    format!(
        "the output {} is one of the input files; nothing was written",
        output.display()
    )
}

/// Why an operation was refused: `output` is already there and `--force`
/// was not given.
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

/// Why the command line was refused: `--every` did not get a number of
/// pages. clap prints it after "error: invalid value '...' for '--every
/// <N>': ".
pub fn not_a_page_count() -> String {
    "the number of pages per file must be a whole number, 1 or more".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_count_names_the_file_and_counts_its_pages() {
        assert_eq!(
            page_count(Path::new("report.pdf"), 12),
            "report.pdf has 12 pages"
        );
        assert_eq!(
            page_count(Path::new("report.pdf"), 1),
            "report.pdf has 1 page"
        );
    }

    #[test]
    fn extracted_counts_the_pages_and_names_the_output() {
        assert_eq!(
            extracted(4, Path::new("out.pdf")),
            "Extracted 4 pages into out.pdf"
        );
        assert_eq!(
            extracted(1, Path::new("out.pdf")),
            "Extracted 1 page into out.pdf"
        );
    }

    #[test]
    fn split_names_the_first_and_the_last_file() {
        let outputs = ["parts/a-001.pdf", "parts/a-002.pdf", "parts/a-003.pdf"].map(PathBuf::from);

        assert_eq!(
            split(5, &outputs),
            "Split 5 pages into 3 files: parts/a-001.pdf to parts/a-003.pdf"
        );
        // Two files: the first and the last, with nothing in between.
        assert_eq!(
            split(2, &outputs[..2]),
            "Split 2 pages into 2 files: parts/a-001.pdf to parts/a-002.pdf"
        );
    }

    #[test]
    fn split_into_one_file_names_it_once() {
        let outputs = [PathBuf::from("parts/a-001.pdf")];

        assert_eq!(
            split(1, &outputs),
            "Split 1 page into 1 file: parts/a-001.pdf"
        );
    }

    #[test]
    fn split_into_no_file_still_reads_right() {
        // A document always has a page, so a split always writes a file;
        // the sentence does not rely on that.
        assert_eq!(split(0, &[]), "Split 0 pages into 0 files");
    }

    #[test]
    fn not_a_page_count_says_what_every_takes() {
        assert_eq!(
            not_a_page_count(),
            "the number of pages per file must be a whole number, 1 or more"
        );
    }

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
