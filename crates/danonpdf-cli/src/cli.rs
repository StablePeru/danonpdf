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
//!   flag) prints a message on stderr and ends the program with exit code
//!   2. This happens before the tool knows whether `--json` was given, so a
//!   usage error is always plain text, never JSON.
//!
//! One rule is checked here and not by clap: "at least two inputs" (see
//! [`MergeArgs::check`]). It is reported through clap all the same, so it
//! looks and ends like every other usage error.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::texts;

/// A merge of fewer files than this is a mistake in the command line.
const MIN_INPUTS: usize = 2;

// The part of the help that a caller needs to use the tool without reading
// anything else: an example, the rules, the exit codes and the JSON shapes.
// It is shown under both `danonpdf-cli --help` and `danonpdf-cli merge
// --help`, so whichever one an agent tries first, it finds the whole
// contract.
//
// `r#"..."#` is a raw string: quotes and backslashes inside it are plain
// characters, so the JSON examples can be written as they are printed.
//
// The lines are kept under 80 columns by hand, except the JSON examples,
// which must stay on one line to be copied.
const CONTRACT: &str = r#"Example:
  danonpdf-cli merge --json --output merged.pdf first.pdf second.pdf

Rules:
  - Every input is read, in the order given. None is skipped for its name,
    and the same file given twice is merged twice.
  - The output path is used exactly as given: no extension is added.
  - The output is never one of the inputs, not even with --force.
  - An existing output is replaced only with --force.
  - The tool never asks a question, never reads stdin and never uses the
    network.

Exit codes:
  0  the operation succeeded
  1  the operation was refused or failed
  2  the command line was wrong (usage error)

Output without --json:
  success  one line on stdout
  failure  one line on stderr, starting with "error: "; nothing on stdout

Output with --json:
  Exactly one JSON object on one line of stdout, and nothing on stderr, for
  a success and for a failure alike.
  success:
  {"ok":true,"operation":"merge","output":"merged.pdf","inputs":2,"pages":12}
  failure:
  {"ok":false,"operation":"merge","error":{"code":"output_exists","message":"...","path":"merged.pdf"}}
  "output" is the path as given, "inputs" the number of files read and
  "pages" the number of pages written. "message" is for people; "path" is
  the file the error is about, and the key is left out when there is none.
  A path that is not valid Unicode is written with replacement characters.
  A usage error (exit code 2) is never JSON: the argument parser prints
  plain text on stderr, also when --json was given.

Error codes (stable: test for these, not for the message):
  output_is_input  the output is one of the inputs
  output_exists    the output already exists and --force was not given
  no_inputs        there was nothing to merge
  read_failed      an input could not be read, or is not a PDF
  encrypted        an input needs a password
  broken_pdf       an input opened, but its structure is damaged
  save_failed      the merged file could not be written"#;

// Shown under the short help (`-h`), which leaves the contract out.
const HELP_POINTER: &str = "Run with --help for an example, the exit codes and the JSON output.";

/// Merge PDF files from the command line: local, no network, no questions
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
        match &cli.command {
            Command::Merge(merge) => merge.check()?,
        }
        Ok(cli)
    }
}

// An enum with one variant per command. `main` matches on it, so adding a
// command here makes the compiler point at the place that must handle it.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Merge PDF files, in the order given, into one new file
    #[command(after_help = HELP_POINTER, after_long_help = CONTRACT)]
    Merge(MergeArgs),
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
        Err(match command.find_subcommand_mut("merge") {
            Some(merge) => merge.error(ErrorKind::TooFewValues, message),
            // Only if the command is renamed without updating the line
            // above; the error is then still reported, with a plainer usage.
            None => command.error(ErrorKind::TooFewValues, message),
        })
    }
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
        // A `match` with one arm today. A second command will make this
        // stop compiling, which is the reminder to handle it.
        match parse(args).unwrap().command {
            Command::Merge(merge) => merge,
        }
    }

    #[test]
    fn the_declarations_are_consistent() {
        // clap checks many mistakes (two flags with one name, a required
        // argument after an optional one...) only when the parser is built.
        // This builds it once, so they fail here and not in front of a user.
        Cli::command().debug_assert();
    }

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
