//! Tests that run the real `danonpdf-cli` program, as a script or an AI
//! agent would: arguments in, then stdout, stderr and the exit code out.
//!
//! The unit tests inside the crate check each piece on its own. These check
//! the contract a caller relies on, which only exists once the pieces are
//! wired together in `main`.
//!
//! A file in `tests/` is an integration test: Cargo builds it as a separate
//! program that can only use the crate from the outside. For a binary crate
//! that means running the executable, whose path Cargo provides at compile
//! time in the `CARGO_BIN_EXE_<name>` environment variable.

use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{self, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::Duration;

use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions, dictionary};
use serde_json::{Value, json};

/// Runs `danonpdf-cli` with `args` and waits for it to end.
///
/// `&[&OsStr]` and not `&[&str]`: a path is not always valid text, and
/// `OsStr` is what the system really passes to a program.
fn cli(args: &[&OsStr]) -> Output {
    run(Command::new(env!("CARGO_BIN_EXE_danonpdf-cli")).args(args))
}

/// Starts `program` and waits for it to end. `cli` is the usual way in;
/// the tests that change the environment of the program build their own
/// `Command` and come here.
fn run(program: &mut Command) -> Output {
    program
        // The tool must never wait for input. With nothing to read, a
        // version that asked a question would fail here instead of hanging
        // the test run.
        .stdin(Stdio::null())
        .output()
        .expect("danonpdf-cli should start")
}

/// Starts `program`, an executable that this test copied a moment ago, and
/// waits for it to end.
///
/// On Linux a file cannot be started while any process holds it open for
/// writing: the start fails with "Text file busy" (ETXTBSY). The copy was
/// closed before `fs::copy` returned, but the tests of this file run as
/// threads of one process, and they start programs all the time. A child
/// that another thread started while the copy was being written holds, for
/// the moment between its `fork` and its `exec`, every file the test
/// process had open then, the half-written copy among them. That moment
/// cannot be seen or waited for from here, and it is short, so the start is
/// tried again.
///
/// Any other error, and this one once the tries are used up, fails the
/// test as it would in [`run`].
fn run_a_fresh_copy(program: &mut Command) -> Output {
    program.stdin(Stdio::null());
    let mut tries_left = 100;
    loop {
        match program.output() {
            // A guard on a `match` arm: the pattern has to match and the
            // test after `if` has to hold.
            Err(error) if error.kind() == io::ErrorKind::ExecutableFileBusy && tries_left > 0 => {
                tries_left -= 1;
                thread::sleep(Duration::from_millis(20));
            }
            result => return result.expect("the copy of danonpdf-cli should start"),
        }
    }
}

/// Runs `danonpdf-cli merge <flags> --output <output> <inputs>`.
fn merge(flags: &[&str], output: &Path, inputs: &[&Path]) -> Output {
    let mut args: Vec<&OsStr> = vec![OsStr::new("merge")];
    args.extend(flags.iter().map(OsStr::new));
    args.push(OsStr::new("--output"));
    args.push(output.as_os_str());
    args.extend(inputs.iter().map(|input| input.as_os_str()));
    cli(&args)
}

/// Runs `danonpdf-cli info <flags> <input>`.
fn info(flags: &[&str], input: &Path) -> Output {
    let mut args: Vec<&OsStr> = vec![OsStr::new("info")];
    args.extend(flags.iter().map(OsStr::new));
    args.push(input.as_os_str());
    cli(&args)
}

/// Runs `danonpdf-cli extract <flags> --pages <pages> --output <output>
/// <input>`.
fn extract(flags: &[&str], pages: &str, output: &Path, input: &Path) -> Output {
    let mut args: Vec<&OsStr> = vec![OsStr::new("extract")];
    args.extend(flags.iter().map(OsStr::new));
    args.push(OsStr::new("--pages"));
    args.push(OsStr::new(pages));
    args.push(OsStr::new("--output"));
    args.push(output.as_os_str());
    args.push(input.as_os_str());
    cli(&args)
}

/// Runs `danonpdf-cli split <flags> --output-dir <folder> <input>`. To
/// split every N pages, put `"--every", "N"` among the flags.
fn split(flags: &[&str], folder: &Path, input: &Path) -> Output {
    let mut args: Vec<&OsStr> = vec![OsStr::new("split")];
    args.extend(flags.iter().map(OsStr::new));
    args.push(OsStr::new("--output-dir"));
    args.push(folder.as_os_str());
    args.push(input.as_os_str());
    cli(&args)
}

/// The environment variable that names the folder of the PDFium library.
/// Spelled out here, not taken from the core: the name is documented, so a
/// change to it must fail a test.
const PDFIUM_DIR: &str = "DANONPDF_PDFIUM_DIR";

/// The arguments of `danonpdf-cli render <flags> --page <page> --output
/// <output> <input>`. To set the width, put `"--width", "N"` among the
/// flags.
fn render_args<'a>(
    flags: &'a [&str],
    page: &'a str,
    output: &'a Path,
    input: &'a Path,
) -> Vec<&'a OsStr> {
    let mut args: Vec<&OsStr> = vec![OsStr::new("render")];
    args.extend(flags.iter().map(OsStr::new));
    args.push(OsStr::new("--page"));
    args.push(OsStr::new(page));
    args.push(OsStr::new("--output"));
    args.push(output.as_os_str());
    args.push(input.as_os_str());
    args
}

/// Runs `danonpdf-cli render` with the PDFium library of the build image:
/// the test run has `DANONPDF_PDFIUM_DIR` set, and the program inherits
/// it.
fn render(flags: &[&str], page: &str, output: &Path, input: &Path) -> Output {
    cli(&render_args(flags, page, output, input))
}

/// Runs `danonpdf-cli render` told to look for the PDFium library in
/// `library_folder`. Every run is a process of its own, so each one can be
/// given another folder: inside one process the library loads once.
fn render_with_library(
    library_folder: &Path,
    flags: &[&str],
    page: &str,
    output: &Path,
    input: &Path,
) -> Output {
    run(Command::new(env!("CARGO_BIN_EXE_danonpdf-cli"))
        .args(render_args(flags, page, output, input))
        .env(PDFIUM_DIR, library_folder))
}

/// What the program wrote to stdout, as text.
fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout should be UTF-8")
}

/// What the program wrote to stderr, as text.
fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr should be UTF-8")
}

/// The one JSON object the program printed.
///
/// Fails the test unless stdout is exactly one line ending in a newline and
/// stderr is empty: with `--json`, a caller parses stdout whole.
fn json_report(output: &Output) -> Value {
    let text = stdout(output);
    assert!(text.ends_with('\n'), "stdout must end with a newline");
    assert_eq!(text.matches('\n').count(), 1, "stdout must be one line");
    assert_eq!(stderr(output), "", "stderr must stay empty with --json");
    serde_json::from_str(&text).expect("stdout should be valid JSON")
}

/// `path` as the text the JSON report is expected to carry.
fn text(path: &Path) -> &str {
    path.to_str().expect("test paths are valid UTF-8")
}

/// A folder under the system temp folder, deleted again when the value is
/// dropped, so a failing test does not leave files behind either.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(test_name: &str) -> Self {
        // The process id keeps two test runs at the same time apart; the
        // test name keeps apart the tests of one run, which share a process.
        let name = format!("danonpdf-cli-{}-{test_name}", std::process::id());
        let path = std::env::temp_dir().join(name);
        // Left over from an earlier run that was killed; no error if not.
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Saves a PDF with `page_count` pages into this folder as `name`.
    fn pdf(&self, name: &str, page_count: i64) -> PathBuf {
        self.save(name, with_pages(page_count))
    }

    /// Saves `document` into this folder as `name` and returns its path.
    fn save(&self, name: &str, mut document: Document) -> PathBuf {
        let path = self.file(name);
        document.save(&path).unwrap();
        path
    }

    /// The names of everything in this folder, sorted, so a test can check
    /// that a refused merge wrote nothing at all.
    fn entries(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// A document with `page_count` empty pages. Page `n` is `500 + n` points
/// wide, so a test can tell which pages ended up where (see
/// [`page_widths`]).
fn with_pages(page_count: i64) -> Document {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let kids: Vec<Object> = (1..=page_count)
        .map(|number| {
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), (500 + number).into(), 842.into()],
            });
            Object::Reference(page_id)
        })
        .collect();
    let pages = dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => page_count };
    document.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document
}

/// Encrypts `document` with RC4 128 bits and the given user password.
fn encrypted(mut document: Document, user_password: &str) -> Document {
    // Encryption keys are derived from the file identifier, so set one.
    let id = Object::string_literal("0123456789abcdef");
    document.trailer.set("ID", vec![id.clone(), id]);
    let version = EncryptionVersion::V2 {
        document: &document,
        owner_password: "owner",
        user_password,
        key_length: 128,
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).unwrap();
    document.encrypt(&state).unwrap();
    document
}

/// The bytes of a PDF without its end: everything before the table that
/// says where each object is (the cross-reference data), so that the
/// table, the trailer and the pointer to the table are all gone.
///
/// The cut is read from the file itself and not counted in bytes from the
/// end. A PDF ends with the keyword `startxref`, the position of the table
/// as a number, and `%%EOF`; that number is where the cut goes. A fixture
/// with one more page, or a version of lopdf that writes the table in
/// another form, moves the table, and the test goes on cutting exactly
/// this.
fn without_its_end(bytes: &[u8]) -> &[u8] {
    const KEYWORD: &[u8] = b"startxref";
    // `windows` slides over the bytes, `KEYWORD.len()` at a time, and
    // `rposition` searches from the end: where the last keyword starts.
    let keyword = bytes
        .windows(KEYWORD.len())
        .rposition(|window| window == KEYWORD)
        .expect("the fixture ends with startxref");
    // The number is plain text: it is read as text and parsed.
    let after = String::from_utf8_lossy(&bytes[keyword + KEYWORD.len()..]);
    let table: usize = after
        .split_whitespace()
        .next()
        .and_then(|number| number.parse().ok())
        .expect("a position follows startxref");
    assert!(table < keyword, "the table should come before its pointer");
    &bytes[..table]
}

/// A document with one empty page, `width` by `height` points.
fn with_one_page(width: i64, height: i64) -> Document {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), width.into(), height.into()],
    });
    let pages = dictionary! {
        "Type" => "Pages",
        "Kids" => vec![Object::Reference(page_id)],
        "Count" => 1,
    };
    document.objects.insert(pages_id, Object::Dictionary(pages));
    let catalog_id = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    document.trailer.set("Root", catalog_id);
    document
}

/// Reads the PNG file at `path`: its width, its height and its pixels, four
/// bytes each (red, green, blue, alpha). Fails the test unless the file is
/// a PNG with 8 bits per channel and alpha, which is what `render` writes.
fn decode_png(path: &Path) -> (u32, u32, Vec<u8>) {
    let bytes = fs::read(path).unwrap();
    // Every PNG file starts with these eight bytes.
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG file");

    let decoder = png::Decoder::new(BufReader::new(File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(frame.color_type, png::ColorType::Rgba);
    assert_eq!(frame.bit_depth, png::BitDepth::Eight);
    pixels.truncate(frame.buffer_size());
    (frame.width, frame.height, pixels)
}

/// How many pages the PDF at `path` has.
fn page_count(path: &Path) -> usize {
    Document::load(path).unwrap().get_pages().len()
}

/// The width of every page of the PDF at `path`, in page order. For a
/// file made from [`with_pages`], `[503, 501]` means: page 3 of the input,
/// then page 1.
fn page_widths(path: &Path) -> Vec<i64> {
    let document = Document::load(path).unwrap();
    document
        .page_iter()
        .map(|page_id| {
            let page = document.get_dictionary(page_id).unwrap();
            let media_box = page.get(b"MediaBox").unwrap().as_array().unwrap();
            media_box[2].as_i64().unwrap()
        })
        .collect()
}

/// The names of everything in `folder`, sorted.
fn entries_of(folder: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

// Success

#[test]
fn merge_prints_one_human_line_and_writes_the_output() {
    let scratch = ScratchDir::new("human-success");
    let a = scratch.pdf("a.pdf", 2);
    let b = scratch.pdf("b.pdf", 1);
    let out = scratch.file("out.pdf");

    let result = merge(&[], &out, &[&a, &b]);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        stdout(&result),
        format!("Merged 2 files (3 pages) into {}\n", out.display())
    );
    assert_eq!(stderr(&result), "");
    assert_eq!(page_count(&out), 3);
}

#[test]
fn merge_with_json_prints_exactly_one_object() {
    let scratch = ScratchDir::new("json-success");
    let a = scratch.pdf("a.pdf", 2);
    let b = scratch.pdf("b.pdf", 1);
    let c = scratch.pdf("c.pdf", 4);
    let out = scratch.file("out.pdf");

    let result = merge(&["--json"], &out, &[&a, &b, &c]);

    assert_eq!(result.status.code(), Some(0));
    // Comparing the whole value, not field by field, also proves that the
    // object has no other field.
    assert_eq!(
        json_report(&result),
        json!({
            "ok": true,
            "operation": "merge",
            "output": text(&out),
            "inputs": 3,
            "pages": 7,
        })
    );
    assert_eq!(page_count(&out), 7);
}

#[test]
fn flags_may_come_after_the_inputs() {
    let scratch = ScratchDir::new("flags-last");
    let a = scratch.pdf("a.pdf", 1);
    let b = scratch.pdf("b.pdf", 1);
    let out = scratch.file("out.pdf");

    let result = cli(&[
        OsStr::new("merge"),
        a.as_os_str(),
        b.as_os_str(),
        OsStr::new("--output"),
        out.as_os_str(),
        OsStr::new("--json"),
    ]);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(json_report(&result)["pages"], 2);
}

#[test]
fn inputs_may_stand_on_both_sides_of_a_flag() {
    let scratch = ScratchDir::new("flags-between");
    let a = scratch.pdf("a.pdf", 1);
    let b = scratch.pdf("b.pdf", 2);
    let out = scratch.file("out.pdf");

    let result = cli(&[
        OsStr::new("merge"),
        a.as_os_str(),
        OsStr::new("--json"),
        OsStr::new("--output"),
        out.as_os_str(),
        b.as_os_str(),
    ]);

    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    let report = json_report(&result);
    assert_eq!(report["inputs"], 2);
    assert_eq!(report["pages"], 3);
}

#[test]
fn inputs_and_output_are_used_exactly_as_given() {
    // Unlike the window, the tool does not filter or fix anything: an input
    // without ".pdf" is read, the same input twice is merged twice, and the
    // output gets no extension added.
    let scratch = ScratchDir::new("as-given");
    let scan = scratch.pdf("scan.dat", 2);
    let out = scratch.file("out");

    let result = merge(&["--json"], &out, &[&scan, &scan]);

    assert_eq!(result.status.code(), Some(0));
    let report = json_report(&result);
    assert_eq!(report["inputs"], 2);
    assert_eq!(report["pages"], 4);
    assert_eq!(report["output"], text(&out));
    assert_eq!(scratch.entries(), ["out", "scan.dat"]);
    assert_eq!(page_count(&out), 4);
}

// Refusals: nothing is written

/// Checks the failure report of a merge and returns its `error` object.
fn error_of(result: &Output, code: &str) -> Value {
    failure_of(result, "merge", code)
}

/// Checks the failure report of `operation` and returns its `error` object.
fn failure_of(result: &Output, operation: &str, code: &str) -> Value {
    assert_eq!(result.status.code(), Some(1));
    let report = json_report(result);
    assert_eq!(report["ok"], false);
    assert_eq!(report["operation"], operation);
    assert_eq!(report["error"]["code"], code);
    let message = report["error"]["message"].as_str().unwrap_or_default();
    assert!(!message.is_empty(), "the error needs a message");
    report["error"].clone()
}

#[test]
fn output_that_is_an_input_is_refused_even_with_force() {
    let scratch = ScratchDir::new("output-is-input");
    let a = scratch.pdf("a.pdf", 2);
    let b = scratch.pdf("b.pdf", 1);
    let before = fs::read(&a).unwrap();
    // The same file as the first input, spelled differently.
    let respelled = scratch.0.join(".").join("a.pdf");

    // `a` exists, so this also proves the order of the checks: the answer
    // is "output_is_input", not "output_exists".
    for flags in [&["--json"][..], &["--json", "--force"][..]] {
        let result = merge(flags, &respelled, &[&a, &b]);

        let error = error_of(&result, "output_is_input");
        assert_eq!(error["path"], text(&respelled));
        assert_eq!(fs::read(&a).unwrap(), before, "the input was changed");
        assert_eq!(scratch.entries(), ["a.pdf", "b.pdf"]);
    }
}

#[test]
fn existing_output_is_refused_without_force() {
    let scratch = ScratchDir::new("output-exists");
    let a = scratch.pdf("a.pdf", 2);
    let b = scratch.pdf("b.pdf", 1);
    let out = scratch.file("out.pdf");
    fs::write(&out, b"someone else's file").unwrap();
    let inputs_before = (fs::read(&a).unwrap(), fs::read(&b).unwrap());

    let result = merge(&["--json"], &out, &[&a, &b]);

    let error = error_of(&result, "output_exists");
    assert_eq!(error["path"], text(&out));
    assert_eq!(fs::read(&out).unwrap(), b"someone else's file");
    assert_eq!(
        (fs::read(&a).unwrap(), fs::read(&b).unwrap()),
        inputs_before
    );
    assert_eq!(scratch.entries(), ["a.pdf", "b.pdf", "out.pdf"]);
}

#[test]
fn force_replaces_an_existing_output() {
    let scratch = ScratchDir::new("force");
    let a = scratch.pdf("a.pdf", 2);
    let b = scratch.pdf("b.pdf", 1);
    let out = scratch.file("out.pdf");
    fs::write(&out, b"an old file").unwrap();

    let result = merge(&["--json", "--force"], &out, &[&a, &b]);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(json_report(&result)["pages"], 3);
    assert_eq!(page_count(&out), 3);
}

// Failures reported by the core

#[test]
fn missing_input_is_read_failed_with_its_path() {
    let scratch = ScratchDir::new("missing-input");
    let a = scratch.pdf("a.pdf", 1);
    let missing = scratch.file("missing.pdf");
    let out = scratch.file("out.pdf");

    let result = merge(&["--json"], &out, &[&a, &missing]);

    let error = error_of(&result, "read_failed");
    assert_eq!(error["path"], text(&missing));
    // The message carries the whole chain of causes, not only the first.
    let message = error["message"].as_str().unwrap();
    assert!(
        message.starts_with(&format!("cannot read {} as a PDF: ", missing.display())),
        "unexpected message: {message}"
    );
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

#[test]
fn encrypted_input_is_reported_with_its_path() {
    // The core only says "input 2"; the tool knows the list, so it can name
    // the file for a caller that reads `path`. The number stays in the
    // message: with several inputs it is what tells them apart.
    let scratch = ScratchDir::new("encrypted");
    let plain = scratch.pdf("plain.pdf", 1);
    let locked = scratch.file("locked.pdf");
    encrypted(with_pages(1), "secret").save(&locked).unwrap();
    let out = scratch.file("out.pdf");

    let result = merge(&["--json"], &out, &[&plain, &locked]);

    let error = error_of(&result, "encrypted");
    assert_eq!(error["path"], text(&locked));
    assert_eq!(
        error["message"],
        "input 2 is encrypted and needs a password"
    );
    assert!(!out.exists(), "nothing should be written");
}

/// What `info`, `extract` and `split` say about a file that needs a
/// password. They read one file, so there is no "input 1" to speak of:
/// that wording belongs to `merge`, which takes a list.
const THE_FILE_IS_ENCRYPTED: &str = "the file is encrypted and needs a password";

#[test]
fn output_in_a_missing_folder_is_save_failed() {
    let scratch = ScratchDir::new("missing-folder");
    let a = scratch.pdf("a.pdf", 1);
    let b = scratch.pdf("b.pdf", 1);
    let out = scratch.file("missing").join("out.pdf");

    let result = merge(&["--json"], &out, &[&a, &b]);

    let error = error_of(&result, "save_failed");
    assert_eq!(error["path"], text(&out));
    assert_eq!(scratch.entries(), ["a.pdf", "b.pdf"]);
}

#[test]
fn without_json_a_failure_goes_to_stderr_only() {
    let scratch = ScratchDir::new("human-failure");
    let a = scratch.pdf("a.pdf", 1);
    let b = scratch.pdf("b.pdf", 1);
    let out = scratch.file("out.pdf");
    fs::write(&out, b"taken").unwrap();

    let result = merge(&[], &out, &[&a, &b]);

    assert_eq!(result.status.code(), Some(1));
    assert_eq!(stdout(&result), "");
    let message = stderr(&result);
    assert!(message.starts_with("error: "), "unexpected: {message}");
    assert!(message.contains("--force"), "unexpected: {message}");
    assert_eq!(message.matches('\n').count(), 1, "one line: {message}");
}

// Usage errors: reported by the argument parser

#[test]
fn usage_errors_exit_with_2_and_explain_on_stderr() {
    let scratch = ScratchDir::new("usage");
    let a = scratch.pdf("a.pdf", 1);
    let out = scratch.file("out.pdf");

    // One input is not a merge.
    let one_input = merge(&[], &out, &[&a]);
    // The same mistake with `--json`: the parser stops before the tool knows
    // which format was wanted, so the answer is still plain text on stderr.
    let one_input_json = merge(&["--json"], &out, &[&a]);
    let no_output = cli(&[OsStr::new("merge"), a.as_os_str(), a.as_os_str()]);
    let unknown_flag = merge(&["--overwrite"], &out, &[&a, &a]);
    let no_command = cli(&[]);

    for result in [
        one_input,
        one_input_json,
        no_output,
        unknown_flag,
        no_command,
    ] {
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(stdout(&result), "");
        assert!(!stderr(&result).is_empty(), "stderr should explain");
    }
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

// Help and version

#[test]
fn version_prints_the_name_and_the_version() {
    let result = cli(&[OsStr::new("--version")]);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        stdout(&result),
        format!("danonpdf-cli {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert_eq!(stderr(&result), "");
}

#[test]
fn help_teaches_the_whole_contract() {
    // Every way to ask; an agent that has never seen the tool may try any.
    let asks: [&[&str]; 11] = [
        &["--help"],
        &["help", "merge"],
        &["merge", "--help"],
        &["help", "info"],
        &["info", "--help"],
        &["help", "extract"],
        &["extract", "--help"],
        &["help", "split"],
        &["split", "--help"],
        &["help", "render"],
        &["render", "--help"],
    ];
    for ask in asks {
        let args: Vec<&OsStr> = ask.iter().map(OsStr::new).collect();
        let result = cli(&args);

        assert_eq!(result.status.code(), Some(0), "{ask:?}");
        assert_eq!(stderr(&result), "", "{ask:?}");
        let help = stdout(&result);
        for expected in [
            // An example to copy, for every command.
            "danonpdf-cli merge --json --output merged.pdf first.pdf second.pdf",
            "danonpdf-cli info --json report.pdf",
            "danonpdf-cli extract --json --pages 1-3,7,10- --output part.pdf report.pdf",
            "danonpdf-cli split --json --every 10 --output-dir parts report.pdf",
            "danonpdf-cli render --json --page 1 --output page.png report.pdf",
            // What render draws: the limits of the image, the name its
            // output must have, and that the report holds the real size.
            "from 1 to 4096",
            "the image is 1024 pixels wide",
            "at most 8192 pixels high",
            "--output must end in .png",
            r#"read "width" and "height" there"#,
            // Where render looks for the PDFium library, and who needs it.
            "pdfium.dll on Windows",
            "libpdfium.so on Linux",
            "DANONPDF_PDFIUM_DIR",
            "merge, info, extract and split do not need it",
            // The page syntax, and which mistakes end with which exit code.
            "A-B  pages A to B, both included; A must not be above B",
            "A-   page A and every page after it",
            "a backwards range such as 5-3, a page listed twice) is a usage error",
            "exit code 1, error code page_out_of_range",
            // How split names its files, and that it writes all or nothing.
            "<name of the input without its extension>-<NNN>.pdf",
            "--force was not given, nothing is written at all",
            // The exit codes, and what happens to a report nobody can read.
            "Exit codes:",
            "If the report cannot be written",
            // The one code this command line can never produce says so.
            "no_inputs          there was nothing to merge; never reported by this",
            // Every JSON shape.
            r#"{"ok":true,"operation":"merge","output":"merged.pdf","inputs":2,"pages":12}"#,
            r#"{"ok":true,"operation":"info","input":"report.pdf","pages":12}"#,
            r#"{"ok":true,"operation":"extract","output":"part.pdf","pages":7}"#,
            r#"{"ok":true,"operation":"split","outputs":["parts/report-001.pdf","parts/report-002.pdf"],"files":2,"pages":12}"#,
            r#"{"ok":true,"operation":"render","output":"page.png","page":1,"width":1024,"height":1449}"#,
            r#"{"ok":false,"operation":"merge","error":{"code":"output_exists","#,
            // The extra key of a split that stopped midway.
            r#"adds "written" to"#,
            // Every error code.
            "output_is_input",
            "output_exists",
            "no_inputs",
            "read_failed",
            "encrypted",
            "broken_pdf",
            "page_out_of_range",
            "save_failed",
            "renderer_unavailable",
            "render_failed",
        ] {
            assert!(help.contains(expected), "{ask:?} lacks {expected:?}");
        }
    }
}

#[test]
fn the_examples_in_the_help_tell_the_truth() {
    // The help shows what `extract --pages 1-3,7,10-` and `split --every
    // 10` print for a 12-page report. Run both on one and compare the
    // numbers, so the examples cannot drift from what the tool does.
    let scratch = ScratchDir::new("help-examples");
    let report = scratch.pdf("report.pdf", 12);

    let extracted = extract(&["--json"], "1-3,7,10-", &scratch.file("part.pdf"), &report);
    let parts = split(
        &["--json", "--every", "10"],
        &scratch.file("parts"),
        &report,
    );

    assert_eq!(json_report(&info(&["--json"], &report))["pages"], 12);
    assert_eq!(json_report(&extracted)["pages"], 7);
    let parts = json_report(&parts);
    assert_eq!(parts["files"], 2);
    assert_eq!(parts["pages"], 12);
    assert_eq!(
        entries_of(&scratch.file("parts")),
        ["report-001.pdf", "report-002.pdf"]
    );

    // The help also shows what `render --page 1` prints, and says that an
    // A4 page drawn at the default width is 1024 by 1449 pixels.
    let a4 = scratch.save("a4.pdf", with_one_page(595, 842));
    let page = scratch.file("page.png");
    let rendered = json_report(&render(&["--json"], "1", &page, &a4));
    assert_eq!(rendered["page"], 1);
    assert_eq!(rendered["width"], 1024);
    assert_eq!(rendered["height"], 1449);
}

// info

#[test]
fn info_prints_one_human_line() {
    let scratch = ScratchDir::new("info-human");
    let input = scratch.pdf("a.pdf", 3);

    let result = info(&[], &input);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        stdout(&result),
        format!("{} has 3 pages\n", input.display())
    );
    assert_eq!(stderr(&result), "");
}

#[test]
fn info_with_json_prints_exactly_one_object() {
    let scratch = ScratchDir::new("info-json");
    let input = scratch.pdf("a.pdf", 12);

    let result = info(&["--json"], &input);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        json_report(&result),
        json!({
            "ok": true,
            "operation": "info",
            "input": text(&input),
            "pages": 12,
        })
    );
    // The order of the keys is part of the contract too, and a parsed
    // value has lost it: `ok` and `operation` come first.
    assert!(
        stdout(&result).starts_with(r#"{"ok":true,"operation":"info","input":""#),
        "unexpected: {}",
        stdout(&result)
    );
    // Nothing was written next to the input.
    assert_eq!(scratch.entries(), ["a.pdf"]);
}

#[test]
fn info_reports_what_it_cannot_read_with_its_path() {
    let scratch = ScratchDir::new("info-failures");
    let missing = scratch.file("missing.pdf");
    let locked = scratch.file("locked.pdf");
    encrypted(with_pages(1), "secret").save(&locked).unwrap();

    let error = failure_of(&info(&["--json"], &missing), "info", "read_failed");
    assert_eq!(error["path"], text(&missing));
    let error = failure_of(&info(&["--json"], &locked), "info", "encrypted");
    assert_eq!(error["path"], text(&locked));
    assert_eq!(error["message"], THE_FILE_IS_ENCRYPTED);
}

// extract

#[test]
fn extract_writes_the_chosen_pages_in_the_order_given() {
    let scratch = ScratchDir::new("extract-success");
    let input = scratch.pdf("in.pdf", 5);
    let out = scratch.file("out.pdf");

    let result = extract(&["--json"], "3,1", &out, &input);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        json_report(&result),
        json!({
            "ok": true,
            "operation": "extract",
            "output": text(&out),
            "pages": 2,
        })
    );
    assert!(
        stdout(&result).starts_with(r#"{"ok":true,"operation":"extract","output":""#),
        "unexpected: {}",
        stdout(&result)
    );
    // Page 3 of the input, then page 1.
    assert_eq!(page_widths(&out), [503, 501]);
    // What `info` says about the result agrees with the report.
    assert_eq!(json_report(&info(&["--json"], &out))["pages"], 2);
    // The input is only read.
    assert_eq!(page_count(&input), 5);
}

#[test]
fn extract_understands_ranges_and_spaces() {
    let scratch = ScratchDir::new("extract-ranges");
    let input = scratch.pdf("in.pdf", 6);
    let out = scratch.file("out.pdf");

    let result = extract(&[], " 5- , 1-2 ", &out, &input);

    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert_eq!(
        stdout(&result),
        format!("Extracted 4 pages into {}\n", out.display())
    );
    assert_eq!(stderr(&result), "");
    assert_eq!(page_widths(&out), [505, 506, 501, 502]);
}

#[test]
fn extract_never_writes_over_its_input() {
    let scratch = ScratchDir::new("extract-output-is-input");
    let input = scratch.pdf("in.pdf", 3);
    let before = fs::read(&input).unwrap();
    // The input itself, spelled differently.
    let respelled = scratch.0.join(".").join("in.pdf");

    for flags in [&["--json"][..], &["--json", "--force"][..]] {
        let result = extract(flags, "1", &respelled, &input);

        let error = failure_of(&result, "extract", "output_is_input");
        assert_eq!(error["path"], text(&respelled));
        assert_eq!(fs::read(&input).unwrap(), before, "the input was changed");
        assert_eq!(scratch.entries(), ["in.pdf"]);
    }
}

#[test]
fn extract_replaces_an_existing_output_only_with_force() {
    let scratch = ScratchDir::new("extract-output-exists");
    let input = scratch.pdf("in.pdf", 3);
    let out = scratch.file("out.pdf");
    fs::write(&out, b"someone else's file").unwrap();

    let refused = extract(&["--json"], "2", &out, &input);

    let error = failure_of(&refused, "extract", "output_exists");
    assert_eq!(error["path"], text(&out));
    assert_eq!(fs::read(&out).unwrap(), b"someone else's file");

    let forced = extract(&["--json", "--force"], "2", &out, &input);

    assert_eq!(forced.status.code(), Some(0));
    assert_eq!(page_widths(&out), [502]);
}

#[test]
fn a_page_past_the_end_is_page_out_of_range_and_writes_nothing() {
    let scratch = ScratchDir::new("extract-past-end");
    let input = scratch.pdf("in.pdf", 5);
    let out = scratch.file("out.pdf");

    let result = extract(&["--json"], "2,9", &out, &input);

    let error = failure_of(&result, "extract", "page_out_of_range");
    // The failure is about the pages asked for, not about a file.
    assert_eq!(error.get("path"), None);
    // The wording belongs to the core and may change, so only what a
    // reader needs from it is pinned: the page that was asked for and how
    // many pages the document has.
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("page 9") && message.contains("5 pages"),
        "unexpected message: {message}"
    );
    assert_eq!(scratch.entries(), ["in.pdf"]);
}

#[test]
fn pages_that_cannot_be_read_are_a_usage_error_also_with_json() {
    let scratch = ScratchDir::new("extract-bad-pages");
    let input = scratch.pdf("in.pdf", 5);
    let out = scratch.file("out.pdf");

    // One text for each rule of the syntax.
    for pages in ["", "1,,3", "abc", "0", "5-3", "1-5,3"] {
        for flags in [&[][..], &["--json"][..]] {
            let result = extract(flags, pages, &out, &input);

            assert_eq!(result.status.code(), Some(2), "{pages:?} {flags:?}");
            assert_eq!(stdout(&result), "", "{pages:?} {flags:?}");
            let message = stderr(&result);
            assert!(
                message.starts_with("error: ") && message.contains("--pages <PAGES>"),
                "{pages:?} {flags:?} gave: {message}"
            );
        }
    }
    assert_eq!(scratch.entries(), ["in.pdf"]);
}

#[test]
fn extract_reports_an_encrypted_input_with_its_path() {
    let scratch = ScratchDir::new("extract-encrypted");
    let locked = scratch.file("locked.pdf");
    encrypted(with_pages(2), "secret").save(&locked).unwrap();
    let out = scratch.file("out.pdf");

    let result = extract(&["--json"], "1", &out, &locked);

    let error = failure_of(&result, "extract", "encrypted");
    assert_eq!(error["path"], text(&locked));
    assert_eq!(error["message"], THE_FILE_IS_ENCRYPTED);
    assert!(!out.exists(), "nothing should be written");
}

// split

#[test]
fn split_writes_one_file_per_page_by_default() {
    let scratch = ScratchDir::new("split-default");
    let input = scratch.pdf("doc.pdf", 3);
    let folder = scratch.file("parts");
    let names = ["doc-001.pdf", "doc-002.pdf", "doc-003.pdf"];
    let outputs = names.map(|name| folder.join(name));

    let result = split(&["--json"], &folder, &input);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(
        json_report(&result),
        json!({
            "ok": true,
            "operation": "split",
            "outputs": outputs.each_ref().map(|output| text(output)),
            "files": 3,
            "pages": 3,
        })
    );
    // The order of the keys, which a parsed value has lost.
    let line = stdout(&result);
    let keys = [
        r#""ok":"#,
        r#""operation":"#,
        r#""outputs":"#,
        r#""files":"#,
        r#""pages":"#,
    ];
    let places: Vec<usize> = keys.iter().map(|key| line.find(key).unwrap()).collect();
    assert!(places.is_sorted(), "unexpected order: {line}");

    assert_eq!(entries_of(&folder), names);
    let widths: Vec<Vec<i64>> = outputs.iter().map(|output| page_widths(output)).collect();
    assert_eq!(widths, [[501], [502], [503]]);
    // The input is only read.
    assert_eq!(page_count(&input), 3);
}

#[test]
fn split_every_n_pages_leaves_a_shorter_last_file() {
    let scratch = ScratchDir::new("split-every");
    let input = scratch.pdf("doc.pdf", 5);
    let folder = scratch.file("parts");

    let result = split(&["--every", "2"], &folder, &input);

    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    assert_eq!(
        stdout(&result),
        format!(
            "Split 5 pages into 3 files: {} to {}\n",
            folder.join("doc-001.pdf").display(),
            folder.join("doc-003.pdf").display()
        )
    );
    assert_eq!(stderr(&result), "");
    assert_eq!(page_widths(&folder.join("doc-001.pdf")), [501, 502]);
    assert_eq!(page_widths(&folder.join("doc-002.pdf")), [503, 504]);
    assert_eq!(page_widths(&folder.join("doc-003.pdf")), [505]);
}

#[test]
fn split_every_as_many_pages_as_there_are_or_more_writes_one_file() {
    // "5" is exactly the page count and "8" is past it: both mean one file
    // with every page, not an error and not an empty second file.
    for every in ["5", "8"] {
        let scratch = ScratchDir::new(&format!("split-every-{every}-of-5"));
        let input = scratch.pdf("doc.pdf", 5);
        let folder = scratch.file("parts");
        let only = folder.join("doc-001.pdf");

        let result = split(&["--json", "--every", every], &folder, &input);

        assert_eq!(result.status.code(), Some(0), "--every {every}");
        assert_eq!(
            json_report(&result),
            json!({
                "ok": true,
                "operation": "split",
                "outputs": [text(&only)],
                "files": 1,
                "pages": 5,
            }),
            "--every {every}"
        );
        assert_eq!(entries_of(&folder), ["doc-001.pdf"], "--every {every}");
        assert_eq!(page_widths(&only), [501, 502, 503, 504, 505]);
    }
}

#[test]
fn split_creates_the_output_folder_and_what_is_missing_above_it() {
    let scratch = ScratchDir::new("split-new-folder");
    let input = scratch.pdf("doc.pdf", 2);
    let folder = scratch.file("new").join("deeper");

    let result = split(&["--json"], &folder, &input);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(entries_of(&folder), ["doc-001.pdf", "doc-002.pdf"]);
}

#[test]
fn split_writes_nothing_when_one_of_its_names_is_taken() {
    let scratch = ScratchDir::new("split-name-taken");
    let input = scratch.pdf("doc.pdf", 3);
    let folder = scratch.file("parts");
    fs::create_dir(&folder).unwrap();
    // The second and the third name are in use; the first one is free.
    fs::write(folder.join("doc-002.pdf"), b"someone else's file").unwrap();
    fs::write(folder.join("doc-003.pdf"), b"another one").unwrap();

    let result = split(&["--json"], &folder, &input);

    // The first name that is taken is the one reported.
    let error = failure_of(&result, "split", "output_exists");
    assert_eq!(error["path"], text(&folder.join("doc-002.pdf")));
    assert_eq!(error.get("written"), None);
    // Not even the file whose name was free: all or nothing.
    assert_eq!(entries_of(&folder), ["doc-002.pdf", "doc-003.pdf"]);
    assert_eq!(
        fs::read(folder.join("doc-002.pdf")).unwrap(),
        b"someone else's file"
    );
}

#[test]
fn split_with_force_replaces_the_files_that_are_there() {
    let scratch = ScratchDir::new("split-force");
    let input = scratch.pdf("doc.pdf", 2);
    let folder = scratch.file("parts");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("doc-002.pdf"), b"an old file").unwrap();

    let result = split(&["--json", "--force"], &folder, &input);

    assert_eq!(result.status.code(), Some(0));
    assert_eq!(json_report(&result)["files"], 2);
    assert_eq!(page_widths(&folder.join("doc-002.pdf")), [502]);
}

#[test]
fn a_split_that_fails_before_planning_does_not_create_the_folder() {
    let scratch = ScratchDir::new("split-missing-input");
    let missing = scratch.file("missing.pdf");
    let folder = scratch.file("parts");

    let result = split(&["--json"], &folder, &missing);

    let error = failure_of(&result, "split", "read_failed");
    assert_eq!(error["path"], text(&missing));
    assert_eq!(scratch.entries(), Vec::<String>::new());
}

// Links only exist in this form on Unix-like systems; creating one on
// Windows needs a different function and, often, special rights.
#[cfg(unix)]
#[test]
fn split_never_writes_over_its_input() {
    // The names of a split never spell the input, so the only way for one
    // of them to be the input is a link. With `--force` and without this
    // rule, the tool would be asked to replace what it is reading.
    let scratch = ScratchDir::new("split-output-is-input");
    let input = scratch.pdf("doc.pdf", 2);
    let before = fs::read(&input).unwrap();
    let folder = scratch.file("parts");
    fs::create_dir(&folder).unwrap();
    let link = folder.join("doc-002.pdf");
    std::os::unix::fs::symlink(&input, &link).unwrap();

    for flags in [&["--json"][..], &["--json", "--force"][..]] {
        let result = split(flags, &folder, &input);

        let error = failure_of(&result, "split", "output_is_input");
        assert_eq!(error["path"], text(&link));
        assert_eq!(fs::read(&input).unwrap(), before, "the input was changed");
        assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
        assert_eq!(entries_of(&folder), ["doc-002.pdf"]);
    }
}

#[test]
fn split_reports_an_encrypted_input_with_its_path() {
    let scratch = ScratchDir::new("split-encrypted");
    let locked = scratch.file("locked.pdf");
    encrypted(with_pages(2), "secret").save(&locked).unwrap();
    let folder = scratch.file("parts");

    let result = split(&["--json"], &folder, &locked);

    let error = failure_of(&result, "split", "encrypted");
    assert_eq!(error["path"], text(&locked));
    assert_eq!(error["message"], THE_FILE_IS_ENCRYPTED);
    assert!(!folder.exists(), "nothing should be written");
}

#[test]
fn a_split_that_stops_midway_lists_the_files_it_wrote_and_keeps_them() {
    let scratch = ScratchDir::new("split-midway");
    let input = scratch.pdf("doc.pdf", 3);
    let folder = scratch.file("parts");
    // A folder holds the name of the second file. `--force` lets the split
    // start anyway, but a file cannot take the place of a folder, so the
    // second save fails after the first one worked.
    fs::create_dir_all(folder.join("doc-002.pdf")).unwrap();

    let result = split(&["--json", "--force"], &folder, &input);

    let error = failure_of(&result, "split", "save_failed");
    assert_eq!(error["path"], text(&folder.join("doc-002.pdf")));
    assert_eq!(error["written"], json!([text(&folder.join("doc-001.pdf"))]));
    let message = error["message"].as_str().unwrap();
    assert!(
        message.starts_with("the split stopped after 1 of 3 files: cannot save "),
        "unexpected message: {message}"
    );
    // The first file is complete and still there; the third was not tried.
    assert_eq!(page_widths(&folder.join("doc-001.pdf")), [501]);
    assert_eq!(entries_of(&folder), ["doc-001.pdf", "doc-002.pdf"]);
}

#[test]
fn without_json_a_split_that_stops_midway_says_so_on_stderr() {
    let scratch = ScratchDir::new("split-midway-human");
    let input = scratch.pdf("doc.pdf", 2);
    let folder = scratch.file("parts");
    fs::create_dir_all(folder.join("doc-002.pdf")).unwrap();

    let result = split(&["--force"], &folder, &input);

    assert_eq!(result.status.code(), Some(1));
    assert_eq!(stdout(&result), "");
    let message = stderr(&result);
    assert!(
        message.starts_with("error: the split stopped after 1 of 2 files: cannot save "),
        "unexpected: {message}"
    );
    assert_eq!(message.matches('\n').count(), 1, "one line: {message}");
}

#[test]
fn every_must_be_1_or_more_also_with_json() {
    let scratch = ScratchDir::new("split-every-usage");
    let input = scratch.pdf("doc.pdf", 2);
    let folder = scratch.file("parts");

    for every in ["0", "abc", "1.5"] {
        let result = split(&["--json", "--every", every], &folder, &input);

        assert_eq!(result.status.code(), Some(2), "{every:?}");
        assert_eq!(stdout(&result), "", "{every:?}");
        assert!(
            stderr(&result).contains("--every <N>"),
            "{every:?} gave: {}",
            stderr(&result)
        );
    }
    assert_eq!(scratch.entries(), ["doc.pdf"]);
}

#[test]
fn the_new_commands_refuse_an_incomplete_command_line() {
    let scratch = ScratchDir::new("new-usage");
    let input = scratch.pdf("doc.pdf", 2);
    let out = scratch.file("out.pdf");

    let info_without_input = cli(&[OsStr::new("info"), OsStr::new("--json")]);
    let extract_without_pages = cli(&[
        OsStr::new("extract"),
        OsStr::new("--output"),
        out.as_os_str(),
        input.as_os_str(),
    ]);
    let split_without_folder = cli(&[OsStr::new("split"), input.as_os_str()]);

    for result in [
        info_without_input,
        extract_without_pages,
        split_without_folder,
    ] {
        assert_eq!(result.status.code(), Some(2));
        assert_eq!(stdout(&result), "");
        assert!(!stderr(&result).is_empty(), "stderr should explain");
    }
    assert_eq!(scratch.entries(), ["doc.pdf"]);
}

// render

#[test]
fn render_with_json_prints_exactly_one_object_and_writes_a_png() {
    let scratch = ScratchDir::new("render-json");
    // Page n of this file is 500 + n points wide and 842 points high.
    let input = scratch.pdf("in.pdf", 3);
    let before = fs::read(&input).unwrap();
    let out = scratch.file("page.png");

    let result = render(&["--json", "--width", "200"], "2", &out, &input);

    assert_eq!(result.status.code(), Some(0), "{}", stdout(&result));
    // Page 2 is 502 by 842 points: 200 * 842 / 502 = 335.4, rounded to 335.
    assert_eq!(
        json_report(&result),
        json!({
            "ok": true,
            "operation": "render",
            "output": text(&out),
            "page": 2,
            "width": 200,
            "height": 335,
        })
    );
    // The order of the keys, which a parsed value has lost.
    let line = stdout(&result);
    let keys = [
        r#""ok":"#,
        r#""operation":"#,
        r#""output":"#,
        r#""page":"#,
        r#""width":"#,
        r#""height":"#,
    ];
    let places: Vec<usize> = keys.iter().map(|key| line.find(key).unwrap()).collect();
    assert!(places.is_sorted(), "unexpected order: {line}");

    // The file is a PNG of the size the report gives. The page is empty,
    // so every pixel is paper: white, and fully opaque.
    let (width, height, pixels) = decode_png(&out);
    assert_eq!((width, height), (200, 335));
    // `as_chunks` looks at the same bytes four at a time, as `[u8; 4]`
    // values, and hands back whatever is left over at the end.
    let (pixels, rest) = pixels.as_chunks::<4>();
    assert!(rest.is_empty(), "the image does not end on a whole pixel");
    assert_eq!(pixels.len(), 200 * 335);
    assert!(
        pixels.iter().all(|pixel| *pixel == [255; 4]),
        "an empty page should be all white and opaque"
    );
    // The input is only read, and nothing else was written.
    assert_eq!(fs::read(&input).unwrap(), before, "the input was changed");
    assert_eq!(scratch.entries(), ["in.pdf", "page.png"]);
}

#[test]
fn render_prints_one_human_line_and_draws_1024_pixels_wide_by_default() {
    let scratch = ScratchDir::new("render-human");
    let input = scratch.save("a4.pdf", with_one_page(595, 842));
    let out = scratch.file("page.png");

    let result = render(&[], "1", &out, &input);

    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    // An A4 page: 1024 * 842 / 595 = 1449.1, rounded to 1449.
    assert_eq!(
        stdout(&result),
        format!(
            "Rendered page 1 (1024 by 1449 pixels) into {}\n",
            out.display()
        )
    );
    assert_eq!(stderr(&result), "");
    let (width, height, _) = decode_png(&out);
    assert_eq!((width, height), (1024, 1449));
}

#[test]
fn render_reports_the_size_it_drew_when_a_page_is_too_tall() {
    let scratch = ScratchDir::new("render-tall");
    // 10 by 2000 points: 100 pixels wide would mean 20000 pixels high.
    let input = scratch.save("tall.pdf", with_one_page(10, 2000));
    let out = scratch.file("tall.png");

    let result = render(&["--json", "--width", "100"], "1", &out, &input);

    assert_eq!(result.status.code(), Some(0), "{}", stdout(&result));
    // The whole page is drawn smaller, to fit the tallest image there is:
    // the report gives that size, not the width that was asked for.
    let report = json_report(&result);
    assert_eq!(report["width"], 41);
    assert_eq!(report["height"], danonpdf_core::MAX_RENDER_HEIGHT);
    let (width, height, _) = decode_png(&out);
    assert_eq!((width, height), (41, danonpdf_core::MAX_RENDER_HEIGHT));
}

#[test]
fn render_accepts_the_widest_image_and_png_in_any_letter_case() {
    let scratch = ScratchDir::new("render-widest");
    let input = scratch.save("wide.pdf", with_one_page(400, 100));
    let out = scratch.file("PAGE.PNG");
    let widest = danonpdf_core::MAX_RENDER_WIDTH.to_string();

    let result = render(&["--json", "--width", widest.as_str()], "1", &out, &input);

    assert_eq!(result.status.code(), Some(0), "{}", stderr(&result));
    let report = json_report(&result);
    assert_eq!(report["width"], danonpdf_core::MAX_RENDER_WIDTH);
    assert_eq!(report["height"], danonpdf_core::MAX_RENDER_WIDTH / 4);
    assert_eq!(scratch.entries(), ["PAGE.PNG", "wide.pdf"]);
}

#[test]
fn render_usage_errors_exit_with_2_also_with_json() {
    let scratch = ScratchDir::new("render-usage");
    let input = scratch.pdf("in.pdf", 2);
    let input = text(&input);
    let (png, jpg, bare) = (
        scratch.file("page.png"),
        scratch.file("page.jpg"),
        scratch.file("page"),
    );
    let (png, jpg, bare) = (text(&png), text(&jpg), text(&bare));
    let too_wide = (danonpdf_core::MAX_RENDER_WIDTH + 1).to_string();

    // What follows `render` on each command line, and a few words that its
    // message must hold: the argument that is wrong or missing.
    let lines: [(&[&str], &str); 11] = [
        (&["--page", "0", "--output", png, input], "--page <N>"),
        (&["--page", "abc", "--output", png, input], "--page <N>"),
        (&["--page", "1.5", "--output", png, input], "--page <N>"),
        (
            &["--page", "1", "--width", "0", "--output", png, input],
            "--width <PX>",
        ),
        (
            &[
                "--page",
                "1",
                "--width",
                too_wide.as_str(),
                "--output",
                png,
                input,
            ],
            "--width <PX>",
        ),
        (
            &["--page", "1", "--width", "wide", "--output", png, input],
            "--width <PX>",
        ),
        (&["--page", "1", "--output", jpg, input], "must end in .png"),
        (
            &["--page", "1", "--output", bare, input],
            "must end in .png",
        ),
        (&["--output", png, input], "--page <N>"),
        (&["--page", "1", input], "--output <FILE>"),
        (&["--page", "1", "--output", png], "<INPUT>"),
    ];
    for (line, expected) in lines {
        // With `--json` too: the parser stops before the tool knows which
        // format was wanted, so the answer is plain text on stderr.
        for json in [false, true] {
            let mut args: Vec<&OsStr> = vec![OsStr::new("render")];
            if json {
                args.push(OsStr::new("--json"));
            }
            args.extend(line.iter().map(OsStr::new));

            let result = cli(&args);

            assert_eq!(result.status.code(), Some(2), "{line:?} json: {json}");
            assert_eq!(stdout(&result), "", "{line:?} json: {json}");
            let message = stderr(&result);
            assert!(
                message.starts_with("error: ") && message.contains(expected),
                "{line:?} json: {json} gave: {message}"
            );
        }
    }
    assert_eq!(scratch.entries(), ["in.pdf"]);
}

#[test]
fn render_of_a_page_past_the_end_is_page_out_of_range_and_writes_nothing() {
    let scratch = ScratchDir::new("render-past-end");
    let input = scratch.pdf("in.pdf", 3);
    let out = scratch.file("page.png");

    let result = render(&["--json"], "4", &out, &input);

    let error = failure_of(&result, "render", "page_out_of_range");
    // The failure is about the page asked for, not about a file.
    assert_eq!(error.get("path"), None);
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("page 4") && message.contains("3 pages"),
        "unexpected message: {message}"
    );
    assert_eq!(scratch.entries(), ["in.pdf"]);
}

#[test]
fn render_replaces_an_existing_output_only_with_force() {
    let scratch = ScratchDir::new("render-output-exists");
    let input = scratch.pdf("in.pdf", 1);
    let out = scratch.file("page.png");
    fs::write(&out, b"someone else's file").unwrap();

    let refused = render(&["--json"], "1", &out, &input);

    let error = failure_of(&refused, "render", "output_exists");
    assert_eq!(error["path"], text(&out));
    assert_eq!(fs::read(&out).unwrap(), b"someone else's file");

    let forced = render(&["--json", "--force", "--width", "50"], "1", &out, &input);

    assert_eq!(forced.status.code(), Some(0), "{}", stdout(&forced));
    let (width, _, _) = decode_png(&out);
    assert_eq!(width, 50);
}

#[test]
fn render_never_writes_over_its_input() {
    // An input is read whatever its name, so a PDF may be called
    // `scan.png`. Only then can the output of a render, which must end in
    // `.png`, be its own input.
    let scratch = ScratchDir::new("render-output-is-input");
    let input = scratch.pdf("scan.png", 1);
    let before = fs::read(&input).unwrap();
    // The input itself, spelled differently.
    let respelled = scratch.0.join(".").join("scan.png");

    for flags in [&["--json"][..], &["--json", "--force"][..]] {
        let result = render(flags, "1", &respelled, &input);

        let error = failure_of(&result, "render", "output_is_input");
        assert_eq!(error["path"], text(&respelled));
        assert_eq!(fs::read(&input).unwrap(), before, "the input was changed");
        assert_eq!(scratch.entries(), ["scan.png"]);
    }
}

#[test]
fn render_refuses_a_file_that_needs_a_password_and_draws_one_that_does_not() {
    let scratch = ScratchDir::new("render-encrypted");
    let locked = scratch.save("locked.pdf", encrypted(with_pages(1), "secret"));
    // Encrypted too, with the empty password: it opens for anyone.
    let open = scratch.save("open.pdf", encrypted(with_pages(1), ""));
    let out = scratch.file("page.png");

    let result = render(&["--json"], "1", &out, &locked);

    let error = failure_of(&result, "render", "encrypted");
    assert_eq!(error["path"], text(&locked));
    assert_eq!(error["message"], THE_FILE_IS_ENCRYPTED);
    assert!(!out.exists(), "nothing should be written");

    let result = render(&["--json", "--width", "100"], "1", &out, &open);

    assert_eq!(result.status.code(), Some(0), "{}", stdout(&result));
    let (width, _, _) = decode_png(&out);
    assert_eq!(width, 100);
}

#[test]
fn render_reports_what_it_cannot_read_with_its_path() {
    let scratch = ScratchDir::new("render-read-failed");
    let missing = scratch.file("missing.pdf");
    let notes = scratch.file("notes.pdf");
    fs::write(&notes, "just some text, not a PDF\n").unwrap();
    let out = scratch.file("page.png");

    let error = failure_of(
        &render(&["--json"], "1", &out, &missing),
        "render",
        "read_failed",
    );
    assert_eq!(error["path"], text(&missing));

    // A file that is there and is not a PDF has the code that the other
    // commands give it, and their words.
    let error = failure_of(
        &render(&["--json"], "1", &out, &notes),
        "render",
        "read_failed",
    );
    assert_eq!(error["path"], text(&notes));
    assert_eq!(
        error["message"],
        format!("cannot read {} as a PDF", notes.display())
    );
    assert_eq!(scratch.entries(), ["notes.pdf"]);
}

#[test]
fn render_into_a_missing_folder_is_save_failed() {
    let scratch = ScratchDir::new("render-missing-folder");
    let input = scratch.pdf("in.pdf", 1);
    let out = scratch.file("missing").join("page.png");

    let result = render(&["--json"], "1", &out, &input);

    let error = failure_of(&result, "render", "save_failed");
    assert_eq!(error["path"], text(&out));
    assert_eq!(scratch.entries(), ["in.pdf"]);
}

#[test]
fn info_and_render_can_disagree_about_a_damaged_file() {
    // `render` reads the file with PDFium; `info` and the other commands
    // read it with another library. AGENTS.md names these two cases.
    let scratch = ScratchDir::new("render-disagree");
    let out = scratch.file("page.png");

    // A file whose end is gone: the table that says where each object is,
    // and what follows it. `info` finds the pages without it.
    let whole = scratch.pdf("whole.pdf", 2);
    let bytes = fs::read(&whole).unwrap();
    let cut = scratch.file("cut.pdf");
    fs::write(&cut, without_its_end(&bytes)).unwrap();

    assert_eq!(json_report(&info(&["--json"], &cut))["pages"], 2);
    let error = failure_of(
        &render(&["--json"], "1", &out, &cut),
        "render",
        "read_failed",
    );
    assert_eq!(error["path"], text(&cut));

    // A file whose catalog names no page tree. For `info` it is damaged;
    // for PDFium it is a document without pages, so there is no page 1.
    let mut document = with_pages(1);
    let catalog_id = document
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let bare_catalog = Object::Dictionary(dictionary! { "Type" => "Catalog" });
    document.objects.insert(catalog_id, bare_catalog);
    let bare = scratch.save("bare.pdf", document);

    let error = failure_of(&info(&["--json"], &bare), "info", "broken_pdf");
    assert_eq!(error["path"], text(&bare));
    let error = failure_of(
        &render(&["--json"], "1", &out, &bare),
        "render",
        "page_out_of_range",
    );
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("0 pages"), "unexpected message: {message}");

    assert!(!out.exists(), "nothing should be written");
}

// render and the PDFium library

/// Runs a render that must fail because the library in `library_folder`
/// is missing or cannot be loaded, and returns its `error` object.
///
/// `failure_of` checks the whole contract of a failure with `--json`: exit
/// code 1, exactly one JSON object on stdout and nothing on stderr.
fn renderer_unavailable(library_folder: &Path, output: &Path, input: &Path) -> Value {
    let result = render_with_library(library_folder, &["--json"], "1", output, input);
    failure_of(&result, "render", "renderer_unavailable")
}

#[test]
fn render_without_the_library_is_renderer_unavailable() {
    let scratch = ScratchDir::new("render-no-library");
    let input = scratch.pdf("in.pdf", 1);
    let out = scratch.file("page.png");
    let empty = scratch.file("no-library-here");
    fs::create_dir(&empty).unwrap();

    let error = renderer_unavailable(&empty, &out, &input);

    // `path` is the file that was looked for: the folder asked for, and
    // the name the system gives the library.
    let library = PathBuf::from(error["path"].as_str().unwrap());
    assert_eq!(library.parent(), Some(empty.as_path()));
    let name = library.file_name().unwrap().to_string_lossy();
    assert!(name.contains("pdfium"), "unexpected library name {name}");
    assert_eq!(
        error["message"],
        format!("the PDFium library is missing: {}", library.display())
    );
    assert!(!out.exists(), "nothing should be written");

    // Without `--json`: the same reason, as one line on stderr.
    let result = render_with_library(&empty, &[], "1", &out, &input);

    assert_eq!(result.status.code(), Some(1));
    assert_eq!(stdout(&result), "");
    assert_eq!(
        stderr(&result),
        format!(
            "error: the PDFium library is missing: {}\n",
            library.display()
        )
    );
    assert!(!out.exists(), "nothing should be written");
}

#[test]
fn a_file_that_is_not_the_library_is_renderer_unavailable_too() {
    let scratch = ScratchDir::new("render-fake-library");
    let input = scratch.pdf("in.pdf", 1);
    let out = scratch.file("page.png");
    let folder = scratch.file("fake-library");
    fs::create_dir(&folder).unwrap();
    // The first failure names the file that was looked for; a file that is
    // not a library is then put exactly there.
    let missing = renderer_unavailable(&folder, &out, &input);
    let library = PathBuf::from(missing["path"].as_str().unwrap());
    fs::write(&library, "this is not a library").unwrap();

    let error = renderer_unavailable(&folder, &out, &input);

    assert_eq!(error["path"], text(&library));
    // What the system said about the file follows the path.
    let message = error["message"].as_str().unwrap();
    let start = format!("cannot load the PDFium library {}: ", library.display());
    assert!(
        message.starts_with(&start) && message.len() > start.len(),
        "unexpected message: {message}"
    );
    assert!(!out.exists(), "nothing should be written");
}

#[test]
fn render_looks_for_the_library_next_to_the_program() {
    // A folder laid out like `dist/`: the program, and next to it the
    // library. It is a copy of the program under test, because the folder
    // of the real one belongs to the build and must stay as it is.
    let scratch = ScratchDir::new("render-next-to-program");
    let input = scratch.pdf("in.pdf", 1);
    let out = scratch.file("page.png");
    let app = scratch.file("app");
    fs::create_dir(&app).unwrap();
    let built = Path::new(env!("CARGO_BIN_EXE_danonpdf-cli"));
    let program = app.join(built.file_name().unwrap());
    fs::copy(built, &program).unwrap();
    let render_from_app = |variable: Option<&str>| {
        let mut command = Command::new(&program);
        command.args(render_args(&["--json"], "1", &out, &input));
        match variable {
            Some(value) => command.env(PDFIUM_DIR, value),
            None => command.env_remove(PDFIUM_DIR),
        };
        run_a_fresh_copy(&mut command)
    };

    // The variable set to nothing counts as not set: the program looks in
    // its own folder, where there is no library yet.
    let error = failure_of(&render_from_app(Some("")), "render", "renderer_unavailable");
    let library = PathBuf::from(error["path"].as_str().unwrap());
    assert_eq!(
        fs::canonicalize(library.parent().unwrap()).unwrap(),
        fs::canonicalize(&app).unwrap()
    );
    assert!(!out.exists(), "nothing should be written");

    // With the library next to the program, and no variable at all, the
    // page is drawn.
    let real_folder = std::env::var_os(PDFIUM_DIR)
        .map(PathBuf::from)
        .expect("the build image names the folder of the PDFium library");
    fs::copy(real_folder.join(library.file_name().unwrap()), &library).unwrap();

    let result = render_from_app(None);

    assert_eq!(result.status.code(), Some(0), "{}", stdout(&result));
    assert_eq!(json_report(&result)["page"], 1);
    decode_png(&out);
}

#[test]
fn a_relative_library_folder_is_reported_as_an_absolute_path() {
    // AGENTS.md promises that `error.path` of `renderer_unavailable` is an
    // absolute path, also when the variable names a relative folder. Only
    // a run of the program can show it: the folder is taken from the
    // current folder of the process, and each run has its own.
    let scratch = ScratchDir::new("render-relative-library");
    let input = scratch.pdf("in.pdf", 1);
    let out = scratch.file("page.png");
    let empty = scratch.file("no-library-here");
    fs::create_dir(&empty).unwrap();

    // One relative word in the variable, and the program started in the
    // folder which that word is taken from.
    let result = run(Command::new(env!("CARGO_BIN_EXE_danonpdf-cli"))
        .args(render_args(&["--json"], "1", &out, &input))
        .current_dir(&scratch.0)
        .env(PDFIUM_DIR, "no-library-here"));

    let error = failure_of(&result, "render", "renderer_unavailable");
    let library = PathBuf::from(error["path"].as_str().unwrap());
    assert!(library.is_absolute(), "{}", library.display());
    // The folder is compared as the system resolves it: the temp folder
    // may be reached through a link, and then the program and this test
    // spell it differently.
    assert_eq!(
        fs::canonicalize(library.parent().unwrap()).unwrap(),
        fs::canonicalize(&empty).unwrap()
    );
    assert!(!out.exists(), "nothing should be written");
}

#[test]
fn the_commands_that_do_not_draw_do_not_need_the_library() {
    let scratch = ScratchDir::new("render-library-not-needed");
    let input = scratch.pdf("in.pdf", 3);
    let out = scratch.file("out.pdf");
    let empty = scratch.file("no-library-here");
    fs::create_dir(&empty).unwrap();
    let without_library = |args: &[&OsStr]| {
        run(Command::new(env!("CARGO_BIN_EXE_danonpdf-cli"))
            .args(args)
            .env(PDFIUM_DIR, &empty))
    };

    let counted = without_library(&[OsStr::new("info"), OsStr::new("--json"), input.as_os_str()]);
    let extracted = without_library(&[
        OsStr::new("extract"),
        OsStr::new("--json"),
        OsStr::new("--pages"),
        OsStr::new("2-"),
        OsStr::new("--output"),
        out.as_os_str(),
        input.as_os_str(),
    ]);

    assert_eq!(json_report(&counted)["pages"], 3);
    assert_eq!(json_report(&extracted)["pages"], 2);
}
