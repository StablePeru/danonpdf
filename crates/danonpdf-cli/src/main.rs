//! `danonpdf-cli`: DanonPDF without the window.
//!
//! The desktop app needs a person in front of a screen. This program does
//! the same work from a command line, so a script or an AI agent can use it:
//! arguments in, one report out, and an exit code that says how it went.
//!
//! It is a thin layer, like the window. Each step has its own module:
//!
//! 1. `cli.rs` says which commands and arguments exist; clap parses them.
//! 2. `merge.rs`, `info.rs`, `extract.rs` and `split.rs` each run one
//!    operation: they apply the tool's rules and call the core.
//!    `outcome.rs` holds what they share: the report, the failure and the
//!    error codes.
//! 3. `output.rs` turns the outcome into text and an exit code.
//!
//! `main` only connects the three and does the one thing the others avoid:
//! writing to the terminal. There is no `windows_subsystem` line here, unlike
//! in the desktop app, because this program is meant to have a console.

mod cli;
mod extract;
mod info;
mod merge;
mod outcome;
mod output;
mod split;
mod texts;

use std::io::{self, Write};
use std::process::ExitCode;

use cli::{Cli, Command};
use output::Format;

// `main` returns an `ExitCode` and never calls `std::process::exit` itself:
// returning lets every value be dropped and every buffer be flushed first.
fn main() -> ExitCode {
    // This only comes back with a valid command line. For `--help`,
    // `--version` or a mistake, clap prints its own text and ends the
    // program right here (exit code 0, 0 and 2), before any file is touched.
    let cli = Cli::from_process_args();

    // Asked before the `match` below, which takes the command apart.
    let operation = cli.command.operation();

    // One arm per command. A new command adds a variant to `Command`, and
    // the compiler then asks for its arm here. Every arm gives the same
    // pair: what happened, and whether `--json` was given.
    let (outcome, json) = match cli.command {
        Command::Merge(args) => (
            merge::run(&args.inputs, &args.output, args.force),
            args.json,
        ),
        Command::Info(args) => (info::run(&args.input), args.json),
        Command::Extract(args) => (
            extract::run(&args.input, &args.pages, &args.output, args.force),
            args.json,
        ),
        Command::Split(args) => (
            split::run(&args.input, args.every, &args.output_dir, args.force),
            args.json,
        ),
    };
    let rendered = output::render(operation, &outcome, Format::from_json_flag(json));

    // `writeln!` and not `println!`: `println!` panics when stdout is gone
    // (the caller closed the pipe, as `head` does), and a panic would end
    // the program with code 101 and a message nobody asked for. The work is
    // already done and the exit code still tells how it went, so a line
    // that cannot be delivered is dropped: `let _ =` says "ignore this
    // result on purpose".
    if let Some(line) = rendered.stdout {
        let _ = writeln!(io::stdout(), "{line}");
    }
    if let Some(line) = rendered.stderr {
        let _ = writeln!(io::stderr(), "{line}");
    }
    ExitCode::from(rendered.exit_code)
}
