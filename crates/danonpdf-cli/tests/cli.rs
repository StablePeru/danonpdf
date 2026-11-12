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
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions, dictionary};
use serde_json::{Value, json};

/// Runs `danonpdf-cli` with `args` and waits for it to end.
///
/// `&[&OsStr]` and not `&[&str]`: a path is not always valid text, and
/// `OsStr` is what the system really passes to a program.
fn cli(args: &[&OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_danonpdf-cli"))
        .args(args)
        // The tool must never wait for input. With nothing to read, a
        // version that asked a question would fail here instead of hanging
        // the test run.
        .stdin(Stdio::null())
        .output()
        .expect("danonpdf-cli should start")
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
        let path = self.file(name);
        with_pages(page_count).save(&path).unwrap();
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

/// A document with `page_count` empty A4 pages.
fn with_pages(page_count: i64) -> Document {
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let kids: Vec<Object> = (0..page_count)
        .map(|_| {
            let page_id = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
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

/// How many pages the PDF at `path` has.
fn page_count(path: &Path) -> usize {
    Document::load(path).unwrap().get_pages().len()
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

/// Checks a failure report and returns its `error` object.
fn error_of(result: &Output, code: &str) -> Value {
    assert_eq!(result.status.code(), Some(1));
    let report = json_report(result);
    assert_eq!(report["ok"], false);
    assert_eq!(report["operation"], "merge");
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
    // the file for a caller that reads `path`.
    let scratch = ScratchDir::new("encrypted");
    let plain = scratch.pdf("plain.pdf", 1);
    let locked = scratch.file("locked.pdf");
    encrypted(with_pages(1), "secret").save(&locked).unwrap();
    let out = scratch.file("out.pdf");

    let result = merge(&["--json"], &out, &[&plain, &locked]);

    let error = error_of(&result, "encrypted");
    assert_eq!(error["path"], text(&locked));
    assert!(!out.exists(), "nothing should be written");
}

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
    // Three ways to ask; an agent that has never seen the tool may try any.
    let asks: [&[&str]; 3] = [&["--help"], &["help", "merge"], &["merge", "--help"]];
    for ask in asks {
        let args: Vec<&OsStr> = ask.iter().map(OsStr::new).collect();
        let result = cli(&args);

        assert_eq!(result.status.code(), Some(0), "{ask:?}");
        assert_eq!(stderr(&result), "", "{ask:?}");
        let help = stdout(&result);
        for expected in [
            // An example to copy.
            "danonpdf-cli merge --json --output merged.pdf first.pdf second.pdf",
            // The exit codes.
            "Exit codes:",
            // Both JSON shapes.
            r#"{"ok":true,"operation":"merge","output":"merged.pdf","inputs":2,"pages":12}"#,
            r#"{"ok":false,"operation":"merge","error":{"code":"output_exists","#,
            // Every error code.
            "output_is_input",
            "output_exists",
            "no_inputs",
            "read_failed",
            "encrypted",
            "broken_pdf",
            "save_failed",
        ] {
            assert!(help.contains(expected), "{ask:?} lacks {expected:?}");
        }
    }
}
