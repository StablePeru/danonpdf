//! What an operation gives back, whichever operation it is: a [`Report`]
//! when it worked, a [`Failure`] when it did not.
//!
//! Each operation has its own module (`merge.rs`, `info.rs`, `extract.rs`,
//! `split.rs`, `render.rs`, `rotate.rs`, `delete.rs`) with a `run` function
//! that returns `Result<Report, Failure>`. Nothing in those modules prints
//! or ends the program: `output.rs` decides how to show either value. That
//! split keeps the rules testable without starting a program and reading
//! its output.
//!
//! What the operations share lives here: the names they go by, the shape
//! of a failure, the two places where an error of the core gets its code
//! (one for each error type of the core), and the two rules about what may
//! be written where.

use std::path::{Path, PathBuf};

use danonpdf_core::{PdfError, RenderError, WhichInput, error_chain, name_is_taken, same_file};

use crate::texts;

/// The operations of the tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    Merge,
    Info,
    Extract,
    Split,
    Render,
    Rotate,
    Delete,
}

impl Operation {
    /// The name of the operation: the word typed on the command line
    /// (`danonpdf-cli merge ...`) and the value of `"operation"` in the
    /// JSON report.
    ///
    /// Both come from here (`cli.rs` gives each command this name), so the
    /// two cannot drift apart. These strings are part of the contract with
    /// every caller: a variant may be renamed freely, its string never.
    ///
    /// `const fn`: it can also be called where the compiler needs the value
    /// before the program runs.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Merge => "merge",
            Self::Info => "info",
            Self::Extract => "extract",
            Self::Split => "split",
            Self::Render => "render",
            Self::Rotate => "rotate",
            Self::Delete => "delete",
        }
    }
}

/// What an operation that worked has to tell.
///
/// One variant per operation, each with the facts of that operation and no
/// others: an `Info` has no output, a `Split` has many. With one struct for
/// all of them, most fields would be empty most of the time, and nothing
/// would say which ones count.
///
/// Every path is kept exactly as the caller spelled it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Report {
    Merge {
        /// Where the merged file was written.
        output: PathBuf,
        /// How many input files were read. The same file given twice
        /// counts twice.
        inputs: usize,
        /// How many pages the merged file has.
        pages: usize,
    },
    Info {
        /// The file that was looked at.
        input: PathBuf,
        /// How many pages it has.
        pages: usize,
    },
    Extract {
        /// Where the new file was written.
        output: PathBuf,
        /// How many pages it has.
        pages: usize,
    },
    Split {
        /// The files that were written, in page order.
        outputs: Vec<PathBuf>,
        /// How many pages they hold together: every page of the input.
        pages: usize,
    },
    Render {
        /// Where the image was written.
        output: PathBuf,
        /// The page that was drawn, counted from 1.
        page: usize,
        /// The size of the image that was written, in pixels. It is read
        /// from the image, not from the command line: a very tall page
        /// comes out narrower than the width that was asked for.
        width: u32,
        height: u32,
    },
    Rotate {
        /// Where the new file was written.
        output: PathBuf,
        /// How many pages it has: every page of the input, the ones that
        /// were turned and the ones that were not.
        pages: usize,
    },
    Delete {
        /// Where the new file was written.
        output: PathBuf,
        /// How many pages it has: the ones that were not deleted.
        pages: usize,
    },
}

/// Why an operation did not happen, or did not finish.
///
/// Views of the same failure for different readers: `code` for a program
/// that decides what to do next, `message` for a person (or a language
/// model) reading the reason, and `path` for whoever needs the file without
/// digging it out of the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub code: ErrorCode,
    /// The reason, with every cause behind it, on one line.
    pub message: String,
    /// The file the failure is about. `None` when it is about no file in
    /// particular.
    pub path: Option<PathBuf>,
    /// The files that were written before the failure, in order. They are
    /// complete and are kept. Only a split can fill it: every other
    /// operation writes one file or none.
    pub written: Vec<PathBuf>,
}

/// The kind of failure, as a caller can test for it.
///
/// An enum and not a string: the compiler then knows every kind there is, so
/// a `match` on it cannot forget one, and a typo is a build error instead of
/// a code that no caller expects.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    /// An output is one of the input files. `--force` does not lift this.
    OutputIsInput,
    /// An output already exists and `--force` was not given.
    OutputExists,
    /// There was nothing to merge.
    NoInputs,
    /// An input could not be read, or is not a PDF.
    ReadFailed,
    /// An input needs a password or, when drawing, is protected in a way
    /// that cannot be opened at all.
    Encrypted,
    /// An input opened, but its structure is damaged.
    BrokenPdf,
    /// A page was asked for that the document does not have.
    PageOutOfRange,
    /// A page was asked for more than once.
    PageRepeated,
    /// The new document would have no pages.
    NoPagesLeft,
    /// An output file, or the folder for it, could not be written.
    SaveFailed,
    /// The library that draws pages (PDFium) is missing or cannot be
    /// loaded. Nothing is wrong with the input, and only `render` needs
    /// that library.
    ///
    /// The failure names the library file that was looked for, with one
    /// exception: when the program cannot tell which folder it runs from,
    /// there is no file to name and the failure has no path. The way out
    /// is then the variable that names the folder (`DANONPDF_PDFIUM_DIR`).
    RendererUnavailable,
    /// The page was not drawn, and no other code says why. In practice:
    /// PDFium gave up on the file without giving a reason.
    ///
    /// It is the code for whatever is left, so a failure that is not the
    /// library's also lands here if it ever comes up from the core: a width
    /// that cannot be drawn. This tool refuses such a width on the command
    /// line (a usage error) before the core is called, which is why the
    /// documentation can tell a caller that nothing in its command line is
    /// wrong.
    RenderFailed,
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
            Self::PageOutOfRange => "page_out_of_range",
            Self::PageRepeated => "page_repeated",
            Self::NoPagesLeft => "no_pages_left",
            Self::SaveFailed => "save_failed",
            Self::RendererUnavailable => "renderer_unavailable",
            Self::RenderFailed => "render_failed",
        }
    }
}

/// The two rules about what may be written where, checked before the core
/// is called, in this order:
///
/// 1. No output may be one of the inputs, whatever `force` says. The core
///    could do it safely (it reads everything before it writes), but a
///    caller that names an input as the output has almost always mixed up
///    its arguments, and the result would replace a file it still wants.
/// 2. No output may exist yet, unless `force` is true.
///
/// The first rule comes first so that the answer does not depend on `force`:
/// an input always exists, so with the other order the same mistake would
/// be "exists" without `--force` and "is an input" with it.
///
/// Every output is checked before the caller writes any of them, and the
/// failure names the first one that breaks a rule, so an operation with
/// several outputs writes all of them or none because of these rules.
///
/// `I` and `O` can be anything that looks like a path (`&Path`,
/// `PathBuf`...), so a caller passes the lists it already has.
///
/// Known limit: the second rule looks first and writes later. Another
/// program can create an output in between, and the save would then replace
/// that new file. Closing the gap needs a "rename, but only if the name is
/// free" that std does not offer on every system; for a tool run by one
/// person or one agent on its own files, the window is accepted.
pub fn check_outputs<I, O>(inputs: &[I], outputs: &[O], force: bool) -> Result<(), Failure>
where
    I: AsRef<Path>,
    O: AsRef<Path>,
{
    // The same lists as plain `&Path`s, whatever type the caller's hold.
    let inputs: Vec<&Path> = inputs.iter().map(AsRef::as_ref).collect();
    let outputs: Vec<&Path> = outputs.iter().map(AsRef::as_ref).collect();

    // `same_file` is the core's rule, the one the window uses too, so both
    // refuse the same outputs: another spelling of an input is still it.
    // `find` gives the first output for which the test holds, or `None`;
    // the test itself asks `any`: is there an input that is this file?
    let an_input = outputs
        .iter()
        .find(|output| inputs.iter().any(|input| same_file(input, output)));
    if let Some(output) = an_input {
        return Err(Failure {
            code: ErrorCode::OutputIsInput,
            message: texts::output_is_input(output),
            path: Some(output.to_path_buf()),
            written: Vec::new(),
        });
    }

    if force {
        return Ok(());
    }
    // Also the core's rule, and also shared with the window: a name counts
    // as taken even when it is only a link whose target is gone.
    match outputs.iter().find(|output| name_is_taken(output)) {
        Some(output) => Err(Failure {
            code: ErrorCode::OutputExists,
            message: texts::output_exists(output),
            path: Some(output.to_path_buf()),
            written: Vec::new(),
        }),
        None => Ok(()),
    }
}

/// Turns the core's error into the tool's failure: a stable code, the whole
/// message, and the file it is about.
///
/// `inputs` is the list the operation was called with. Two kinds of error
/// do not carry a path, only which input they are about (the only one, or
/// its place in the list); with the list at hand, the path can be looked
/// up for the caller.
pub fn failure_from<P: AsRef<Path>>(error: &PdfError, inputs: &[P]) -> Failure {
    // No `_ =>` arm, on purpose. With one, a variant added to `PdfError`
    // later would fall into it silently and reach callers under a wrong
    // code. Without one, the compiler refuses to build this crate until the
    // new variant is given a code here.
    let (code, path) = match error {
        PdfError::NoInputs => (ErrorCode::NoInputs, None),
        PdfError::Read { path, .. } => (ErrorCode::ReadFailed, Some(path.clone())),
        PdfError::Encrypted { input } => (ErrorCode::Encrypted, path_of(inputs, *input)),
        PdfError::Broken { input, .. } => (ErrorCode::BrokenPdf, path_of(inputs, *input)),
        // About the pages that were asked for, not about a file: no path.
        PdfError::PageOutOfRange { .. } => (ErrorCode::PageOutOfRange, None),
        // Also about the pages that were asked for: no path either.
        PdfError::PageRepeated { .. } => (ErrorCode::PageRepeated, None),
        PdfError::NoPagesLeft => (ErrorCode::NoPagesLeft, None),
        // Two ways for the output not to reach the disk, one code: a caller
        // does the same about both (check `path` and what is around it).
        // `|` joins two patterns into one arm; both bind `path` to the same
        // type, which is what makes that allowed.
        PdfError::CreateFolder { path, .. } | PdfError::Save { path, .. } => {
            (ErrorCode::SaveFailed, Some(path.clone()))
        }
    };
    Failure {
        code,
        // The core's message says what failed; the errors behind it say
        // why. `error_chain` joins them, as the window's status line does.
        message: error_chain(error),
        path,
        written: Vec::new(),
    }
}

/// Turns an error of the core's `render` module into the tool's failure,
/// as [`failure_from`] does for every other error of the core.
///
/// `input` is the file that was to be drawn. Drawing reads one file, so an
/// error that is about a file and carries no path of its own is about that
/// one.
///
/// A code that the other operations already use is reused wherever a
/// caller would do the same about the failure, so a script that handles
/// `read_failed` for `info` handles it for `render` with no new case. Two
/// codes are new, for the two failures that only drawing has.
pub fn render_failure_from(error: &RenderError, input: &Path) -> Failure {
    // The path for the errors that are about the file being drawn. A
    // closure, so the path is only copied in the arms that use it.
    let drawn_file = || Some(input.to_path_buf());
    // No `_ =>` arm, for the reason given in `failure_from`: a variant
    // added to `RenderError` must be given its code here before this crate
    // builds again.
    let (code, path) = match error {
        // The next four are the ways for PDFium not to be there. A caller
        // does the same about all of them (put the library where the tool
        // looks for it, or stop asking for images), and none says anything
        // about the input. `path` is the library file that was looked for.
        //
        // The first one has no file to name: the program could not tell
        // which folder it runs from, so it never got as far as a file
        // name. This is the one `renderer_unavailable` without a path, and
        // AGENTS.md says so: the caller's way out is to name the folder in
        // `DANONPDF_PDFIUM_DIR`, which is used instead of the program's
        // own folder.
        RenderError::LibraryFolderUnknown { .. } => (ErrorCode::RendererUnavailable, None),
        RenderError::LibraryMissing { path } | RenderError::LibraryFailed { path, .. } => {
            (ErrorCode::RendererUnavailable, Some(path.clone()))
        }
        // Not reached by this tool: it loads the library once per run. If
        // it ever is, the file that could not be used is the one asked for.
        RenderError::LibraryAlreadyLoaded { asked, .. } => {
            (ErrorCode::RendererUnavailable, Some(asked.clone()))
        }
        RenderError::Read { path, .. } => (ErrorCode::ReadFailed, Some(path.clone())),
        // "Read, and not a PDF" is what `read_failed` already means for
        // the other operations: their `PdfError::Read` covers a file that
        // cannot be opened and one that is not a PDF alike, and the core
        // words both errors the same.
        RenderError::NotPdf { path } => (ErrorCode::ReadFailed, Some(path.clone())),
        RenderError::Encrypted => (ErrorCode::Encrypted, drawn_file()),
        // Protected in a way that PDFium cannot open, password or not.
        // `encrypted` and not `broken_pdf`: the file is not damaged, and
        // what a caller can do is what it does about a password it does
        // not have, which is to find a copy without the protection. The
        // other operations already report such a file as `encrypted`: for
        // them, every protected file that did not open is one.
        RenderError::UnsupportedProtection => (ErrorCode::Encrypted, drawn_file()),
        RenderError::Broken { .. } => (ErrorCode::BrokenPdf, drawn_file()),
        // About the page that was asked for, not about a file: no path.
        RenderError::PageOutOfRange { .. } => (ErrorCode::PageOutOfRange, None),
        // Not reached by this tool: the command line refuses such a width
        // first, as a usage error (exit code 2), so no caller ever sees
        // this arm. It still needs a code, because the `match` has no `_`
        // arm. `render_failed` is the code for "not drawn, and no other
        // code says why" (see `ErrorCode::RenderFailed`), and the message
        // is the core's own sentence about the width. No path: the
        // failure is about a number, not about a file. A new code for a
        // case that cannot happen would be one more promise to keep.
        RenderError::WidthOutOfRange { .. } => (ErrorCode::RenderFailed, None),
        // PDFium gave up on this file without saying why.
        RenderError::Engine { .. } => (ErrorCode::RenderFailed, drawn_file()),
        RenderError::Save { path, .. } => (ErrorCode::SaveFailed, Some(path.clone())),
    };
    Failure {
        code,
        message: error_chain(error),
        path,
        written: Vec::new(),
    }
}

/// The path of the input that `which` speaks of. `None` when the list has
/// no such input.
///
/// The message words the two cases differently ("the file", "input 2"),
/// but a caller that reads `path` gets the file either way.
fn path_of<P: AsRef<Path>>(inputs: &[P], which: WhichInput) -> Option<PathBuf> {
    // No `_ =>` arm here either: a third way of naming an input would have
    // to say where its path is.
    let input = match which {
        // An operation that reads one file is called with a list of one.
        // `first` gives `None` for an empty list instead of panicking.
        WhichInput::Only => inputs.first()?,
        // The core counts from 1. `checked_sub` gives `None` for 0 instead
        // of wrapping below zero, and `get` gives `None` past the end
        // instead of panicking as `inputs[i]` would. `?` passes the first
        // `None` on.
        WhichInput::Numbered(number) => inputs.get(number.checked_sub(1)?)?,
    };
    Some(input.as_ref().to_path_buf())
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::*;

    /// The two inputs every case below was "called" with.
    fn two_inputs() -> [PathBuf; 2] {
        [PathBuf::from("first.pdf"), PathBuf::from("second.pdf")]
    }

    #[test]
    fn every_core_error_has_a_code_and_names_its_file() {
        // One case per variant of `PdfError`. If the core grows a variant,
        // the `match` in `failure_from` (which has no `_` arm) stops
        // compiling, so this test cannot pass until the new variant has a
        // code. Add its case here at the same time.
        let cases: [(PdfError, ErrorCode, Option<&str>); 9] = [
            (PdfError::NoInputs, ErrorCode::NoInputs, None),
            (
                PdfError::Read {
                    path: PathBuf::from("second.pdf"),
                    source: lopdf::Error::InvalidPassword,
                },
                ErrorCode::ReadFailed,
                Some("second.pdf"),
            ),
            (
                PdfError::Encrypted {
                    input: WhichInput::Numbered(2),
                },
                ErrorCode::Encrypted,
                Some("second.pdf"),
            ),
            (
                PdfError::Broken {
                    input: WhichInput::Numbered(1),
                    reason: "it has no catalog",
                },
                ErrorCode::BrokenPdf,
                Some("first.pdf"),
            ),
            (
                PdfError::PageOutOfRange { page: 9, pages: 5 },
                ErrorCode::PageOutOfRange,
                None,
            ),
            (
                PdfError::PageRepeated { page: 3 },
                ErrorCode::PageRepeated,
                None,
            ),
            (PdfError::NoPagesLeft, ErrorCode::NoPagesLeft, None),
            (
                PdfError::CreateFolder {
                    path: PathBuf::from("parts"),
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
                },
                ErrorCode::SaveFailed,
                Some("parts"),
            ),
            (
                PdfError::Save {
                    path: PathBuf::from("out.pdf"),
                    source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
                },
                ErrorCode::SaveFailed,
                Some("out.pdf"),
            ),
        ];

        for (error, code, path) in cases {
            let failure = failure_from(&error, &two_inputs());

            assert_eq!(failure.code, code, "{error:?}");
            assert_eq!(failure.path.as_deref(), path.map(Path::new), "{error:?}");
            assert_eq!(failure.message, error_chain(&error), "{error:?}");
            assert_eq!(failure.written, Vec::<PathBuf>::new(), "{error:?}");
        }
    }

    #[test]
    fn every_render_error_has_a_code_and_names_its_file() {
        // One case per variant of `RenderError`, as above for `PdfError`:
        // the `match` in `render_failure_from` has no `_` arm either.
        let denied = || io::Error::new(io::ErrorKind::PermissionDenied, "access denied");
        let library = "app/pdfium.dll";
        let cases: [(RenderError, ErrorCode, Option<&str>); 13] = [
            // The library is not there, in each of its four ways. The
            // path is that of the library, never that of the input.
            (
                RenderError::LibraryFolderUnknown { source: denied() },
                ErrorCode::RendererUnavailable,
                None,
            ),
            (
                RenderError::LibraryMissing {
                    path: PathBuf::from(library),
                },
                ErrorCode::RendererUnavailable,
                Some(library),
            ),
            (
                RenderError::LibraryFailed {
                    path: PathBuf::from(library),
                    source: "not a valid library".into(),
                },
                ErrorCode::RendererUnavailable,
                Some(library),
            ),
            (
                RenderError::LibraryAlreadyLoaded {
                    loaded: PathBuf::from("other/pdfium.dll"),
                    asked: PathBuf::from(library),
                },
                ErrorCode::RendererUnavailable,
                Some(library),
            ),
            // The input, with the codes the other operations give it.
            (
                RenderError::Read {
                    path: PathBuf::from("gone.pdf"),
                    source: denied(),
                },
                ErrorCode::ReadFailed,
                Some("gone.pdf"),
            ),
            (
                RenderError::NotPdf {
                    path: PathBuf::from("notes.pdf"),
                },
                ErrorCode::ReadFailed,
                Some("notes.pdf"),
            ),
            (
                RenderError::Encrypted,
                ErrorCode::Encrypted,
                Some("report.pdf"),
            ),
            (
                RenderError::UnsupportedProtection,
                ErrorCode::Encrypted,
                Some("report.pdf"),
            ),
            (
                RenderError::Broken {
                    reason: "a page cannot be read",
                },
                ErrorCode::BrokenPdf,
                Some("report.pdf"),
            ),
            // What was asked for, not a file: no path.
            (
                RenderError::PageOutOfRange { page: 9, pages: 5 },
                ErrorCode::PageOutOfRange,
                None,
            ),
            (
                RenderError::WidthOutOfRange {
                    width: 0,
                    maximum: 4096,
                },
                ErrorCode::RenderFailed,
                None,
            ),
            (
                RenderError::Engine {
                    doing: "drawing the page",
                    detail: "Unknown".to_owned(),
                },
                ErrorCode::RenderFailed,
                Some("report.pdf"),
            ),
            (
                RenderError::Save {
                    path: PathBuf::from("page.png"),
                    source: denied(),
                },
                ErrorCode::SaveFailed,
                Some("page.png"),
            ),
        ];

        for (error, code, path) in cases {
            let failure = render_failure_from(&error, Path::new("report.pdf"));

            assert_eq!(failure.code, code, "{error:?}");
            assert_eq!(failure.path.as_deref(), path.map(Path::new), "{error:?}");
            assert_eq!(failure.message, error_chain(&error), "{error:?}");
            assert_eq!(failure.written, Vec::<PathBuf>::new(), "{error:?}");
        }
    }

    #[test]
    fn a_library_folder_that_is_unknown_is_renderer_unavailable_without_a_path() {
        // AGENTS.md promises this case to callers by name: the one
        // `renderer_unavailable` whose report has no `error.path`, because
        // the program never got as far as a file to name. The table above
        // has it as one row among thirteen; here it stands alone, next to
        // its nearest neighbour, so that the promise fails under its own
        // name if it is ever broken.
        let input = Path::new("report.pdf");
        let unknown = RenderError::LibraryFolderUnknown {
            source: io::Error::other("the path of the program has no folder"),
        };

        let failure = render_failure_from(&unknown, input);

        assert_eq!(failure.code, ErrorCode::RendererUnavailable);
        // Not the library, which has no known place, and not the input
        // either: nothing is wrong with the file that was to be drawn.
        assert_eq!(failure.path, None);
        assert_eq!(failure.message, error_chain(&unknown));

        // The same code with the library looked for and not found: that
        // one does name a file.
        let missing = RenderError::LibraryMissing {
            path: PathBuf::from("app/pdfium.dll"),
        };
        let failure = render_failure_from(&missing, input);
        assert_eq!(failure.code, ErrorCode::RendererUnavailable);
        assert_eq!(failure.path.as_deref(), Some(Path::new("app/pdfium.dll")));
    }

    #[test]
    fn a_render_error_reads_as_the_other_operations_would_put_it() {
        // The same code, the same sentence and the same path for the same
        // trouble, whichever command met it: a caller needs no case of its
        // own for `render`.
        let input = Path::new("report.pdf");

        let drawn = render_failure_from(&RenderError::Encrypted, input);
        let counted = failure_from(
            &PdfError::Encrypted {
                input: WhichInput::Only,
            },
            &[input],
        );
        assert_eq!(drawn, counted);

        let drawn = render_failure_from(&RenderError::PageOutOfRange { page: 9, pages: 5 }, input);
        let extracted = failure_from(&PdfError::PageOutOfRange { page: 9, pages: 5 }, &[input]);
        assert_eq!(drawn, extracted);

        // The whole chain of causes, as for every other failure.
        let failed = RenderError::LibraryFailed {
            path: PathBuf::from("app/pdfium.dll"),
            source: "not a valid library".into(),
        };
        assert_eq!(
            render_failure_from(&failed, input).message,
            "cannot load the PDFium library app/pdfium.dll: not a valid library"
        );
    }

    #[test]
    fn an_input_number_outside_the_list_names_no_file() {
        // The core counts inputs from 1, so 0 is never a real input, and 3
        // is past the end of a list of two. Neither may panic or name a
        // wrong file.
        for number in [0, 3] {
            let error = PdfError::Encrypted {
                input: WhichInput::Numbered(number),
            };
            let failure = failure_from(&error, &two_inputs());

            assert_eq!(failure.code, ErrorCode::Encrypted);
            assert_eq!(failure.path, None, "input {number}");
        }
    }

    #[test]
    fn an_error_about_the_only_input_names_that_file_and_no_number() {
        // Every operation but `merge` reads one file. The code and the
        // path are what they are for a merge; only the words differ.
        let report = [Path::new("report.pdf")];

        let encrypted = PdfError::Encrypted {
            input: WhichInput::Only,
        };
        let failure = failure_from(&encrypted, &report);
        assert_eq!(failure.code, ErrorCode::Encrypted);
        assert_eq!(failure.path.as_deref(), Some(Path::new("report.pdf")));
        assert_eq!(
            failure.message,
            "the file is encrypted and needs a password"
        );

        let broken = PdfError::Broken {
            input: WhichInput::Only,
            reason: "it has no catalog",
        };
        let failure = failure_from(&broken, &report);
        assert_eq!(failure.code, ErrorCode::BrokenPdf);
        assert_eq!(failure.path.as_deref(), Some(Path::new("report.pdf")));
        assert_eq!(failure.message, "the file is damaged: it has no catalog");

        // Called with no list at all, there is no file to name. That must
        // not panic. The type of the empty list has to be written out,
        // because nothing in it tells the compiler what it would hold.
        let nothing: [&Path; 0] = [];
        assert_eq!(failure_from(&encrypted, &nothing).path, None);
    }

    #[test]
    fn error_codes_are_spelled_as_documented() {
        // These strings are the contract with every caller. Changing one
        // here means changing the help, the README and AGENTS.md, and
        // breaking the scripts that test for it.
        let codes = [
            (ErrorCode::OutputIsInput, "output_is_input"),
            (ErrorCode::OutputExists, "output_exists"),
            (ErrorCode::NoInputs, "no_inputs"),
            (ErrorCode::ReadFailed, "read_failed"),
            (ErrorCode::Encrypted, "encrypted"),
            (ErrorCode::BrokenPdf, "broken_pdf"),
            (ErrorCode::PageOutOfRange, "page_out_of_range"),
            (ErrorCode::PageRepeated, "page_repeated"),
            (ErrorCode::NoPagesLeft, "no_pages_left"),
            (ErrorCode::SaveFailed, "save_failed"),
            (ErrorCode::RendererUnavailable, "renderer_unavailable"),
            (ErrorCode::RenderFailed, "render_failed"),
        ];
        for (code, text) in codes {
            assert_eq!(code.as_str(), text);
        }
    }

    #[test]
    fn operation_names_are_spelled_as_documented() {
        // Part of the contract too: the command typed and the value of
        // "operation" in every JSON report.
        let names = [
            (Operation::Merge, "merge"),
            (Operation::Info, "info"),
            (Operation::Extract, "extract"),
            (Operation::Split, "split"),
            (Operation::Render, "render"),
            (Operation::Rotate, "rotate"),
            (Operation::Delete, "delete"),
        ];
        for (operation, name) in names {
            assert_eq!(operation.name(), name);
        }
    }

    #[test]
    fn the_first_output_that_is_an_input_is_refused_whatever_force_says() {
        // Nothing here is on disk, so paths are compared as given.
        let inputs = [Path::new("a.pdf"), Path::new("b.pdf")];
        let outputs = [Path::new("new.pdf"), Path::new("b.pdf"), Path::new("a.pdf")];

        for force in [false, true] {
            let failure = check_outputs(&inputs, &outputs, force).unwrap_err();

            assert_eq!(failure.code, ErrorCode::OutputIsInput, "force: {force}");
            assert_eq!(failure.path.as_deref(), Some(Path::new("b.pdf")));
            assert_eq!(failure.message, texts::output_is_input(Path::new("b.pdf")));
        }
    }

    #[test]
    fn outputs_that_are_free_and_not_inputs_are_accepted() {
        let inputs = [Path::new("a.pdf")];
        let outputs = [
            Path::new("danonpdf-no-such-folder/a-001.pdf"),
            Path::new("danonpdf-no-such-folder/a-002.pdf"),
        ];

        assert_eq!(check_outputs(&inputs, &outputs, false), Ok(()));
        // No output at all is nothing to refuse.
        let none: [&Path; 0] = [];
        assert_eq!(check_outputs(&inputs, &none, false), Ok(()));
    }
}
