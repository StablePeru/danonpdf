//! Turns the outcome of an operation into what the program says: a line for
//! stdout, a line for stderr, and an exit code.
//!
//! [`render`] only builds those three values; `main` is what writes them.
//! Tests can then check every word and every code without capturing the
//! output of a running program.
//!
//! The types that become JSON live here, not in the core: the core returns
//! plain Rust values and knows nothing about output formats, so adding
//! another format later touches this module only.

use std::borrow::Cow;
use std::path::Path;

use serde::Serialize;

use crate::merge::{Failure, MergeReport, OPERATION};
use crate::texts;

/// Exit code when the operation worked.
pub const EXIT_OK: u8 = 0;
/// Exit code when the operation was refused or failed.
///
/// The third code, 2 for a command line that is itself wrong, is not here
/// because it never passes through this module: clap prints its own message
/// and ends the program with 2 before any operation starts (see `cli.rs`).
pub const EXIT_FAILED: u8 = 1;

/// How the outcome is written.
///
/// An enum and not the `bool` that `--json` is: `render(&outcome, true)`
/// says nothing at the place where it is called, `Format::Json` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// One sentence for a person: on stdout if it worked, on stderr if not.
    Human,
    /// One JSON object on stdout, whether it worked or not.
    Json,
}

impl Format {
    /// The format that the `--json` flag asks for.
    pub fn from_json_flag(json: bool) -> Self {
        if json { Self::Json } else { Self::Human }
    }
}

/// Everything the program does once the operation has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The line for stdout, without its newline. `None` prints nothing.
    pub stdout: Option<String>,
    /// The line for stderr, without its newline. `None` prints nothing.
    pub stderr: Option<String>,
    pub exit_code: u8,
}

/// What to print, and the exit code, for `outcome` in `format`.
///
/// In JSON, a failure goes to stdout too, and stderr stays empty: a caller
/// reads one stream and always finds exactly one object there. The exit
/// code already says whether it worked.
pub fn render(outcome: &Result<MergeReport, Failure>, format: Format) -> Rendered {
    // Matching on the pair lists the four cases side by side, and the
    // compiler checks that none is missing.
    match (outcome, format) {
        (Ok(report), Format::Human) => Rendered {
            stdout: Some(texts::merged(report.inputs, report.pages, &report.output)),
            stderr: None,
            exit_code: EXIT_OK,
        },
        (Ok(report), Format::Json) => Rendered {
            stdout: Some(to_json(&SuccessJson {
                ok: true,
                operation: OPERATION,
                output: lossy(&report.output),
                inputs: report.inputs,
                pages: report.pages,
            })),
            stderr: None,
            exit_code: EXIT_OK,
        },
        (Err(failure), Format::Human) => Rendered {
            stdout: None,
            stderr: Some(texts::error_line(&failure.message)),
            exit_code: EXIT_FAILED,
        },
        (Err(failure), Format::Json) => Rendered {
            stdout: Some(to_json(&FailureJson {
                ok: false,
                operation: OPERATION,
                error: ErrorJson {
                    code: failure.code.as_str(),
                    message: &failure.message,
                    // `as_deref` lends the `PathBuf` inside the `Option` as
                    // a `&Path`, and `map` converts it only if it is there.
                    path: failure.path.as_deref().map(lossy),
                },
            })),
            stderr: None,
            exit_code: EXIT_FAILED,
        },
    }
}

/// The JSON object for a merge that worked.
///
/// The fields are written in the order they are declared, so the object
/// starts with `ok` and `operation` whatever the outcome: a caller (or a
/// person reading a log) sees first what happened, then the details.
///
/// `'a` is a lifetime: the struct borrows its text from the report instead
/// of copying it, and `'a` tells the compiler that the report must outlive
/// this short-lived value.
#[derive(Serialize)]
struct SuccessJson<'a> {
    /// Always `true` here. It is a field, not something implied by the
    /// shape, so a caller can test one key before reading anything else.
    ok: bool,
    operation: &'a str,
    output: Cow<'a, str>,
    inputs: usize,
    pages: usize,
}

/// The JSON object for an operation that was refused or failed.
#[derive(Serialize)]
struct FailureJson<'a> {
    /// Always `false` here.
    ok: bool,
    operation: &'a str,
    error: ErrorJson<'a>,
}

/// The `error` part of [`FailureJson`].
#[derive(Serialize)]
struct ErrorJson<'a> {
    code: &'a str,
    message: &'a str,
    /// Left out of the object, not written as `null`, when the failure is
    /// about no file: a caller tests "is there a path?" in one way only.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<Cow<'a, str>>,
}

/// `path` as text for JSON.
///
/// JSON strings are Unicode, but a path is not always valid Unicode (a
/// Linux file name is any bytes; a Windows one may hold half of a UTF-16
/// pair). `to_string_lossy` replaces what cannot be written with "�". For
/// such a path the text in the report is therefore not the exact name on
/// disk: good enough to tell which file it was, not to open it again.
///
/// `Cow` ("clone on write") is why this costs nothing in the usual case: it
/// borrows the path's own bytes when they are valid, and only builds a new
/// `String` when it has to replace something.
fn lossy(path: &Path) -> Cow<'_, str> {
    path.to_string_lossy()
}

/// `value` as JSON on one line.
///
/// `to_string` (not `to_string_pretty`) writes no line breaks of its own and
/// escapes the ones inside strings as `\n`, so the result is always one
/// line: a caller can read stdout line by line.
fn to_json<T: Serialize>(value: &T) -> String {
    // serde_json only fails when a type refuses to be serialised or a map
    // has keys that are not strings. The structs above hold text, numbers
    // and a bool, so neither can happen; if a later change breaks that,
    // every test that renders JSON fails here with this message.
    serde_json::to_string(value).expect("the report types always serialise to JSON")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::merge::ErrorCode;

    fn report() -> Result<MergeReport, Failure> {
        Ok(MergeReport {
            output: PathBuf::from("out.pdf"),
            inputs: 3,
            pages: 12,
        })
    }

    fn failure(path: Option<&str>) -> Result<MergeReport, Failure> {
        Err(Failure {
            code: ErrorCode::OutputExists,
            message: "the output out.pdf already exists".to_owned(),
            path: path.map(PathBuf::from),
        })
    }

    #[test]
    fn the_json_flag_chooses_the_format() {
        assert_eq!(Format::from_json_flag(true), Format::Json);
        assert_eq!(Format::from_json_flag(false), Format::Human);
    }

    #[test]
    fn human_success_is_one_line_on_stdout() {
        assert_eq!(
            render(&report(), Format::Human),
            Rendered {
                stdout: Some("Merged 3 files (12 pages) into out.pdf".to_owned()),
                stderr: None,
                exit_code: EXIT_OK,
            }
        );
    }

    #[test]
    fn human_failure_is_one_line_on_stderr_and_nothing_on_stdout() {
        assert_eq!(
            render(&failure(Some("out.pdf")), Format::Human),
            Rendered {
                stdout: None,
                stderr: Some("error: the output out.pdf already exists".to_owned()),
                exit_code: EXIT_FAILED,
            }
        );
    }

    #[test]
    fn json_success_has_the_documented_shape() {
        // Compared as text, not as parsed JSON: the order of the fields and
        // the lack of spaces are part of what the help shows.
        assert_eq!(
            render(&report(), Format::Json),
            Rendered {
                stdout: Some(
                    r#"{"ok":true,"operation":"merge","output":"out.pdf","inputs":3,"pages":12}"#
                        .to_owned()
                ),
                stderr: None,
                exit_code: EXIT_OK,
            }
        );
    }

    #[test]
    fn json_failure_goes_to_stdout_and_leaves_stderr_empty() {
        let expected = concat!(
            r#"{"ok":false,"operation":"merge","error":{"code":"output_exists","#,
            r#""message":"the output out.pdf already exists","path":"out.pdf"}}"#
        );
        assert_eq!(
            render(&failure(Some("out.pdf")), Format::Json),
            Rendered {
                stdout: Some(expected.to_owned()),
                stderr: None,
                exit_code: EXIT_FAILED,
            }
        );
    }

    #[test]
    fn json_failure_without_a_file_has_no_path_key() {
        let rendered = render(&failure(None), Format::Json);
        assert_eq!(
            rendered.stdout.as_deref(),
            Some(concat!(
                r#"{"ok":false,"operation":"merge","error":{"code":"output_exists","#,
                r#""message":"the output out.pdf already exists"}}"#
            ))
        );
    }

    #[test]
    fn json_escapes_what_a_path_may_hold() {
        // A Windows path is full of backslashes, and a quote is a legal
        // character in a Linux file name. Both must be escaped, or the
        // object would not parse.
        let outcome = Ok(MergeReport {
            output: PathBuf::from(r#"C:\docs\my "final" file.pdf"#),
            inputs: 2,
            pages: 2,
        });

        let line = render(&outcome, Format::Json).stdout.unwrap();

        assert!(
            line.contains(r#""output":"C:\\docs\\my \"final\" file.pdf""#),
            "unexpected: {line}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["output"], r#"C:\docs\my "final" file.pdf"#);
    }

    #[test]
    fn json_is_always_a_single_line() {
        // A newline inside a message must not split the object in two: a
        // caller reads one line.
        let outcome = Err(Failure {
            code: ErrorCode::SaveFailed,
            message: "first line\nsecond line".to_owned(),
            path: None,
        });

        let line = render(&outcome, Format::Json).stdout.unwrap();

        assert!(!line.contains('\n'), "unexpected: {line}");
    }

    // Linux file names are bytes, not text, so they can be invalid UTF-8.
    // Windows names are built differently, so this test only exists on
    // Unix-like systems.
    #[cfg(unix)]
    #[test]
    fn a_path_that_is_not_utf8_is_written_with_replacement_characters() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        // 0xE9 is "é" in Latin-1, but on its own it is not valid UTF-8.
        let outcome = Ok(MergeReport {
            output: PathBuf::from(OsStr::from_bytes(b"caf\xe9.pdf")),
            inputs: 2,
            pages: 2,
        });

        let line = render(&outcome, Format::Json).stdout.unwrap();

        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["output"], "caf\u{fffd}.pdf");
    }
}
