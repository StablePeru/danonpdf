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
//! Two rules are checked here and not by clap: "at least two inputs" (see
//! [`MergeArgs::check`]) and "the image is written to a `.png` file" (see
//! [`RenderArgs::check`]). They are reported through clap all the same, so
//! they look and end like every other usage error.

use std::ffi::OsString;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};

use clap::error::ErrorKind;
use clap::{Args, CommandFactory, Parser, Subcommand};
use danonpdf_core::{MAX_RENDER_WIDTH, PageSelection};

use crate::outcome::Operation;
use crate::texts;

/// A merge of fewer files than this is a mistake in the command line.
const MIN_INPUTS: usize = 2;

/// How wide `render` draws a page, in pixels, when `--width` is not given.
///
/// 1024 pixels is about 124 dots per inch for an A4 page (1024 by 1449
/// pixels): body text is easy to read, for a person and for a model that
/// looks at the image, and the PNG of a page of text stays well under a
/// megabyte. It is a quarter of the widest image the core draws
/// (`MAX_RENDER_WIDTH`), so a caller that needs more detail has room to
/// ask for it.
const DEFAULT_RENDER_WIDTH: u32 = 1024;

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
//
// A constant text cannot take a number from another constant, so the limits
// of an image (its widest and tallest size, and the default width) are
// typed here by hand. The test `the_help_states_the_limits_of_an_image`
// compares them with the constants they copy.
const CONTRACT: &str = r#"Examples:
  danonpdf-cli merge --json --output merged.pdf first.pdf second.pdf
  danonpdf-cli info --json report.pdf
  danonpdf-cli extract --json --pages 1-3,7,10- --output part.pdf report.pdf
  danonpdf-cli split --json --every 10 --output-dir parts report.pdf
  danonpdf-cli render --json --page 1 --output page.png report.pdf

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

Images (render):
  render draws one page, --page N counted from 1, and writes it as a PNG
  file: 8 bits per channel with alpha, on white paper that is fully opaque.
  --width is the width of the image in pixels, from 1 to 4096. Without it
  the image is 1024 pixels wide. The height follows from the shape of the
  page: an A4 page drawn 1024 pixels wide is 1449 pixels high.
  A very tall page is drawn narrower than asked, so that its image is
  at most 8192 pixels high. The report gives the size of the image that
  was written: read "width" and "height" there, not from the command
  line.
  --output must end in .png, in any letter case: the tool adds no
  extension, and what it writes is a PNG whatever the name says.
  --page 0, a --width out of range and an --output that does not end in
  .png are usage errors: exit code 2. A page past the end of the document
  is found only once the file is open: exit code 1, error code
  page_out_of_range.
  render reads the file with PDFium, the other commands with another
  library, and the two do not always agree about a damaged file: one that
  info counts may fail to render, and the other way round.

The PDFium library (render only):
  render loads PDFium from a file when it runs: pdfium.dll on Windows,
  libpdfium.so on Linux. It looks for that file in the folder of the
  program itself, or in the folder named by the environment variable
  DANONPDF_PDFIUM_DIR when it is set and not empty. Nowhere else: not in
  the current folder and not along PATH. If the file is not there or
  cannot be loaded: exit code 1, error code renderer_unavailable.
  merge, info, extract and split do not need it.

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
  {"ok":true,"operation":"render","output":"page.png","page":1,"width":1024,"height":1449}
  failure, one shape for every command:
  {"ok":false,"operation":"merge","error":{"code":"output_exists","message":"...","path":"merged.pdf"}}
  "input", "output" and "outputs" are paths as given, "outputs" in page
  order. "inputs" is the number of files read and "files" the number of
  files written. "pages" is the number of pages of the input (info, split)
  or of the output (merge, extract). "page" is the page that was drawn,
  and "width" and "height" the size in pixels of the image that was
  written (render). "message" is for people; "path" is the file the error
  is about, and the key is left out when there is none.
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
  encrypted          an input needs a password; for render, also an input
                     protected in a way that cannot be opened at all
  broken_pdf         an input opened, but its structure is damaged
  page_out_of_range  --pages or --page names a page past the end of the
                     document
  save_failed        an output file, or the folder for it, could not be
                     written
  renderer_unavailable
                     the PDFium library is missing or cannot be loaded;
                     "path" is the library file that was looked for, as an
                     absolute path, and is left out when the program cannot
                     tell which folder to look in (set DANONPDF_PDFIUM_DIR
                     then). Only render reports it, and nothing is wrong
                     with the input
  render_failed      the page was not drawn and no other code says why:
                     PDFium gave up on the file without a reason. Only
                     render reports it"#;

// Shown under the short help (`-h`), which leaves the contract out.
const HELP_POINTER: &str =
    "Run with --help for examples, the page syntax, the exit codes and the JSON output.";

/// Merge, split, inspect and draw PDF files: local, no network, no questions
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
        // A merge and a render have one each; the other three are named
        // all the same, so that a new command has to be placed here on
        // purpose.
        match &cli.command {
            Command::Merge(merge) => merge.check()?,
            Command::Render(render) => render.check()?,
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

    /// Draw one page of a PDF file and write it as a PNG image
    #[command(
        name = Operation::Render.name(),
        after_help = HELP_POINTER,
        after_long_help = CONTRACT
    )]
    Render(RenderArgs),
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
            Self::Render(_) => Operation::Render,
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
        Err(usage_error(
            Operation::Merge,
            ErrorKind::TooFewValues,
            message,
        ))
    }
}

/// A usage error about the command of `operation`, for a rule that is
/// checked in this file and not by clap.
///
/// The error is built by clap, from that command, so it gets the same
/// "error:" prefix, usage line, colours and exit code as the mistakes clap
/// finds itself. `kind` is clap's own name for the sort of mistake.
fn usage_error(operation: Operation, kind: ErrorKind, message: String) -> clap::Error {
    let mut command = Cli::command();
    // `build` fills in what clap only works out when it parses, such as the
    // full name "danonpdf-cli merge" that the usage line shows.
    command.build();
    match command.find_subcommand_mut(operation.name()) {
        Some(subcommand) => subcommand.error(kind, message),
        // Not reached: every command is named after its operation (see
        // `Command`). If it ever is, the error is still reported, with a
        // plainer usage line.
        None => command.error(kind, message),
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

#[derive(Debug, Args)]
pub struct RenderArgs {
    /// The page to draw, counted from 1
    // `NonZeroUsize`, as for `--every`: "page 0" never gets past the
    // parser, and the core's `render_page` takes this very type, so there
    // is no number to check again on the way.
    #[arg(long, value_name = "N", value_parser = page_number)]
    pub page: NonZeroUsize,

    // No doc comment on this field: its help is the `help = ...` below. A
    // doc comment is fixed text, and this sentence names the widest image
    // the core draws. Built with `format!` from the core's constant, the
    // help cannot fall behind it. clap adds "[default: 1024]" by itself,
    // from `default_value_t`.
    #[arg(
        long,
        value_name = "PX",
        default_value_t = DEFAULT_RENDER_WIDTH,
        value_parser = image_width,
        help = format!(
            "The width of the image in pixels, from 1 to {MAX_RENDER_WIDTH}; the height follows the page"
        )
    )]
    pub width: u32,

    /// Where to write the PNG image; the name must end in .png
    #[arg(long, value_name = "FILE")]
    pub output: PathBuf,

    /// Replace the output if it already exists (it may never be the input)
    #[arg(long)]
    pub force: bool,

    /// Print the result as one JSON object on stdout, also when it fails
    #[arg(long)]
    pub json: bool,

    /// The PDF file to draw a page of
    #[arg(value_name = "INPUT")]
    pub input: PathBuf,
}

impl RenderArgs {
    /// The rule clap cannot express: the output is named as a PNG file.
    ///
    /// The tool never adds an extension to an output, and what a render
    /// writes is a PNG whatever its name says. A `page.jpg` that holds a
    /// PNG would mislead whoever opens it next, so the name is refused
    /// before anything is read or written.
    ///
    /// Why not a `value_parser` on the field, like the two numbers have:
    /// such a function is handed the argument as text (`&str`), so clap
    /// would first refuse a path that is not valid UTF-8. Checked here,
    /// the path stays the `PathBuf` it was parsed into.
    fn check(&self) -> Result<(), clap::Error> {
        if is_png_path(&self.output) {
            return Ok(());
        }
        Err(usage_error(
            Operation::Render,
            ErrorKind::ValueValidation,
            texts::output_is_not_png(&self.output),
        ))
    }
}

/// Whether `path` names a PNG file by its extension, in any letter case
/// (`page.png`, `PAGE.PNG`). Only the name is looked at, never the disk.
fn is_png_path(path: &Path) -> bool {
    // Compared as `OsStr`, without turning the name into text first: a
    // file name is not always valid UTF-8.
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
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

/// Reads the value of `--page`, with a reason of its own for the same
/// purpose as [`pages_per_file`].
fn page_number(text: &str) -> Result<NonZeroUsize, String> {
    text.parse().map_err(|_| texts::not_a_page_number())
}

/// Reads the value of `--width`: a whole number of pixels from 1 to the
/// widest image the core draws.
///
/// The core checks the same range again when it is asked to draw. It is
/// checked here first so that a width out of range is a usage error (exit
/// code 2, before any file is opened), like every other value that cannot
/// be right whatever the file holds. The limit is the core's constant, not
/// a number of this file, so the two checks cannot disagree.
fn image_width(text: &str) -> Result<u32, String> {
    // A `match` with a guard: the first arm takes a number that parsed
    // *and* is in range. Everything else (not a number, a fraction, 0, too
    // wide) falls to the second arm and gets the same sentence.
    match text.parse::<u32>() {
        Ok(width) if (1..=MAX_RENDER_WIDTH).contains(&width) => Ok(width),
        _ => Err(texts::not_an_image_width(MAX_RENDER_WIDTH)),
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

    /// The arguments of the `render` command in `args`.
    fn render_args(args: &[&str]) -> RenderArgs {
        let Command::Render(render) = parse(args).unwrap().command else {
            panic!("{args:?} is not a render");
        };
        render
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
        let lines: [(Operation, &[&str]); 5] = [
            (Operation::Merge, &["--output", "o.pdf", "a.pdf", "b.pdf"]),
            (Operation::Info, &["a.pdf"]),
            (
                Operation::Extract,
                &["--pages", "1", "--output", "o.pdf", "a.pdf"],
            ),
            (Operation::Split, &["--output-dir", "parts", "a.pdf"]),
            (
                Operation::Render,
                &["--page", "1", "--output", "o.png", "a.pdf"],
            ),
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

    // render

    #[test]
    fn render_takes_a_page_an_output_and_one_input() {
        let render = render_args(&["render", "--page", "3", "--output", "page.png", "in.pdf"]);

        assert_eq!(render.page.get(), 3);
        assert_eq!(render.width, DEFAULT_RENDER_WIDTH);
        assert_eq!(render.output, PathBuf::from("page.png"));
        assert_eq!(render.input, PathBuf::from("in.pdf"));
        assert!(!render.force, "--force must be off unless given");
        assert!(!render.json, "--json must be off unless given");

        let render = render_args(&[
            "render", "--width", "200", "--page", "1", "--output", "p.png", "in.pdf",
        ]);
        assert_eq!(render.width, 200);
    }

    #[test]
    fn the_page_must_be_a_whole_number_from_1_up() {
        for page in ["0", "abc", "1.5", "", "1-3"] {
            let error =
                parse(&["render", "--page", page, "--output", "p.png", "in.pdf"]).unwrap_err();

            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{page:?}");
            assert_eq!(error.exit_code(), 2, "{page:?}");
            let text = error.to_string();
            assert!(text.contains("--page <N>"), "{page:?} gave: {text}");
            assert!(
                text.contains("pages are counted from 1"),
                "{page:?} gave: {text}"
            );
        }
    }

    #[test]
    fn the_width_must_be_one_that_the_core_draws() {
        let widest = MAX_RENDER_WIDTH.to_string();
        let too_wide = (MAX_RENDER_WIDTH + 1).to_string();
        let line = |width| {
            parse(&[
                "render", "--page", "1", "--width", width, "--output", "p.png", "in.pdf",
            ])
        };

        // Both ends of the range are in it.
        for width in ["1", widest.as_str()] {
            assert!(line(width).is_ok(), "{width:?} should be accepted");
        }
        for width in ["0", too_wide.as_str(), "abc", "1.5", "", "4294967296"] {
            let error = line(width).unwrap_err();

            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{width:?}");
            assert_eq!(error.exit_code(), 2, "{width:?}");
            let text = error.to_string();
            assert!(text.contains("--width <PX>"), "{width:?} gave: {text}");
            assert!(
                text.contains(&format!("from 1 to {widest}")),
                "{width:?} gave: {text}"
            );
        }
    }

    #[test]
    fn the_output_of_a_render_must_end_in_png() {
        let line = |output| parse(&["render", "--page", "1", "--output", output, "in.pdf"]);

        for output in ["page.png", "PAGE.PNG", "out/Page.Png", "page.pdf.png"] {
            assert!(line(output).is_ok(), "{output:?} should be accepted");
        }
        // `.png` alone is a hidden file with no extension, as `.pdf` is for
        // the core's `is_pdf_path`.
        for output in ["page.jpg", "page", "page.png.bak", "page.", ".png"] {
            let error = line(output).unwrap_err();

            assert_eq!(error.kind(), ErrorKind::ValueValidation, "{output:?}");
            assert_eq!(error.exit_code(), 2, "{output:?}");
            // Written like clap's own errors: the reason, then how to call
            // the `render` command.
            let text = error.to_string();
            assert!(
                text.starts_with("error: render writes a PNG image"),
                "{output:?} gave: {text}"
            );
            assert!(text.contains(output), "{output:?} gave: {text}");
            assert!(
                text.contains("Usage: danonpdf-cli render"),
                "{output:?} gave: {text}"
            );
        }
    }

    #[test]
    fn render_needs_a_page_an_output_and_one_input() {
        let lines: [&[&str]; 4] = [
            &["render", "--output", "p.png", "in.pdf"],
            &["render", "--page", "1", "in.pdf"],
            &["render", "--page", "1", "--output", "p.png"],
            &[
                "render", "--page", "1", "--output", "p.png", "a.pdf", "b.pdf",
            ],
        ];
        for line in lines {
            let error = parse(line).unwrap_err();

            assert_eq!(error.exit_code(), 2, "{line:?}");
        }
    }

    #[test]
    fn the_help_states_the_limits_of_an_image() {
        use danonpdf_core::{MAX_RENDER_HEIGHT, PDFIUM_DIR_VARIABLE};

        // The contract is a constant text, so these numbers and this name
        // are typed in it by hand. Each must still be what the code uses.
        for expected in [
            format!("from 1 to {MAX_RENDER_WIDTH}. Without it"),
            format!("the image is {DEFAULT_RENDER_WIDTH} pixels wide"),
            format!("most {MAX_RENDER_HEIGHT} pixels high"),
            format!("variable\n  {PDFIUM_DIR_VARIABLE} when it is set"),
            format!(r#""width":{DEFAULT_RENDER_WIDTH},"#),
        ] {
            assert!(CONTRACT.contains(&expected), "the help lacks {expected:?}");
        }

        // The short help of `--width` is built from the same constants.
        let command = Cli::command();
        let render = command
            .find_subcommand(Operation::Render.name())
            .expect("there is a render command");
        let width = render
            .get_arguments()
            .find(|argument| argument.get_id() == "width")
            .expect("render has a --width");
        let help = width.get_help().expect("--width has a help").to_string();
        assert!(
            help.contains(&format!("from 1 to {MAX_RENDER_WIDTH}")),
            "unexpected: {help}"
        );
        assert_eq!(
            width.get_default_values(),
            [DEFAULT_RENDER_WIDTH.to_string().as_str()]
        );
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
