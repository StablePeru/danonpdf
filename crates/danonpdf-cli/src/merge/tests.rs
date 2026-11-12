//! Tests for the merge operation, called as a function. `tests/cli.rs`
//! checks the same rules again through the real program; here they are
//! checked where a failure points straight at the rule.

use std::fs;
use std::path::PathBuf;

use lopdf::{Document, Object, dictionary};

use super::*;
use crate::outcome::ErrorCode;
use crate::texts;

/// A folder under the system temp folder, deleted again when the value is
/// dropped, so a failing test does not leave files behind either.
struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new(test_name: &str) -> Self {
        // The process id keeps two test runs at the same time apart; the
        // test name keeps apart the tests of one run, which share a process.
        let name = format!("danonpdf-cli-unit-{}-{test_name}", std::process::id());
        let path = std::env::temp_dir().join(name);
        // Left over from an earlier run that was killed; no error if not.
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn file(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }

    /// Saves a PDF with `page_count` empty pages into this folder as `name`.
    fn pdf(&self, name: &str, page_count: i64) -> PathBuf {
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
        let catalog_id =
            document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog_id);

        let path = self.file(name);
        document.save(&path).unwrap();
        path
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// The operation

#[test]
fn run_reports_the_inputs_the_pages_and_the_output_as_given() {
    let scratch = ScratchDir::new("success");
    let inputs = [scratch.pdf("a.pdf", 2), scratch.pdf("b.pdf", 3)];
    let output = scratch.file("out.pdf");

    let report = run(&inputs, &output, false).unwrap();

    assert_eq!(
        report,
        Report::Merge {
            output: output.clone(),
            inputs: 2,
            pages: 5,
        }
    );
    assert!(output.exists(), "the merged file was not written");
}

#[test]
fn output_that_is_an_input_is_refused_whatever_force_says() {
    let scratch = ScratchDir::new("output-is-input");
    let inputs = [scratch.pdf("a.pdf", 1), scratch.pdf("b.pdf", 1)];
    let before = fs::read(&inputs[1]).unwrap();
    // The second input, spelled differently.
    let output = scratch.0.join(".").join("b.pdf");

    for force in [false, true] {
        let failure = run(&inputs, &output, force).unwrap_err();

        assert_eq!(failure.code, ErrorCode::OutputIsInput, "force: {force}");
        assert_eq!(failure.path, Some(output.clone()));
        assert_eq!(failure.message, texts::output_is_input(&output));
        assert_eq!(fs::read(&inputs[1]).unwrap(), before);
    }
}

#[test]
fn existing_output_is_refused_unless_forced() {
    let scratch = ScratchDir::new("output-exists");
    let inputs = [scratch.pdf("a.pdf", 1), scratch.pdf("b.pdf", 1)];
    let output = scratch.file("out.pdf");
    fs::write(&output, b"someone else's file").unwrap();

    let failure = run(&inputs, &output, false).unwrap_err();

    assert_eq!(failure.code, ErrorCode::OutputExists);
    assert_eq!(failure.path, Some(output.clone()));
    assert_eq!(failure.message, texts::output_exists(&output));
    assert_eq!(fs::read(&output).unwrap(), b"someone else's file");

    // The same call with `force` replaces it.
    let report = run(&inputs, &output, true).unwrap();
    assert_eq!(
        report,
        Report::Merge {
            output: output.clone(),
            inputs: 2,
            pages: 2,
        }
    );
    assert_ne!(fs::read(&output).unwrap(), b"someone else's file");
}

// Links only exist in this form on Unix-like systems; creating one on
// Windows needs a different function and, often, special rights.
#[cfg(unix)]
#[test]
fn a_link_to_nothing_still_counts_as_an_existing_output() {
    let scratch = ScratchDir::new("dangling-link");
    let inputs = [scratch.pdf("a.pdf", 1), scratch.pdf("b.pdf", 1)];
    let output = scratch.file("out.pdf");
    std::os::unix::fs::symlink(scratch.file("gone.pdf"), &output).unwrap();
    // `exists` follows the link, finds nothing, and says no.
    assert!(!output.exists());

    let failure = run(&inputs, &output, false).unwrap_err();

    assert_eq!(failure.code, ErrorCode::OutputExists);
    assert!(fs::symlink_metadata(&output).unwrap().is_symlink());
}

#[test]
fn a_failure_in_the_core_keeps_its_whole_chain_of_causes() {
    let scratch = ScratchDir::new("missing-input");
    let inputs = [scratch.pdf("a.pdf", 1), scratch.file("missing.pdf")];
    let output = scratch.file("out.pdf");

    let failure = run(&inputs, &output, false).unwrap_err();

    assert_eq!(failure.code, ErrorCode::ReadFailed);
    assert_eq!(failure.path, Some(inputs[1].clone()));
    // "cannot read ... as a PDF" is the core's own message; what follows the
    // colon is the system's reason, which the core keeps as the source.
    let start = format!("cannot read {} as a PDF: ", inputs[1].display());
    assert!(
        failure.message.starts_with(&start) && failure.message.len() > start.len(),
        "unexpected message: {}",
        failure.message
    );
    assert!(!output.exists(), "nothing should be written");
}
