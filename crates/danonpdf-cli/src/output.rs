//! Turns the outcome of an operation into what the program says: a line for
//! stdout, a line for stderr, and an exit code.
//!
//! [`render`] only builds those three values; `main` is what writes them.
//! Tests can then check every word and every code without capturing the
//! output of a running program.
//!
//! There is one `render` for every operation. What differs between them
//! (the sentence, the fields of the JSON object) is chosen by a `match` on
//! the [`Report`]; what they share (where each line goes, the exit code,
//! the shape of a failure) is written once.
//!
//! The types that become JSON live here, not in the core: the core returns
//! plain Rust values and knows nothing about output formats, so adding
//! another format later touches this module only.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::outcome::{Failure, Operation, Report};
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
/// An enum and not the `bool` that `--json` is: `render(.., true)` says
/// nothing at the place where it is called, `Format::Json` does.
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

/// What to print, and the exit code, for the `outcome` of `operation` in
/// `format`.
///
/// `operation` is given apart from the outcome because a failure does not
/// know which operation it stopped, and the JSON report names it either
/// way.
///
/// In JSON, a failure goes to stdout too, and stderr stays empty: a caller
/// reads one stream and always finds exactly one object there. The exit
/// code already says whether it worked.
pub fn render(operation: Operation, outcome: &Result<Report, Failure>, format: Format) -> Rendered {
    // Matching on the pair lists the four cases side by side, and the
    // compiler checks that none is missing.
    match (outcome, format) {
        (Ok(report), Format::Human) => Rendered {
            stdout: Some(human_line(report)),
            stderr: None,
            exit_code: EXIT_OK,
        },
        (Ok(report), Format::Json) => Rendered {
            stdout: Some(to_json(&SuccessJson {
                ok: true,
                operation: operation.name(),
                details: DetailsJson::of(report),
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
                operation: operation.name(),
                error: ErrorJson {
                    code: failure.code.as_str(),
                    message: &failure.message,
                    // `as_deref` lends the `PathBuf` inside the `Option` as
                    // a `&Path`, and `map` converts it only if it is there.
                    path: failure.path.as_deref().map(lossy),
                    written: all_lossy(&failure.written),
                },
            })),
            stderr: None,
            exit_code: EXIT_FAILED,
        },
    }
}

/// The sentence for an operation that worked, without `--json`.
fn human_line(report: &Report) -> String {
    // One arm per operation. A pattern takes the variant apart: the names
    // in braces become variables holding references to its fields. The
    // counts are `&usize` here, hence the `*` to get the number itself.
    match report {
        Report::Merge {
            output,
            inputs,
            pages,
        } => texts::merged(*inputs, *pages, output),
        Report::Info { input, pages } => texts::page_count(input, *pages),
        Report::Extract { output, pages } => texts::extracted(*pages, output),
        Report::Split { outputs, pages } => texts::split(*pages, outputs),
    }
}

/// The JSON object for an operation that worked.
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
    /// `flatten` writes the fields of `details` straight into this object,
    /// after the two above, instead of nesting them under a "details" key.
    #[serde(flatten)]
    details: DetailsJson<'a>,
}

/// The fields that each operation adds to [`SuccessJson`], in the order
/// they are written.
///
/// It mirrors [`Report`], with every path already turned into text. A
/// separate type keeps the choice of JSON keys and their order in this
/// module, away from the values the operations return.
///
/// `untagged`: serde normally writes the name of the variant around its
/// fields (`{"Merge":{...}}`). The report already says which operation it
/// was, so only the fields are wanted.
#[derive(Serialize)]
#[serde(untagged)]
enum DetailsJson<'a> {
    Merge {
        output: Cow<'a, str>,
        inputs: usize,
        pages: usize,
    },
    Info {
        input: Cow<'a, str>,
        pages: usize,
    },
    Extract {
        output: Cow<'a, str>,
        pages: usize,
    },
    Split {
        outputs: Vec<Cow<'a, str>>,
        /// How many files were written: the length of `outputs`, spelled
        /// out so a caller need not count.
        files: usize,
        pages: usize,
    },
}

impl<'a> DetailsJson<'a> {
    /// The JSON fields for `report`.
    fn of(report: &'a Report) -> Self {
        // No `_` arm: a new kind of report does not build until it is
        // given its JSON fields here.
        match report {
            Report::Merge {
                output,
                inputs,
                pages,
            } => Self::Merge {
                output: lossy(output),
                inputs: *inputs,
                pages: *pages,
            },
            Report::Info { input, pages } => Self::Info {
                input: lossy(input),
                pages: *pages,
            },
            Report::Extract { output, pages } => Self::Extract {
                output: lossy(output),
                pages: *pages,
            },
            Report::Split { outputs, pages } => Self::Split {
                outputs: all_lossy(outputs),
                files: outputs.len(),
                pages: *pages,
            },
        }
    }
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
    /// The files written before the failure. Left out when there are none,
    /// which is every failure except a split that stopped midway: a caller
    /// that finds the key knows there are files to deal with.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    written: Vec<Cow<'a, str>>,
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

/// Every path of `paths` as text for JSON, in the same order.
fn all_lossy(paths: &[PathBuf]) -> Vec<Cow<'_, str>> {
    // `iter()` lends `&PathBuf`s and `lossy` wants a `&Path`: `as_path`
    // makes one from the other.
    paths.iter().map(|path| lossy(path.as_path())).collect()
}

/// `value` as JSON on one line.
///
/// `to_string` (not `to_string_pretty`) writes no line breaks of its own and
/// escapes the ones inside strings as `\n`, so the result is always one
/// line: a caller can read stdout line by line.
fn to_json<T: Serialize>(value: &T) -> String {
    // serde_json only fails when a type refuses to be serialised or a map
    // has keys that are not strings. The types above hold text, numbers,
    // lists of text and a bool, so neither can happen; if a later change
    // breaks that, every test that renders JSON fails here with this
    // message.
    serde_json::to_string(value).expect("the report types always serialise to JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outcome::ErrorCode;

    fn merge_report() -> Result<Report, Failure> {
        Ok(Report::Merge {
            output: PathBuf::from("out.pdf"),
            inputs: 3,
            pages: 12,
        })
    }

    fn failure(path: Option<&str>) -> Result<Report, Failure> {
        Err(Failure {
            code: ErrorCode::OutputExists,
            message: "the output out.pdf already exists".to_owned(),
            path: path.map(PathBuf::from),
            written: Vec::new(),
        })
    }

    /// What `report` prints as JSON, after checking that it is a success
    /// written to stdout only.
    fn json_success(operation: Operation, report: Report) -> String {
        let rendered = render(operation, &Ok(report), Format::Json);
        assert_eq!(rendered.stderr, None);
        assert_eq!(rendered.exit_code, EXIT_OK);
        rendered.stdout.expect("a success prints on stdout")
    }

    #[test]
    fn the_json_flag_chooses_the_format() {
        assert_eq!(Format::from_json_flag(true), Format::Json);
        assert_eq!(Format::from_json_flag(false), Format::Human);
    }

    #[test]
    fn human_success_is_one_line_on_stdout() {
        assert_eq!(
            render(Operation::Merge, &merge_report(), Format::Human),
            Rendered {
                stdout: Some("Merged 3 files (12 pages) into out.pdf".to_owned()),
                stderr: None,
                exit_code: EXIT_OK,
            }
        );
    }

    #[test]
    fn every_operation_has_its_own_human_line() {
        let cases = [
            (
                Report::Info {
                    input: PathBuf::from("x.pdf"),
                    pages: 12,
                },
                "x.pdf has 12 pages",
            ),
            (
                Report::Extract {
                    output: PathBuf::from("out.pdf"),
                    pages: 4,
                },
                "Extracted 4 pages into out.pdf",
            ),
            (
                Report::Split {
                    outputs: vec![
                        PathBuf::from("dir/x-001.pdf"),
                        PathBuf::from("dir/x-002.pdf"),
                    ],
                    pages: 30,
                },
                "Split 30 pages into 2 files: dir/x-001.pdf to dir/x-002.pdf",
            ),
        ];
        for (report, line) in cases {
            // The operation only matters for JSON; any one will do here.
            let rendered = render(Operation::Info, &Ok(report), Format::Human);

            assert_eq!(
                rendered,
                Rendered {
                    stdout: Some(line.to_owned()),
                    stderr: None,
                    exit_code: EXIT_OK,
                }
            );
        }
    }

    #[test]
    fn human_failure_is_one_line_on_stderr_and_nothing_on_stdout() {
        assert_eq!(
            render(Operation::Merge, &failure(Some("out.pdf")), Format::Human),
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
            render(Operation::Merge, &merge_report(), Format::Json),
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
    fn json_info_has_the_documented_shape() {
        let report = Report::Info {
            input: PathBuf::from("x.pdf"),
            pages: 12,
        };

        assert_eq!(
            json_success(Operation::Info, report),
            r#"{"ok":true,"operation":"info","input":"x.pdf","pages":12}"#
        );
    }

    #[test]
    fn json_extract_has_the_documented_shape() {
        let report = Report::Extract {
            output: PathBuf::from("out.pdf"),
            pages: 4,
        };

        assert_eq!(
            json_success(Operation::Extract, report),
            r#"{"ok":true,"operation":"extract","output":"out.pdf","pages":4}"#
        );
    }

    #[test]
    fn json_split_has_the_documented_shape() {
        let report = Report::Split {
            outputs: vec![
                PathBuf::from("dir/x-001.pdf"),
                PathBuf::from("dir/x-002.pdf"),
            ],
            pages: 30,
        };

        assert_eq!(
            json_success(Operation::Split, report),
            concat!(
                r#"{"ok":true,"operation":"split","#,
                r#""outputs":["dir/x-001.pdf","dir/x-002.pdf"],"files":2,"pages":30}"#
            )
        );
    }

    #[test]
    fn json_failure_goes_to_stdout_and_leaves_stderr_empty() {
        let expected = concat!(
            r#"{"ok":false,"operation":"merge","error":{"code":"output_exists","#,
            r#""message":"the output out.pdf already exists","path":"out.pdf"}}"#
        );
        assert_eq!(
            render(Operation::Merge, &failure(Some("out.pdf")), Format::Json),
            Rendered {
                stdout: Some(expected.to_owned()),
                stderr: None,
                exit_code: EXIT_FAILED,
            }
        );
    }

    #[test]
    fn json_failure_names_the_operation_that_failed() {
        for operation in [Operation::Info, Operation::Extract, Operation::Split] {
            let line = render(operation, &failure(None), Format::Json)
                .stdout
                .unwrap();

            let start = format!(
                r#"{{"ok":false,"operation":"{}","error":"#,
                operation.name()
            );
            assert!(line.starts_with(&start), "unexpected: {line}");
        }
    }

    #[test]
    fn json_failure_without_a_file_has_no_path_key() {
        let rendered = render(Operation::Merge, &failure(None), Format::Json);
        assert_eq!(
            rendered.stdout.as_deref(),
            Some(concat!(
                r#"{"ok":false,"operation":"merge","error":{"code":"output_exists","#,
                r#""message":"the output out.pdf already exists"}}"#
            ))
        );
    }

    #[test]
    fn json_failure_of_a_split_that_stopped_lists_what_it_wrote() {
        let outcome = Err(Failure {
            code: ErrorCode::SaveFailed,
            message: "the split stopped after 2 of 3 files: cannot save dir/x-003.pdf".to_owned(),
            path: Some(PathBuf::from("dir/x-003.pdf")),
            written: vec![
                PathBuf::from("dir/x-001.pdf"),
                PathBuf::from("dir/x-002.pdf"),
            ],
        });

        let rendered = render(Operation::Split, &outcome, Format::Json);

        // `written` comes last, after `path`, and only because it is not
        // empty: the two tests above show the object without it.
        assert_eq!(
            rendered.stdout.as_deref(),
            Some(concat!(
                r#"{"ok":false,"operation":"split","error":{"code":"save_failed","#,
                r#""message":"the split stopped after 2 of 3 files: cannot save dir/x-003.pdf","#,
                r#""path":"dir/x-003.pdf","written":["dir/x-001.pdf","dir/x-002.pdf"]}}"#
            ))
        );
        assert_eq!(rendered.exit_code, EXIT_FAILED);
    }

    #[test]
    fn json_escapes_what_a_path_may_hold() {
        // A Windows path is full of backslashes, and a quote is a legal
        // character in a Linux file name. Both must be escaped, or the
        // object would not parse.
        let report = Report::Merge {
            output: PathBuf::from(r#"C:\docs\my "final" file.pdf"#),
            inputs: 2,
            pages: 2,
        };

        let line = json_success(Operation::Merge, report);

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
            written: Vec::new(),
        });

        let line = render(Operation::Merge, &outcome, Format::Json)
            .stdout
            .unwrap();

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
        let report = Report::Merge {
            output: PathBuf::from(OsStr::from_bytes(b"caf\xe9.pdf")),
            inputs: 2,
            pages: 2,
        };

        let line = json_success(Operation::Merge, report);

        let parsed: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(parsed["output"], "caf\u{fffd}.pdf");
    }
}
