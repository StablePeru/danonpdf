//! The command line: which commands, flags and arguments exist, and the
//! help that explains them.
//!
//! clap's `derive` reads the types below and writes the parser, the usage
//! errors and the `--help` text from them, so the three cannot disagree. A
//! doc comment (`///`) on a type or field is not only documentation here:
//! clap prints it as the help for that command or argument. Notes for
//! whoever reads the code are therefore written as plain `//` comments,
//! which clap does not see.
//!
//! What clap does by itself, before `main` gets anything back:
//!
//! - `--help`, `-h` and `help <command>` print the help on stdout and end
//!   the program with exit code 0. So does `--version`.
//! - A command line that breaks the rules below (no `--output`, an unknown
//!   flag, a `--pages` that cannot be read) prints a message on stderr and
//!   ends the program with exit code 2. This happens before the tool knows
//!   whether `--json` was given, so a usage error is always plain text,
//!   never JSON.
//!
//! clap also turns the text of an argument into the type of its field, and
//! a text that does not fit the type is a usage error like any other. That
//! is how `--pages` is checked: its field is a `PageSelection`, the core
//! knows how to read one from text, and what the core refuses clap reports.
//!
//! One rule is checked here and not by clap: "at least two inputs" (see
//! [`MergeArgs::check`]). It is reported through clap all the same, so it
//! looks and ends like every other usage error.

use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};
use danonpdf_core::PageSelection;

use crate::outcome::Operation;
use crate::texts;

/// A merge of fewer files than this is a mistake in the command line.
const MIN_INPUTS: usize = 2;

// The part of the help that a caller needs to use the tool without reading
// anything else: examples, the rules, the exit codes and the JSON shapes.
// It is shown under `danonpdf-cli --help` and under the `--help` of every
// command, so whichever one an agent tries first, it finds the whole
// contract.
//
// `r#"..."#` is a raw string: quotes and backslashes inside it are plain
// characters, so the JSON examples can be written as they are printed.
//
// The lines are kept under 80 columns by hand, except the JSON examples,
// which must stay on one line to be copied.
const CONTRACT: &str = r#"Examples:
  danonpdf-cli merge --json --output merged.pdf first.pdf second.pdf
  danonpdf-cli info --json report.pdf
  danonpdf-cli extract --json --pages 1-3,7,10- --output part.pdf report.pdf
  danonpdf-cli split --json --every 10 --output-dir parts report.pdf

Pages (the --pages of extract):
  A list of items separated by commas. Pages are counted from 1.
    N    page N
    A-B  pages A to B, both included; A must not be above B
    A-   page A and every page after it
  Example: 1-3,7,10- takes pages 1, 2, 3 and 7, and from 10 to the last.
  The pages are written in the order given: 3,1 puts page 3 first. Spaces
  around an item are ignored. No page may be listed twice (1-5,3 is
  refused).
  A list that cannot be read (empty, an empty item, not a number, page 0,
  a backwards range such as 5-3, a page listed twice) is a usage error:
  exit code 2. A page past the end of the document is found only once the
  file is open: exit code 1, error code page_out_of_range. Run info first
  to learn how many pages a file has.

Rules:
  - Every input is read as given: none is skipped for its name, and merge
    reads the same file twice when it is given twice.
  - Output paths are used exactly as given: no extension is added.
  - An output is never an input, not even with --force.
  - An existing output is replaced only with --force.
  - split writes <name of the input without its extension>-<NNN>.pdf into
    --output-dir, which it creates if it is missing: report-001.pdf,
    report-002.pdf... (four digits or more from a thousand files on).
    --every N puts N pages in each file; the last one may have fewer.
    Every name is checked before anything is written: if one is taken and
    --force was not given, nothing is written at all.
  - The tool never asks a question, never reads stdin and never uses the
    network.

Exit codes:
  0  the operation succeeded
  1  the operation was refused or failed
  2  the command line was wrong (usage error)
  If the report cannot be written (stdout or stderr is closed, or the disk
  is full), the line is dropped and the exit code still tells the result.

Output without --json:
  success  one line on stdout
  failure  one line on stderr, starting with "error: "; nothing on stdout

Output with --json:
  Exactly one JSON object on one line of stdout, and nothing on stderr, for
  a success and for a failure alike. "ok" and "operation" come first.
  success, one shape per command:
  {"ok":true,"operation":"merge","output":"merged.pdf","inputs":2,"pages":12}
  {"ok":true,"operation":"info","input":"report.pdf","pages":12}
  {"ok":true,"operation":"extract","output":"part.pdf","pages":7}
  {"ok":true,"operation":"split","outputs":["parts/report-001.pdf","parts/report-002.pdf"],"files":2,"pages":12}
  failure, one shape for every command:
  {"ok":false,"operation":"merge","error":{"code":"output_exists","message":"...","path":"merged.pdf"}}
  "input", "output" and "outputs" are paths as given, "outputs" in page
  order. "inputs" is the number of files read and "files" the number of
  files written. "pages" is the number of pages of the input (info, split)
  or of the output (merge, extract). "message" is for people; "path" is
  the file the error is about, and the key is left out when there is none.
  A split that stops after writing some of its files adds "written" to
  "error": the files already written, in order. They are complete and are
  kept, and "path" is the file that failed. No other failure has the key.
  A path that is not valid Unicode is written with replacement characters.
  A usage error (exit code 2) is never JSON: the argument parser prints
  plain text on stderr, also when --json was given.

Error codes (stable: test for these, not for the message):
  output_is_input    an output is one of the inputs
  output_exists      an output already exists and --force was not given
  no_inputs          there was nothing to merge; never reported by this
                     command line, which refuses fewer than two inputs first
                     as a usage error (exit code 2, plain text on stderr)
  read_failed        an input could not be read, or is not a PDF
  encrypted          an input needs a password
  broken_pdf         an input opened, but its structure is damaged
  page_out_of_range  --pages names a page past the end of the document
  save_failed        an output file, or the folder for it, could not be
                     written"#;

// Shown under the short help (`-h`), which leaves the contract out.
const HELP_POINTER: &str =
    "Run with --help for examples, the page syntax, the exit codes and the JSON output.";

/// Merge, split and inspect PDF files: local, no network, no questions
///
/// DanonPDF for scripts and AI agents. It does one operation per run, never
/// waits for input, and reports the result as a line of text or, with
/// --json, as one JSON object.
// `version` makes clap answer `--version` with the name and version from
// Cargo.toml. `term_width = 0` turns off clap's own line wrapping, so the
// help is printed exactly as it is laid out in this file.
#[derive(Debug, Parser)]
#[command(
    version,
    term_width = 0,
    verbatim_doc_comment,
    after_help = HELP_POINTER,
    after_long_help = CONTRACT
)]
pub struct Cli {
    // Not an `Option`: clap then requires a command, and running the tool
    // with no arguments at all is a usage error (exit code 2).
    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    /// The command line this program was started with, parsed and checked.
    ///
    /// It only comes back with a valid command line. For `--help`,
    /// `--version` or a mistake, `exit` prints clap's own text (on stdout
    /// for the first two, on stderr for a mistake) and ends the program
    /// with exit code 0, 0 or 2.
    pub fn from_process_args() -> Self {
        // `args_os` and not `args`: a file name does not have to be valid
        // UTF-8, and `args` would panic on one that is not.
        Self::from_args(std::env::args_os()).unwrap_or_else(|error| error.exit())
    }

    /// Parses and checks `args`, the program name first.
    ///
    /// Returns the error instead of printing it and ending the program, so
    /// tests can look at it. The two generic types are clap's own bounds:
    /// anything that can be walked through and turned into `OsString`s.
    pub fn from_args<I, T>(args: I) -> Result<Self, clap::Error>
    where
        I: IntoIterator<Item = T>,
        T: Into<OsString> + Clone,
    {
        let cli = Self::try_parse_from(args)?;
        // One arm per command, each with the checks that clap cannot do.
        // Only a merge has one; the other three are named all the same, so
        // that a new command has to be placed here on purpose.
        match &cli.command {
            Command::Merge(merge) => merge.check()?,
            Command::Info(_) | Command::Extract(_) | Command::Split(_) => {}
        }
        Ok(cli)
    }
}

// An enum with one variant per command. `main` matches on it, so adding a
// command here makes the compiler point at the place that must handle it.
//
// `name = ...` gives each command the name of its operation. clap would
// otherwise make the name from the variant ("Merge" becomes "merge"), which
// gives the same words today; taking them from `Operation` means the
// command typed and the "operation" in the JSON report come from one place.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Merge PDF files, in the order given, into one new file
    #[command(
        name = Operation::Merge.name(),
        after_help = HELP_POINTER,
        after_long_help = CONTRACT
    )]
    Merge(MergeArgs),

    /// Say how many pages a PDF file has
    #[command(
        name = Operation::Info.name(),
        after_help = HELP_POINTER,
        after_long_help = CONTRACT
    )]
    Info(InfoArgs),

    /// Copy chosen pages of a PDF file, in the order given, into a new file
    #[command(
        name = Operation::Extract.name(),
        after_help = HELP_POINTER,
        after_long_help = CONTRACT
    )]
    Extract(ExtractArgs),

    /// Split a PDF file into files of N pages each (one page by default)
    #[command(
        name = Operation::Split.name(),
        after_help = HELP_POINTER,
        after_long_help = CONTRACT
    )]
    Split(SplitArgs),
}

impl Command {
    /// The operation this command runs.
    pub fn operation(&self) -> Operation {
        // `_` inside a variant ignores what it holds: only which variant
        // it is matters here.
        match self {
            Self::Merge(_) => Operation::Merge,
            Self::Info(_) => Operation::Info,
            Self::Extract(_) => Operation::Extract,
            Self::Split(_) => Operation::Split,
        }
    }
}

#[derive(Debug, Args)]
pub struct MergeArgs {
    /// Where to write the merged PDF; the path is used exactly as given
    // `PathBuf` and not `String`: clap then accepts a path that is not
    // valid UTF-8 instead of refusing it.
    #[arg(long, value_name = "FILE")]
    pub output: PathBuf,

    /// Replace the output if it already exists (it may never be an input)
    // A `bool` field becomes a switch: absent is `false`, present is `true`.
    #[arg(long)]
    pub force: bool,

    /// Print the result as one JSON object on stdout, also when it fails
    #[arg(long)]
    pub json: bool,

    /// The PDF files to merge, in this order (at least two)
    // `required` makes clap refuse a command line with no input at all. It
    // cannot be asked for "two or more": `num_args = 2..` counts each run
    // of inputs on its own, so `a.pdf --output out.pdf b.pdf` (two runs of
    // one) would be refused although it names two files. `check` below
    // counts them all together instead.
    #[arg(value_name = "INPUT", required = true)]
    pub inputs: Vec<PathBuf>,
}

impl MergeArgs {
    /// The rule clap cannot express: at least two inputs in total.
    fn check(&self) -> Result<(), clap::Error> {
        if self.inputs.len() >= MIN_INPUTS {
            return Ok(());
        }
        let message = texts::too_few_inputs(MIN_INPUTS, self.inputs.len());

        // The error is built by clap, from the `merge` command, so it gets
        // the same "error:" prefix, usage line, colours and exit code as
        // the mistakes clap finds itself. `build` fills in what clap only
        // works out when it parses, such as the full name "danonpdf-cli
        // merge" that the usage line shows.
        let mut command = Cli::command();
        command.build();
        Err(match command.find_subcommand_mut(Operation::Merge.name()) {
            Some(merge) => merge.error(ErrorKind::TooFewValues, message),
            // Not reached: the command is named after the same
            // operation above. If it ever is, the error is still
            // reported, with a plainer usage line.
            None => command.error(ErrorKind::TooFewValues, message),
        })
    }
}

#[derive(Debug, Args)]
pub struct InfoArgs {
    /// Print the result as one JSON object on stdout, also when it fails
    #[arg(long)]
    pub json: bool,

    /// The PDF file to look at
    // One `PathBuf` and not a `Vec`: clap then takes exactly one input and
    // refuses a second one as a usage error.
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
}

#[derive(Debug, Args)]
pub struct ExtractArgs {
    /// The pages to take, in this order: for example 1-3,7,10-
    // The field is a `PageSelection`, not a `String`. clap sees that the
    // type can be read from text (it implements `FromStr`, in the core) and
    // does so while it parses. A text the core refuses becomes a usage
    // error with the core's own reason in it, and this struct can only
    // ever hold a selection that is well formed.
    #[arg(long, value_name = "PAGES")]
    pub pages: PageSelection,

    /// Where to write the new PDF; the path is used exactly as given
    #[arg(long, value_name = "FILE")]
    pub output: PathBuf,

    /// Replace the output if it already exists (it may never be the input)
    #[arg(long)]
    pub force: bool,

    /// Print the result as one JSON object on stdout, also when it fails
    #[arg(long)]
    pub json: bool,

    /// The PDF file to take the pages from
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
}

#[derive(Debug, Args)]
pub struct SplitArgs {
    /// How many pages go into each file; the last file may have fewer
    // `NonZeroUsize` is a whole number that cannot be 0, so "a file every 0
    // pages" never gets past the parser. `default_value_t` is the value
    // when the flag is absent: `MIN` is the smallest one there is, 1, which
    // means one file per page. `value_parser` names the function that
    // reads the text, for the sake of its error message.
    #[arg(
        long,
        value_name = "N",
        default_value_t = NonZeroUsize::MIN,
        value_parser = pages_per_file
    )]
    pub every: NonZeroUsize,

    /// The folder for the new files; it is created if it does not exist
    // clap names the flag after the field, with a dash for the underscore:
    // `--output-dir`.
    #[arg(long, value_name = "DIR")]
    pub output_dir: PathBuf,

    /// Replace files that already exist (none may ever be the input)
    #[arg(long)]
    pub force: bool,

    /// Print the result as one JSON object on stdout, also when it fails
    #[arg(long)]
    pub json: bool,

    /// The PDF file to split
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
}

/// Reads the value of `--every`.
///
/// `text.parse()` alone would do: it already refuses 0, a fraction and
/// anything that is not a number. But its own reasons are written for
/// programmers ("number would be zero for non-zero type"). `map_err` swaps
/// whatever the reason was for one sentence that says what is wanted.
fn pages_per_file(text: &str) -> Result<NonZeroUsize, String> {
    text.parse().map_err(|_| texts::not_a_page_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `args` as if they followed the program name on a command line.
    fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
        Cli::from_args(std::iter::once(&"danonpdf-cli").chain(args))
    }

    /// The arguments of the `merge` command in `args`.
    fn merge_args(args: &[&str]) -> MergeArgs {
        // `let ... else`: a pattern that may not match, with what to do
        // when it does not. It reads better than a `match` when only one
        // variant is of interest.
        let Command::Merge(merge) = parse(args).unwrap().command else {
            panic!("{args:?} is not a merge");
        };
        merge
    }

    /// The arguments of the `extract` command in `args`.
    fn extract_args(args: &[&str]) -> ExtractArgs {
        let Command::Extract(extract) = parse(args).unwrap().command else {
            panic!("{args:?} is not an extract");
        };
        extract
    }

    /// The arguments of the `split` command in `args`.
    fn split_args(args: &[&str]) -> SplitArgs {
        let Command::Split(split) = parse(args).unwrap().command else {
            panic!("{args:?} is not a split");
        };
        split
    }

    #[test]
    fn the_declarations_are_consistent() {
        // clap checks many mistakes (two flags with one name, a required
        // argument after an optional one...) only when the parser is built.
        // This builds it once, so they fail here and not in front of a user.
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_is_named_after_its_operation() {
        // A smallest valid command line for each command, starting with the
        // name of its operation. If the name typed and the operation ever
        // came apart, the line would not parse, or would run another one.
        let lines: [(Operation, &[&str]); 4] = [
            (Operation::Merge, &["--output", "o.pdf", "a.pdf", "b.pdf"]),
            (Operation::Info, &["a.pdf"]),
            (
                Operation::Extract,
                &["--pages", "1", "--output", "o.pdf", "a.pdf"],
            ),
            (Operation::Split, &["--output-dir", "parts", "a.pdf"]),
        ];
        for (operation, rest) in lines {
            let args: Vec<&str> = std::iter::once(operation.name())
                .chain(rest.iter().copied())
                .collect();

            let cli = parse(&args).unwrap();

            assert_eq!(cli.command.operation(), operation, "{args:?}");
        }
    }

    // merge

    #[test]
    fn merge_takes_the_output_and_the_inputs_in_order() {
        let merge = merge_args(&["merge", "--output", "out.pdf", "b.pdf", "a.pdf", "b.pdf"]);

        assert_eq!(merge.output, PathBuf::from("out.pdf"));
        // As given: not sorted, and the repeated file is kept.
        assert_eq!(merge.inputs, ["b.pdf", "a.pdf", "b.pdf"].map(PathBuf::from));
        assert!(!merge.force, "--force must be off unless given");
        assert!(!merge.json, "--json must be off unless given");
    }

    #[test]
    fn force_and_json_are_plain_switches() {
        let merge = merge_args(&[
            "merge", "a.pdf", "b.pdf", "--json", "--force", "--output", "out.pdf",
        ]);

        assert!(merge.force);
        assert!(merge.json);
    }

    #[test]
    fn inputs_may_stand_on_both_sides_of_a_flag() {
        // Two inputs, with a flag in between. clap sees two runs of one
        // value each, so a "two or more" rule inside clap would refuse it.
        let merge = merge_args(&["merge", "a.pdf", "--output", "out.pdf", "b.pdf"]);

        assert_eq!(merge.inputs, ["a.pdf", "b.pdf"].map(PathBuf::from));
    }

    #[test]
    fn fewer_than_two_inputs_is_a_usage_error() {
        let one = parse(&["merge", "--output", "out.pdf", "a.pdf"]).unwrap_err();
        assert_eq!(one.kind(), ErrorKind::TooFewValues);
        assert_eq!(one.exit_code(), 2);
        // Written like clap's own errors: the reason, then how to call the
        // `merge` command.
        let text = one.to_string();
        assert!(
            text.starts_with("error: merge needs at least 2 input files; only 1 was given"),
            "unexpected: {text}"
        );
        assert!(
            text.contains("Usage: danonpdf-cli merge"),
            "unexpected: {text}"
        );

        let none = parse(&["merge", "--output", "out.pdf"]).unwrap_err();
        assert_eq!(none.kind(), ErrorKind::MissingRequiredArgument);
        assert_eq!(none.exit_code(), 2);
    }

    #[test]
    fn a_missing_output_is_a_usage_error() {
        let error = parse(&["merge", "a.pdf", "b.pdf"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::MissingRequiredArgument);
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn json_does_not_change_how_a_usage_error_is_reported() {
        // The parser fails before the tool knows that JSON was wanted, so
        // the same kind of error, with the same exit code, comes back.
        let error = parse(&["merge", "--json", "--output", "out.pdf", "a.pdf"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::TooFewValues);
        assert_eq!(error.exit_code(), 2);
    }

    // info

    #[test]
    fn info_takes_exactly_one_input() {
        let Command::Info(info) = parse(&["info", "a.pdf"]).unwrap().command else {
            panic!("not an info");
        };
        assert_eq!(info.input, PathBuf::from("a.pdf"));
        assert!(!info.json, "--json must be off unless given");

        let none = parse(&["info"]).unwrap_err();
        assert_eq!(none.kind(), ErrorKind::MissingRequiredArgument);
        let two = parse(&["info", "a.pdf", "b.pdf"]).unwrap_err();
        assert_eq!(two.kind(), ErrorKind::UnknownArgument);
        assert_eq!(two.exit_code(), 2);
    }

    // extract

    #[test]
    fn extract_reads_the_pages_into_a_selection() {
        let extract = extract_args(&[
            "extract", "--pages", "3,1-2", "--output", "out.pdf", "in.pdf",
        ]);

        // Already a selection, not text: resolved against a 5-page
        // document, it gives the pages in the order written.
        assert_eq!(extract.pages.resolve(5).unwrap(), [3, 1, 2]);
        assert_eq!(extract.output, PathBuf::from("out.pdf"));
        assert_eq!(extract.input, PathBuf::from("in.pdf"));
        assert!(!extract.force, "--force must be off unless given");
        assert!(!extract.json, "--json must be off unless given");
    }

    #[test]
    fn pages_that_cannot_be_read_are_a_usage_error() {
        // One text per rule of the syntax, each with a few words of the
        // core's reason: the same sentence reaches the user through clap.
        let cases = [
            ("", "no pages were given"),
            ("1,,3", "item 2 of the page list is empty"),
            ("abc", "is not a page"),
            ("0", "there is no page 0"),
            ("5-3", "runs backwards"),
            ("1-5,3", "page 3 is listed more than once"),
        ];
        for (pages, reason) in cases {
            let error = parse(&[
                "extract", "--json", "--pages", pages, "--output", "out.pdf", "in.pdf",
            ])
            .unwrap_err();

            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{pages:?}");
            assert_eq!(error.exit_code(), 2, "{pages:?}");
            let text = error.to_string();
            assert!(text.contains("--pages <PAGES>"), "{pages:?} gave: {text}");
            assert!(text.contains(reason), "{pages:?} gave: {text}");
        }
    }

    #[test]
    fn a_page_past_the_end_is_not_a_usage_error() {
        // Nothing in the text is wrong, and the parser has no document to
        // count: page 9999 is refused later, by the core, with exit code 1.
        let extract = extract_args(&[
            "extract", "--pages", "9999", "--output", "out.pdf", "in.pdf",
        ]);

        assert!(extract.pages.resolve(5).is_err());
    }

    #[test]
    fn extract_needs_pages_an_output_and_one_input() {
        let lines: [&[&str]; 4] = [
            &["extract", "--output", "out.pdf", "in.pdf"],
            &["extract", "--pages", "1", "in.pdf"],
            &["extract", "--pages", "1", "--output", "out.pdf"],
            &[
                "extract", "--pages", "1", "--output", "o.pdf", "a.pdf", "b.pdf",
            ],
        ];
        for line in lines {
            let error = parse(line).unwrap_err();

            assert_eq!(error.exit_code(), 2, "{line:?}");
        }
    }

    // split

    #[test]
    fn split_puts_one_page_in_each_file_unless_told_otherwise() {
        let split = split_args(&["split", "--output-dir", "parts", "in.pdf"]);

        assert_eq!(split.every.get(), 1);
        assert_eq!(split.output_dir, PathBuf::from("parts"));
        assert_eq!(split.input, PathBuf::from("in.pdf"));
        assert!(!split.force, "--force must be off unless given");
        assert!(!split.json, "--json must be off unless given");

        let split = split_args(&["split", "--every", "10", "--output-dir", "parts", "in.pdf"]);
        assert_eq!(split.every.get(), 10);
    }

    #[test]
    fn every_must_be_a_whole_number_from_1_up() {
        for every in ["0", "abc", "1.5", "", "+"] {
            let error =
                parse(&["split", "--every", every, "--output-dir", "parts", "in.pdf"]).unwrap_err();

            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{every:?}");
            assert_eq!(error.exit_code(), 2, "{every:?}");
            let text = error.to_string();
            assert!(
                text.contains("must be a whole number, 1 or more"),
                "{every:?} gave: {text}"
            );
        }
        // A negative number looks like a flag to the parser, which refuses
        // it one step earlier. It is a usage error all the same.
        let negative =
            parse(&["split", "--every", "-1", "--output-dir", "parts", "in.pdf"]).unwrap_err();
        assert_eq!(negative.exit_code(), 2);
    }

    #[test]
    fn split_needs_an_output_folder_and_one_input() {
        let lines: [&[&str]; 3] = [
            &["split", "in.pdf"],
            &["split", "--output-dir", "parts"],
            &["split", "--output-dir", "parts", "a.pdf", "b.pdf"],
        ];
        for line in lines {
            let error = parse(line).unwrap_err();

            assert_eq!(error.exit_code(), 2, "{line:?}");
        }
    }

    // Help and version

    #[test]
    fn help_and_version_end_with_success() {
        // clap reports "print the help" through the same error type, but
        // with exit code 0: asking for help is not a mistake.
        let help = parse(&["--help"]).unwrap_err();
        assert_eq!(help.kind(), ErrorKind::DisplayHelp);
        assert_eq!(help.exit_code(), 0);

        let version = parse(&["--version"]).unwrap_err();
        assert_eq!(version.kind(), ErrorKind::DisplayVersion);
        assert_eq!(version.exit_code(), 0);
    }
}
