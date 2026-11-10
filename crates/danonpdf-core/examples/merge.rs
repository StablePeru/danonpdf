//! Merges PDF files from the command line, to try the core without the app:
//!
//! ```text
//! cargo run --release --example merge -- OUTPUT.pdf INPUT1.pdf INPUT2.pdf ...
//! ```

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    // `args_os` and not `args`: a file name does not have to be valid UTF-8,
    // and `args` would panic on one that is not. `skip(1)` drops the first
    // argument, which is the program's own name.
    let mut args = std::env::args_os().skip(1).map(PathBuf::from);
    let Some(output) = args.next() else {
        eprintln!("usage: merge OUTPUT.pdf INPUT1.pdf INPUT2.pdf ...");
        return ExitCode::FAILURE;
    };
    // The rest of the arguments are the inputs, in order.
    let inputs: Vec<PathBuf> = args.collect();

    let start = Instant::now();
    // `&inputs` lends the list to `merge_files`, so we can still print its
    // length afterwards.
    match danonpdf_core::merge_files(&inputs, &output) {
        Ok(page_count) => {
            println!(
                "Merged {} files ({page_count} pages) into {} in {:.1?}",
                inputs.len(),
                output.display(),
                start.elapsed(),
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            // Each `source()` is the error that caused the one before it.
            let mut cause = error.source();
            while let Some(inner) = cause {
                eprintln!("  caused by: {inner}");
                cause = inner.source();
            }
            ExitCode::FAILURE
        }
    }
}
